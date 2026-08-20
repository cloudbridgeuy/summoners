//! The `EffectLeaf` interpreter. Every printed ability — a normal attack's
//! Damage, a Spell's Heal or draw — bottoms out in one of these leaves, and
//! this is the one place that turns a leaf plus its targets into a state
//! change and events. This module is not named in the design document's
//! module list; it exists because both `engine::resolution` (a normal
//! attack, rules §29–30) and `engine::stack` (a Spell's resolution, rules
//! §34) need the same per-leaf behavior and neither owns the other.
//!
//! `DealDamage` and `Heal` are positional (rules §30: targets are
//! Positions, never Summon identities), so a leaf silently misses if its
//! target Position is empty by the time it runs — the same miss semantics
//! `engine::resolution::resolve_attack` already used before this module
//! existed. `DrawCards` has no Position target; it always draws for
//! `controller` directly. `MoveSummon` and `SwapPositions` are also
//! positional, but their caller (`engine::skills::activate_skill`) validates
//! both positions before either leaf ever runs, since a Skill resolves
//! immediately rather than sitting on the Stack until a target might change
//! (rules §43) — so a miss here only ever means a caller supplied something
//! this interpreter cannot act on, not a legal in-fiction whiff.
//! `ProduceMana` delegates to `engine::upkeep::produce_mana` against the
//! named Summon's own printed Types. `ReadySummon` is positional against
//! `controller`'s own board. `BlockResponses` is the one remaining
//! documented no-op: it gates casting an Attack Spell as a response rather
//! than changing board state, so `engine::stack::cast_spell` reads its
//! condition and block directly instead of this interpreter.

use crate::domain::cards::{CardKind, EffectCondition, EffectLeaf, EffectSource, family};
use crate::domain::events::{BattlefieldTarget, DamageContext, GameEvent};
use crate::domain::ids::{CardInstanceId, PlayerId, Position};
use crate::domain::state::{
    DurationMarker, GameState, ManaSource, MovementStep, PlayerState, SummonInstance, WorkItem,
};
use crate::engine::{damage, upkeep};

/// Apply one printed `EffectLeaf`, controlled by `controller` — the
/// attacking or casting player — against `targets`. `DealDamage` reads its
/// target off `controller`'s opponent's board and enqueues a
/// `WorkItem::DestructionCheck` for a real hit (rules §23, §29–30);
/// `Heal` reads its target off `controller`'s own board and cannot reduce
/// Damage below zero (rules §22); `DrawCards` draws for `controller`
/// directly, ignoring `targets`, and always enqueues a
/// `WorkItem::LossCheck` (rules §2, §10 step 2, §58 — an empty Deck on a
/// forced draw is an immediate loss). `WorkItem::DestructionCheck` and
/// `WorkItem::LossCheck` are documented no-ops elsewhere in the engine
/// today; this interpreter only enqueues them. `MoveSummon` and
/// `SwapPositions` read and write `controller`'s own board and enqueue
/// `WorkItem::MovementTrigger`s (rules §28); `ProduceMana` and `ReadySummon`
/// are documented on their own functions below.
pub(crate) fn apply_leaf(
    state: &GameState,
    source: EffectSource,
    targets: &[Position],
    leaf: &EffectLeaf,
) -> (GameState, Vec<GameEvent>) {
    let controller = source.controller();
    match leaf {
        EffectLeaf::DealDamage(effect) => deal_damage(state, source, targets, effect),
        EffectLeaf::Heal { amount } => heal(state, controller, targets, *amount),
        EffectLeaf::DrawCards { amount } => draw_cards(state, controller, *amount),
        EffectLeaf::MoveSummon => move_summon(state, controller, targets),
        EffectLeaf::SwapPositions => swap_positions(state, controller, targets),
        EffectLeaf::ProduceMana => produce_mana_leaf(state, controller, targets),
        EffectLeaf::ReadySummon => ready_summon(state, controller, targets),
        EffectLeaf::CannotBeMovedByOpponent => {
            cannot_be_moved_by_opponent(state, controller, targets)
        }
        EffectLeaf::ReturnSpellFromDiscard => return_spell_from_discard(state, controller),
        EffectLeaf::ReturnSpellToDeckTop => return_spell_to_deck_top(state, controller),
        EffectLeaf::SwapOpposingPositions => swap_opposing_positions(state, controller, targets),
        EffectLeaf::LookAtPrizes => look_at_prizes(state, controller),
        // `BlockResponses` gates casting an Attack Spell as a response
        // rather than changing board state — its condition and block are
        // read by `engine::stack::cast_spell` directly, not here (rules
        // §32–33).
        EffectLeaf::BlockResponses { .. } => (state.clone(), Vec::new()),
    }
}

