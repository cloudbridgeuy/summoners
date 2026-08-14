//! The resolution loop's full three-step contract: once a Priority window
//! has closed, resolve whatever is left on the current Stack segment
//! strictly top-first (step 1, rules §35) — a segment's own item(s) must
//! finish before anything queued underneath it resumes (rules §38, §40) —
//! then drain `state.work`, including whatever the segment's resolution
//! just fed back into it (step 2), and finally rest once both are settled
//! (step 3). A respondable trigger opening a new window mid-drain (rules
//! §38) pauses the whole loop immediately, leaving the rest of `work` — and
//! the segment base the trigger just pushed — exactly where they are until
//! that window closes.

use crate::domain::cards::{Attack, CardSet, EffectLeaf, EntityId, Persistent};
use crate::domain::events::GameEvent;
use crate::domain::ids::{PlayerId, Position};
use crate::domain::state::{CardRef, GameState, StackItem, WorkItem};
use crate::engine::{destruction, effects, loss, triggers, upkeep};

/// Drain `state.work`, then the Stack, until both are settled, a decision
/// pauses the loop (`pending` becomes set), or `state.status` leaves
/// `Playing` — the game ends (rules §2: losing is immediate, so nothing
/// queued after that point runs) or breaks (a rule demanded a component no
/// entity printed; whatever is still queued in `work` at that point is left
/// exactly where it is, unread). Returns the resulting state and every
/// event produced along the way, in order.
pub(crate) fn drain(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();

    loop {
        if state.pending.is_some() || !state.status.is_playing() {
            break;
        }

        // A respondable trigger can open a Priority window mid-drain
        // (rules §38): once that happens, the rest of `work` — including
        // any `FireTrigger` items discovery queued behind it — must wait
        // for the window to close, the same way step 2 already waits
        // before touching the Stack.
        if state.turn.window.is_some() {
            break;
        }

        // Step 1: the window is closed (the loop above already broke out
        // otherwise), so if the current Stack segment still holds
        // something, resolve its top item first (rules §35 — strictly
        // top-first). A segment interrupts whatever `work` was doing to
        // open its own window (rules §38); once that window closes, the
        // segment's own item(s) must finish — and its base must come off
        // `stack_segment_bases` — before the `work` items it interrupted
        // resume underneath it (rules §40). Whatever this feeds back into
        // `work` is picked up by step 2 on a later iteration, once this
        // segment (and any it is nested in) is fully settled.
        if stack_has_unresolved_items(&state) {
            let (next_state, item_events) = resolve_top_stack_item(&state);
            state = next_state;
            events.extend(item_events);
            continue;
        }

        // Step 2: the current Stack segment is settled. Drain `work`.
        if let Some(item) = state.work.pop_front() {
            let (next_state, item_events) = execute(&state, &item);
            state = next_state;
            events.extend(item_events);
            continue;
        }

        // Step 3: both the current Stack segment and the work queue are
        // settled — rest here until the next action.
        break;
    }

    (state, events)
}

/// Whether the current Stack segment still holds an item to resolve. With
/// no segment open, the base defaults to 0 and this is the whole Stack;
/// with one open (rules §38 — a respondable trigger, and anything played
/// in response to it), it is only the part above that segment's base, so
/// an outer, interrupted segment cannot be touched until this one empties
/// back down to where it started.
fn stack_has_unresolved_items(state: &GameState) -> bool {
    let base = state.stack_segment_bases.last().copied().unwrap_or(0);
    state.stack.len() > base
}

/// Run one `WorkItem`, returning the resulting state and the events it
/// produced.
fn execute(state: &GameState, item: &WorkItem) -> (GameState, Vec<GameEvent>) {
    match item {
        WorkItem::ReadyAll => upkeep::ready_all(state),
        WorkItem::DrawCard => execute_draw(state),
        WorkItem::ProduceMana(source) => upkeep::produce_mana(state, *source),
        WorkItem::BeginMainPhase => upkeep::begin_main_phase(state),

        WorkItem::DestructionCheck(position) => destruction::check(state, *position),
        WorkItem::DiscardDestroyedChain(position) => {
            destruction::discard_destroyed_chain(state, *position)
        }
        WorkItem::RecordMainLoss(player) => destruction::record_main_loss(state, *player),
        WorkItem::RecoverPrize(player) => destruction::recover_prize(state, *player),
        WorkItem::PromoteBenchSummon(player) => destruction::promote_bench_summon(state, *player),
        WorkItem::ResolveMovementConsequences(player) => {
            destruction::resolve_movement_consequences(state, *player)
        }

        WorkItem::MovementTrigger(step, player, position) => {
            triggers::movement_trigger(state, *step, *player, *position)
        }
        WorkItem::FireTrigger(player, position, event) => {
            triggers::fire_queued(state, *player, *position, *event)
        }

        WorkItem::LossCheck(player) => loss::check(state, *player),
    }
}

/// `WorkItem::DrawCard`: draw for the active player, and treat an empty
/// Deck as the immediate loss it is (rules §2, §10 step 2, §58).
fn execute_draw(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let (state, mut events, outcome) = upkeep::draw_card(state);
    match outcome {
        upkeep::DrawOutcome::Drew => (state, events),
        upkeep::DrawOutcome::DeckEmpty => {
            let player = state.turn.active_player;
            let (state, loss_events) = loss::draw_failure(&state, player);
            events.extend(loss_events);
            (state, events)
        }
    }
}

