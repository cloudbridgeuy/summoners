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
//! `controller`'s own board. Every other leaf is a documented no-op until a
//! later fixture gives it behavior.

use crate::domain::cards::EffectLeaf;
use crate::domain::events::GameEvent;
use crate::domain::ids::{PlayerId, Position};
use crate::domain::state::{
    GameState, ManaSource, MovementStep, PlayerState, SummonInstance, WorkItem,
};
use crate::engine::upkeep;

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
    controller: PlayerId,
    targets: &[Position],
    leaf: &EffectLeaf,
) -> (GameState, Vec<GameEvent>) {
    match leaf {
        EffectLeaf::DealDamage { amount, .. } => deal_damage(state, controller, targets, *amount),
        EffectLeaf::Heal { amount } => heal(state, controller, targets, *amount),
        EffectLeaf::DrawCards { amount } => draw_cards(state, controller, *amount),
        EffectLeaf::MoveSummon => move_summon(state, controller, targets),
        EffectLeaf::SwapPositions => swap_positions(state, controller, targets),
        EffectLeaf::ProduceMana => produce_mana_leaf(state, controller, targets),
        EffectLeaf::ReadySummon => ready_summon(state, controller, targets),
        EffectLeaf::ConditionalBonus { .. }
        | EffectLeaf::BlockResponses(_)
        | EffectLeaf::ReturnSpellFromDiscard
        | EffectLeaf::LookAtPrizes
        | EffectLeaf::ReturnSpellToDeckTop
        | EffectLeaf::CannotBeMovedByOpponent => (state.clone(), Vec::new()),
    }
}

/// Deal `amount` Damage to whichever Summon occupies `controller`'s
/// opponent's first target Position right now (rules §30). No target, or
/// an empty target Position, is a miss: nothing to damage, no event, and
/// no `DestructionCheck`.
fn deal_damage(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    amount: u32,
) -> (GameState, Vec<GameEvent>) {
    let Some(&position) = targets.first() else {
        return (state.clone(), Vec::new());
    };

    let mut state = state.clone();
    let defender = controller.opponent();
    let Some(summon) = summon_at_mut(state.players.get_mut(defender), position) else {
        return (state, Vec::new());
    };

    let before = summon.damage;
    let after = before.saturating_add(amount);
    summon.damage = after;
    state.work.push_back(WorkItem::DestructionCheck(position));

    (
        state,
        vec![GameEvent::DamageApplied {
            position,
            before,
            after,
        }],
    )
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

    state
        .work
        .push_back(WorkItem::MovementTrigger(MovementStep::LeavingBench, from));
    state
        .work
        .push_back(WorkItem::MovementTrigger(MovementStep::EnteringBench, to));

    (state, Vec::new())
}