/// Whether `condition` currently holds for `controller`'s effect, read
/// against `targets` where the condition needs a defender (rules §30 for
/// `DefenderEnteredMainThisTurn`; the turn-global flag directly for
/// `SpellPlayedThisTurn`). Shared by `conditional_bonus` and
/// `engine::stack::cast_spell`'s `BlockResponses` gate, so both read the
/// same rule the same way.
pub(crate) fn condition_holds(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    condition: EffectCondition,
) -> bool {
    damage::condition_holds(state, controller, targets, condition)
}

/// Attach `DurationMarker::CannotBeMovedByOpponent` to `controller`'s own
/// Summon at `targets[0]` — the Old Sow's `Root and Renew` (rules §44). No
/// target, or an empty target Position, is a miss. The marker expires in
/// `engine::turn::handover`, only for the player whose Summon carries it,
/// the turn after it was set.
fn cannot_be_moved_by_opponent(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let Some(&position) = targets.first() else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let Some(summon) = summon_at_mut(state.players.get_mut(controller), position) else {
        return (state, Vec::new());
    };
    if !summon
        .duration_markers
        .contains(&DurationMarker::CannotBeMovedByOpponent)
    {
        summon
            .duration_markers
            .push(DurationMarker::CannotBeMovedByOpponent);
    }

    (state, Vec::new())
}

/// Return the first Spell-kind card from `controller`'s discard pile to
/// their hand — the Griefsinger's destruction trigger (rules §44). An empty
/// discard, or a discard with no Spell in it, is a silent no-op. The
/// design's "you may" is simplified to unconditional; the engine has no
/// optional-sub-effect vocabulary yet.
fn return_spell_from_discard(
    state: &GameState,
    controller: PlayerId,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let cards = state.cards.clone();
    let player_state = state.players.get_mut(controller);
    let Some(index) = player_state.discard.iter().position(|card_ref| {
        cards
            .get(card_ref.def)
            .is_some_and(|entity| family(entity) == CardKind::Spell)
    }) else {
        return (state, Vec::new());
    };
    let card = player_state.discard.remove(index);
    player_state.hand.push(card);

    (state, Vec::new())
}

/// Return the first Spell-kind card from `controller`'s hand to the top of
/// their deck — half of the Griefsinger's `Foresee` (rules §44). An empty
/// hand, or a hand with no Spell in it, is a silent no-op. The design's
/// "you may return" is likewise simplified to unconditional.
fn return_spell_to_deck_top(
    state: &GameState,
    controller: PlayerId,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let cards = state.cards.clone();
    let player_state = state.players.get_mut(controller);
    let Some(index) = player_state.hand.iter().position(|card_ref| {
        cards
            .get(card_ref.def)
            .is_some_and(|entity| family(entity) == CardKind::Spell)
    }) else {
        return (state, Vec::new());
    };
    let card = player_state.hand.remove(index);
    player_state.deck.insert(0, card);

    (state, Vec::new())
}

/// Reveal `controller`'s own face-down Prizes without changing them — the
/// Griefsinger's `Foresee` (rules §25, §44). This crate models no hidden
/// information: `GameState` is one plain, fully visible value (decision 3),
/// so a reader can already see `controller`'s Prizes without this leaf
/// running at all. Naming them on the event is therefore an honest
/// observation, not a new information leak — the same shape `CardDrawn` and
/// `PrizeRecovered` already use. Always emits, even against an empty Prize
/// pile, so a caller can see the look itself happened.
fn look_at_prizes(state: &GameState, controller: PlayerId) -> (GameState, Vec<GameEvent>) {
    let prizes: Vec<CardInstanceId> = state
        .players
        .get(controller)
        .prizes
        .iter()
        .map(|card_ref| card_ref.instance)
        .collect();

    (
        state.clone(),
        vec![GameEvent::PrizesViewed {
            player: controller,
            prizes,
        }],
    )
}