/// Resolve the item on top of the Stack (rules §35: the Stack always
/// resolves top-first), through the shared `engine::effects` interpreter.
fn resolve_top_stack_item(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let Some(item) = state.stack.pop() else {
        return (state, Vec::new());
    };

    let mut events = vec![GameEvent::StackItemResolved { item: item.clone() }];

    match item {
        StackItem::Attack { attacker, target } => {
            let (next_state, leaf_events) = resolve_attack(&state, attacker, target);
            state = next_state;
            events.extend(leaf_events);
        }
        StackItem::Spell {
            caster,
            card,
            targets,
        } => {
            let (next_state, leaf_events) = resolve_spell(&state, caster, card, &targets);
            state = next_state;
            events.extend(leaf_events);
        }
        StackItem::Trigger {
            controller,
            targets,
            effects,
            ..
        } => {
            let (next_state, leaf_events) = apply_leaves(&state, controller, &targets, &effects);
            state = next_state;
            events.extend(leaf_events);
        }
    }

    // Rules §38, §40: once popping that item drains the Stack back down to
    // the current segment's own base, the segment is settled — drop the
    // base so the next iteration's `stack_has_unresolved_items` reads
    // whatever segment (or the implicit one at 0) sits below it, letting
    // the `work` that segment interrupted resume underneath it.
    if state.stack_segment_bases.last() == Some(&state.stack.len()) {
        state.stack_segment_bases.pop();
    }

    (state, events)
}

/// Apply an attack's printed effects against `target`, re-reading the
/// attacker's Attack node at resolution time rather than trusting whatever
/// was printed when the attack was declared, since `StackItem::Attack`
/// carries no effects field of its own (rules §30).
fn resolve_attack(
    state: &GameState,
    attacker: PlayerId,
    target: Position,
) -> (GameState, Vec<GameEvent>) {
    apply_leaves(
        state,
        attacker,
        &[target],
        &attacker_effects(state, attacker),
    )
}

/// Apply a Spell's or an Enchantment's printed effects against `targets`,
/// then place the resolved card in its post-resolution zone: a card
/// printing `Persistent` stays in play, added to its caster's
/// `enchantments` (rules §44: "Enchantments ... remain in play after
/// resolving ... until an effect removes it"); every other card moves to
/// its caster's discard pile, the same way a destroyed upgrade chain does
/// (rules §56). Persistence is a printed fact, not a card family — see
/// `Persistent`'s own doc comment.
fn resolve_spell(
    state: &GameState,
    caster: PlayerId,
    card: CardRef,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let (mut state, events) = apply_leaves(
        state,
        caster,
        targets,
        &spell_effects(&state.cards, card.def),
    );
    let persists = state
        .cards
        .get(card.def)
        .is_some_and(|entity| entity.get::<Persistent>().is_some());
    let player_state = state.players.get_mut(caster);
    if persists {
        player_state.enchantments.push(card);
    } else {
        player_state.discard.push(card);
    }
    (state, events)
}

/// Run every effect leaf in order through the shared interpreter, folding
/// its state and events forward. `pub(crate)` so `engine::triggers` can
/// resolve an immediate trigger's effects through the same single
/// interpreter path as an attack, a Spell, and a respondable trigger's own
/// Stack item. Rules §30: when `leaves` names an immutable `DealDamage`
/// (the Old Sow's `Root and Renew`-adjacent Attack text), any
/// `ConditionalBonus` in the same list is skipped outright rather than
/// resolved and left to find its condition false — the one "increase" this
/// crate's vocabulary has never runs against Damage the printed text says
/// cannot be increased.
pub(crate) fn apply_leaves(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    leaves: &[EffectLeaf],
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();
    let damage_is_immutable = effects::immutable_damage_in(leaves);
    for leaf in leaves {
        if damage_is_immutable && matches!(leaf, EffectLeaf::ConditionalBonus { .. }) {
            continue;
        }
        let (next_state, leaf_events) = effects::apply_leaf(&state, controller, targets, leaf);
        state = next_state;
        events.extend(leaf_events);
    }
    (state, events)
}

/// The attacker's currently printed Attack effects, read off the nested
/// entity `Component::Attack` wraps — not off the card itself. No `Attack`
/// component at all, the same as an unresolvable card, answers with no
/// effects.
fn attacker_effects(state: &GameState, attacker: PlayerId) -> Vec<EffectLeaf> {
    let Some(main_summon) = &state.players.get(attacker).main else {
        return Vec::new();
    };
    let Some(entity) = state.cards.get(main_summon.chain.top().def) else {
        return Vec::new();
    };
    let Some(attack) = entity.get::<Attack>() else {
        return Vec::new();
    };
    attack.all::<EffectLeaf>().into_iter().cloned().collect()
}

/// A Spell's or an Enchantment's printed effects, read straight off the
/// top-level entity — neither wraps a nested entity of its own the way an
/// Attack does, so both families read the same components the same way. An
/// unresolvable card answers with no effects.
fn spell_effects(cards: &CardSet, def_id: EntityId) -> Vec<EffectLeaf> {
    let Some(entity) = cards.get(def_id) else {
        return Vec::new();
    };
    entity.all::<EffectLeaf>().into_iter().cloned().collect()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