/// Exchange `controller`'s Main Summon with the one at the Bench slot named
/// by `targets[0]` — the same exchange `engine::board::retreat` performs,
/// reached here through a Skill instead of the normal Retreat action (rules
/// §43). Callers validate both sides are occupied before this runs, so this
/// stays a defensive no-op otherwise. `ready` travels with each Summon
/// unchanged; only `entered_main_this_turn` is set on the Summon now
/// entering Main, matching `retreat`'s own bookkeeping. Enqueues the same
/// four movement triggers in the same fixed order (rules §28).
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
    let Some(mut incoming_main) = player_state.bench[slot.index()].take() else {
        player_state.main = Some(vacating_main);
        return (state, Vec::new());
    };
    incoming_main.entered_main_this_turn = true;
    player_state.main = Some(incoming_main);
    player_state.bench[slot.index()] = Some(vacating_main);

    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingMain,
        Position::Main,
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringBench,
        Position::Bench(slot),
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingBench,
        Position::Bench(slot),
    ));
    state.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringMain,
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
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{BenchSlot, CardInstanceId};
    use crate::domain::state::{
        CardRef, ManaBank, PerPlayer, Phase, PlayerState, TurnState, UpgradeChain,
    };
    use std::collections::VecDeque;

    fn whelp(owner: PlayerId) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(1),
                    def: CardDefId("quarry-whelp"),
                },
                vec![],
            ),
            damage: 0,
            ready: false,
            owner,
            controller: owner,
            duration_markers: vec![],
            played_this_turn: false,
            upgraded_this_turn: false,
            entered_main_this_turn: false,
        }
    }

    fn player_state(owner: PlayerId) -> PlayerState {
        PlayerState {
            main: Some(whelp(owner)),
            bench: [None, None, None],
            deck: vec![
                CardRef {
                    instance: CardInstanceId(10),
                    def: CardDefId("quarry-whelp"),
                },
                CardRef {
                    instance: CardInstanceId(11),
                    def: CardDefId("quarry-whelp"),
                },
            ],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            has_coin: false,
        }
    }

    fn base_state() -> GameState {
        GameState {
            players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
            turn: TurnState {
                active_player: PlayerId::One,
                phase: Phase::Main,
                window: None,
                normal_attack_used: false,
                normal_retreat_used: false,
                spell_played_this_turn: false,
            },
            stack: vec![],
            stack_segment_bases: vec![],
            work: VecDeque::new(),
            pending: None,
            outcome: None,
        }
    }

    #[test]
    fn deal_damage_hits_the_defenders_main_and_enqueues_a_destruction_check() {
        let state = base_state();
        let leaf = EffectLeaf::DealDamage {
            amount: 10,
            immutable: false,
        };

        let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert_eq!(
            events,
            vec![GameEvent::DamageApplied {
                position: Position::Main,
                before: 0,
                after: 10,
            }]
        );
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::DestructionCheck(Position::Main)])
        );
    }

    #[test]
    fn deal_damage_on_an_empty_position_is_a_silent_miss() {
        let state = base_state();
        let leaf = EffectLeaf::DealDamage {
            amount: 10,
            immutable: false,
        };

        let (state, events) = apply_leaf(
            &state,
            PlayerId::One,
            &[Position::Bench(BenchSlot::First)],
            &leaf,
        );

        assert!(events.is_empty());
        assert!(state.work.is_empty());
    }

    #[test]
    fn deal_damage_with_no_target_is_a_silent_miss() {
        let state = base_state();
        let leaf = EffectLeaf::DealDamage {
            amount: 10,
            immutable: false,
        };

        let (state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

        assert!(events.is_empty());
        assert!(state.work.is_empty());
    }

    #[test]
    fn heal_removes_damage_from_the_casters_own_main() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            damage: 15,
            ..whelp(PlayerId::One)
        });
        let leaf = EffectLeaf::Heal { amount: 20 };

        let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert_eq!(
            events,
            vec![GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            }]
        );
        assert_eq!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .damage,
            0
        );
    }

    #[test]
    fn heal_cannot_reduce_damage_below_zero() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            damage: 5,
            ..whelp(PlayerId::One)
        });
        let leaf = EffectLeaf::Heal { amount: 20 };

        let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert_eq!(
            events,
            vec![GameEvent::Healed {
                position: Position::Main,
                amount: 5,
            }]
        );
        assert_eq!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .damage,
            0
        );
    }

    #[test]
    fn heal_on_an_empty_position_is_a_silent_miss() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = None;
        let leaf = EffectLeaf::Heal { amount: 20 };

        let (_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert!(events.is_empty());
    }

    #[test]
    fn draw_cards_draws_for_the_controller_and_enqueues_a_loss_check() {
        let state = base_state();
        let leaf = EffectLeaf::DrawCards { amount: 1 };

        let (state, events) = apply_leaf(&state, PlayerId::Two, &[], &leaf);

        assert_eq!(
            events,
            vec![GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: CardInstanceId(10),
            }]
        );
        assert_eq!(state.players.get(PlayerId::Two).hand.len(), 1);
        assert_eq!(state.players.get(PlayerId::Two).deck.len(), 1);
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)])
        );
    }

    #[test]
    fn draw_cards_stops_early_once_the_deck_empties() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).deck = vec![CardRef {
            instance: CardInstanceId(20),
            def: CardDefId("quarry-whelp"),
        }];
        let leaf = EffectLeaf::DrawCards { amount: 5 };

        let (state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

        assert_eq!(events.len(), 1, "only one card was available to draw");
        assert!(state.players.get(PlayerId::One).deck.is_empty());
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::LossCheck(PlayerId::One)]),
            "a LossCheck is still enqueued even though the request over-asked"
        );
    }

    #[test]
    fn every_other_leaf_is_a_documented_no_op() {
        let state = base_state();
        let leaves = [
            EffectLeaf::ConditionalBonus {
                condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
                amount: 5,
            },
            EffectLeaf::BlockResponses(crate::domain::cards::ResponseBlock::AttackSpells),
            EffectLeaf::ReturnSpellFromDiscard,
            EffectLeaf::LookAtPrizes,
            EffectLeaf::ReturnSpellToDeckTop,
            EffectLeaf::CannotBeMovedByOpponent,
        ];

        for leaf in leaves {
            let (next_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);
            assert!(events.is_empty());
            assert_eq!(next_state, state);
        }
    }

    #[test]
    fn move_summon_relocates_a_bench_summon_to_an_empty_bench_slot_and_enqueues_its_triggers() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).bench[0] = Some(whelp(PlayerId::One));
        let leaf = EffectLeaf::MoveSummon;

        let (state, events) = apply_leaf(
            &state,
            PlayerId::One,
            &[
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            &leaf,
        );

        assert!(events.is_empty());
        assert!(state.players.get(PlayerId::One).bench[0].is_none());
        assert!(state.players.get(PlayerId::One).bench[1].is_some());
        assert_eq!(
            state.work,
            VecDeque::from(vec![
                WorkItem::MovementTrigger(
                    MovementStep::LeavingBench,
                    Position::Bench(BenchSlot::First)
                ),
                WorkItem::MovementTrigger(
                    MovementStep::EnteringBench,
                    Position::Bench(BenchSlot::Second)
                ),
            ])
        );
    }

    #[test]
    fn move_summon_onto_an_occupied_destination_is_a_silent_miss() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).bench[0] = Some(whelp(PlayerId::One));
        state.players.get_mut(PlayerId::One).bench[1] = Some(whelp(PlayerId::One));
        let leaf = EffectLeaf::MoveSummon;

        let (next_state, events) = apply_leaf(
            &state,
            PlayerId::One,
            &[
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            &leaf,
        );

        assert!(events.is_empty());
        assert_eq!(next_state, state);
    }

    #[test]
    fn move_summon_from_an_empty_source_is_a_silent_miss() {
        let state = base_state();
        let leaf = EffectLeaf::MoveSummon;

        let (next_state, events) = apply_leaf(
            &state,
            PlayerId::One,
            &[
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            &leaf,
        );

        assert!(events.is_empty());
        assert_eq!(next_state, state);
    }

    #[test]
    fn swap_positions_exchanges_main_and_the_named_bench_slot_and_enqueues_the_four_triggers() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).bench[0] = Some(whelp(PlayerId::One));
        let leaf = EffectLeaf::SwapPositions;

        let (state, events) = apply_leaf(
            &state,
            PlayerId::One,
            &[Position::Bench(BenchSlot::First)],
            &leaf,
        );

        assert_eq!(
            events,
            vec![GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            }]
        );
        assert!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .entered_main_this_turn
        );
        assert!(state.players.get(PlayerId::One).bench[0].is_some());
        assert_eq!(
            state.work,
            VecDeque::from(vec![
                WorkItem::MovementTrigger(MovementStep::LeavingMain, Position::Main),
                WorkItem::MovementTrigger(
                    MovementStep::EnteringBench,
                    Position::Bench(BenchSlot::First)
                ),
                WorkItem::MovementTrigger(
                    MovementStep::LeavingBench,
                    Position::Bench(BenchSlot::First)
                ),
                WorkItem::MovementTrigger(MovementStep::EnteringMain, Position::Main),
            ])
        );
    }

    #[test]
    fn swap_positions_against_an_empty_bench_slot_is_a_silent_miss() {
        let state = base_state();
        let leaf = EffectLeaf::SwapPositions;

        let (next_state, events) = apply_leaf(
            &state,
            PlayerId::One,
            &[Position::Bench(BenchSlot::First)],
            &leaf,
        );

        assert!(events.is_empty());
        assert_eq!(next_state, state);
    }

    #[test]
    fn produce_mana_leaf_banks_the_named_summons_own_printed_type() {
        let state = base_state();
        let leaf = EffectLeaf::ProduceMana;

        let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert_eq!(
            events,
            vec![GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Summon(Position::Main),
                mana_type: crate::domain::ids::ManaType::Matter,
            }]
        );
        assert_eq!(state.players.get(PlayerId::One).mana.matter, 1);
    }

    #[test]
    fn produce_mana_leaf_with_no_target_is_a_no_op() {
        let state = base_state();
        let leaf = EffectLeaf::ProduceMana;

        let (next_state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

        assert!(events.is_empty());
        assert_eq!(next_state, state);
    }

    #[test]
    fn ready_summon_turns_the_targeted_summon_ready() {
        let state = base_state();
        let leaf = EffectLeaf::ReadySummon;

        let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert_eq!(
            events,
            vec![GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            }]
        );
        assert!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .ready
        );
    }

    #[test]
    fn ready_summon_on_an_empty_position_is_a_silent_miss() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = None;
        let leaf = EffectLeaf::ReadySummon;

        let (_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

        assert!(events.is_empty());
    }
}