/// Exchange `controller`'s opponent's Main Summon with the Bench Summon at
/// `targets[0]` — the Warden's `Rearrange` (exchange branch only; rules
/// §16, `designs/types_archetypes.md` §5). Refuses the swap, as a silent
/// miss, when the opposing Main Summon carries
/// `DurationMarker::CannotBeMovedByOpponent` — the Old Sow's `Root and
/// Renew` answering it (`engine::skills::validate_swap_opposing_positions`
/// already rejects this before the leaf ever runs; this check is
/// defense-in-depth so the leaf is safe on its own). Enqueues the same four
/// movement triggers `swap_positions` does, but for the opponent's board;
/// `entered_main_this_turn` is likewise left for the queued `EnteringMain`
/// step to set, not set here.
fn swap_opposing_positions(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let Some(&target) = targets.first() else {
        return (state.clone(), Vec::new());
    };
    let Position::Bench(slot) = target else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let opponent = controller.opponent();
    let player_state = state.players.get_mut(opponent);
    let Some(vacating_main) = player_state.main.take() else {
        return (state, Vec::new());
    };
    if vacating_main
        .duration_markers
        .contains(&DurationMarker::CannotBeMovedByOpponent)
    {
        player_state.main = Some(vacating_main);
        return (state, Vec::new());
    }
    let Some(incoming_main) = player_state.bench[slot.index()].take() else {
        player_state.main = Some(vacating_main);
        return (state, Vec::new());
    };
    player_state.main = Some(incoming_main);
    player_state.bench[slot.index()] = Some(vacating_main);

    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingMain,
        opponent,
        Position::Bench(slot),
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringBench,
        opponent,
        Position::Bench(slot),
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingBench,
        opponent,
        Position::Main,
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringMain,
        opponent,
        Position::Main,
    ));

    (
        state,
        vec![GameEvent::SummonsSwapped {
            player: opponent,
            main: slot,
        }],
    )
}

/// Deal `amount` Damage to whichever Summon occupies `controller`'s
/// opponent's first target Position right now (rules §30). No target, or
/// an empty target Position, is a miss: nothing to damage, no event, and
/// no `DestructionCheck`.
fn deal_damage(
    state: &GameState,
    source: EffectSource,
    targets: &[Position],
    effect: &crate::domain::cards::DamageEffect,
) -> (GameState, Vec<GameEvent>) {
    let Some(&position) = targets.first() else {
        return (state.clone(), Vec::new());
    };
    let context = DamageContext {
        source: source.into(),
        target: BattlefieldTarget {
            controller: source.controller().opponent(),
            position,
        },
    };
    let intent = damage::DamageIntent {
        context,
        effect: effect.clone(),
    };
    let resolution = damage::evaluate(state, &intent);
    damage::commit(state, &resolution)
}

/// Remove up to `amount` accumulated Damage from whichever Summon occupies
/// `controller`'s own first target Position right now, never below zero
/// (rules §22). No target, or an empty target Position, is a miss: nothing
/// to heal, no event.
fn heal(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    amount: u32,
) -> (GameState, Vec<GameEvent>) {
    let Some(&position) = targets.first() else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let Some(summon) = summon_at_mut(state.players.get_mut(controller), position) else {
        return (state, Vec::new());
    };

    let removed = amount.min(summon.damage);
    summon.damage -= removed;

    (
        state,
        vec![GameEvent::Healed {
            position,
            amount: removed,
        }],
    )
}

/// Draw up to `amount` cards for `controller`, stopping early if the Deck
/// empties, and always enqueue a `WorkItem::LossCheck` for `controller`
/// afterward — even a zero-card draw still names the check the design's
/// resolution loop expects to see after a draw.
fn draw_cards(state: &GameState, controller: PlayerId, amount: u32) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();

    for _ in 0..amount {
        let player_state = state.players.get_mut(controller);
        if player_state.deck.is_empty() {
            break;
        }
        let card = player_state.deck.remove(0);
        player_state.hand.push(card);
        events.push(GameEvent::CardDrawn {
            player: controller,
            card: card.instance,
        });
    }

    state.work.push_back(WorkItem::LossCheck(controller));

    (state, events)
}

/// Relocate `controller`'s own Summon from `targets[0]` to the empty Bench
/// slot at `targets[1]` — never Main, since Main can never be voluntarily
/// emptied without a replacement (rules §27; `swap_positions` is that
/// replacement's leaf). Callers validate both positions before this runs
/// (`engine::skills::activate_skill` rejects an occupied destination or a
/// missing source with `ActionError::InvalidTarget` first), so this stays a
/// defensive no-op on any input it cannot act on, matching this
/// interpreter's other leaves. `ready` and every other field travel with the
/// moved Summon unchanged (movement, not a new arrival — the same reading
/// `engine::destruction::promote_from_slot` gives Promotion). Enqueues the
/// two movement triggers a Bench-to-Bench move fires (rules §28, §36).
fn move_summon(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let (Some(&from), Some(&to)) = (targets.first(), targets.get(1)) else {
        return (state.clone(), Vec::new());
    };
    let (Position::Bench(from_slot), Position::Bench(to_slot)) = (from, to) else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let player_state = state.players.get_mut(controller);
    if player_state.bench[to_slot.index()].is_some() {
        return (state, Vec::new());
    }
    let Some(summon) = player_state.bench[from_slot.index()].take() else {
        return (state, Vec::new());
    };
    player_state.bench[to_slot.index()] = Some(summon);

    // Rules §28: both steps are recorded against `to`, not `from` — the
    // moved Summon is the only one either step could name, and by the time
    // `engine::triggers::movement_trigger` drains these it already sits at
    // `to` (the same reading `engine::destruction::resolve_movement_
    // consequences` gives a one-way Promotion).
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingBench,
        controller,
        to,
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringBench,
        controller,
        to,
    ));

    (state, Vec::new())
}

/// Exchange `controller`'s Main Summon with the one at the Bench slot named
/// by `targets[0]` — the same exchange `engine::board::retreat` performs,
/// reached here through a Skill instead of the normal Retreat action (rules
/// §43). Callers validate both sides are occupied before this runs, so this
/// stays a defensive no-op otherwise. `ready` travels with each Summon
/// unchanged; `entered_main_this_turn` is left alone here and set instead
/// when the queued `EnteringMain` trigger below drains through
/// `engine::triggers::movement_trigger` — the one place in this crate that
/// sets it, since every path onto Main enqueues that same step. Enqueues the
/// same four movement triggers in the same fixed order (rules §28).
fn swap_positions(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let Some(&target) = targets.first() else {
        return (state.clone(), Vec::new());
    };
    let Position::Bench(slot) = target else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let player_state = state.players.get_mut(controller);
    let Some(vacating_main) = player_state.main.take() else {
        return (state, Vec::new());
    };
    let Some(incoming_main) = player_state.bench[slot.index()].take() else {
        player_state.main = Some(vacating_main);
        return (state, Vec::new());
    };
    player_state.main = Some(incoming_main);
    player_state.bench[slot.index()] = Some(vacating_main);

    // Rules §28: the swap above already moved both Summons, so each step
    // names the position its Summon occupies now — the same convention
    // `engine::board::retreat` uses for the same exchange. The Summon that
    // left Main is at `Bench(slot)` for both `LeavingMain` and
    // `EnteringBench`; the Summon that left the Bench is at `Main` for both
    // `LeavingBench` and `EnteringMain`.
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingMain,
        controller,
        Position::Bench(slot),
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringBench,
        controller,
        Position::Bench(slot),
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingBench,
        controller,
        Position::Main,
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringMain,
        controller,
        Position::Main,
    ));

    (
        state,
        vec![GameEvent::SummonsSwapped {
            player: controller,
            main: slot,
        }],
    )
}

/// Produce Mana from one Summon's own printed Types — `targets[0]` names its
/// position — rather than `controller`'s player-wide anchor (rules §11–12,
/// `ManaSource::Summon`). Delegates entirely to `engine::upkeep::produce_mana`
/// so a Skill-driven production pauses on the same
/// `PendingInput::ManaProduction` a multi-type Summon's natural production
/// would. No target is a no-op: nothing names which Summon produces.
fn produce_mana_leaf(
    state: &GameState,
    _controller: PlayerId,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let Some(&position) = targets.first() else {
        return (state.clone(), Vec::new());
    };
    upkeep::produce_mana(state, ManaSource::Summon(position))
}

/// Turn Ready the Summon at `controller`'s own `targets[0]` (rules §53). No
/// target, or an empty target Position, is a miss: nothing to Ready, no
/// event.
fn ready_summon(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let Some(&position) = targets.first() else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let Some(summon) = summon_at_mut(state.players.get_mut(controller), position) else {
        return (state, Vec::new());
    };
    summon.ready = true;

    (
        state,
        vec![GameEvent::SummonsReadied {
            player: controller,
            positions: vec![position],
        }],
    )
}

/// The Summon at `position`, mutably, if any.
fn summon_at_mut(player: &mut PlayerState, position: Position) -> Option<&mut SummonInstance> {
    match position {
        Position::Main => player.main.as_mut(),
        Position::Bench(slot) => player.bench[slot.index()].as_mut(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
