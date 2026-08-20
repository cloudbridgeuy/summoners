//! Triggered Ability discovery and firing (rules §28, §36–41).
//!
//! Discovery never processes a trigger inline: `discover_front` and
//! `discover_back` each read the ordered candidates able to fire one event
//! right now — one candidate per matching Trigger ability, not per Summon,
//! since a card may print more than one Trigger matching the same event —
//! and queue one `WorkItem::FireTrigger` per candidate, naming the ability
//! by its `EntityId` so it can be told apart from a sibling ability printed
//! on the same card. Candidates queue to the front when a destruction chain
//! is already mid-drain (rules §41 still resolves ahead of the rest of that
//! chain), to the back when nothing else is queued yet (Upkeep). The
//! existing work-queue drain (`engine::resolution::drain`) then fires them
//! one at a time, so a respondable trigger that opens a Priority window
//! mid-discovery pauses the rest of the queue exactly the way any other
//! paused decision does — no extra state is needed to remember what is
//! left.
//!
//! A movement trigger (rules §28) never has more than one candidate Summon
//! — the one that just moved — but that Summon may itself print several
//! Trigger abilities matching the step's mapped event, so `movement_trigger`
//! queues one `WorkItem::FireTrigger` per matching ability to the front of
//! `work`, the same way discovery does, rather than firing any of them
//! directly: firing more than one inline would have nowhere to remember an
//! untried remainder if the first of several opened a Priority window.

use crate::domain::cards::{
    EffectLeaf, EffectSource, Entity, EntityId, Respondable, Trigger, TriggerEvent,
};
use crate::domain::events::GameEvent;
use crate::domain::ids::PlayerId;
use crate::domain::ids::Position;
use crate::domain::state::{GameState, MovementStep, StackItem, SummonInstance, WorkItem};
use crate::engine::{destruction, resolution, stack};

/// The `TriggerEvent` a movement step fires (rules §28, §36).
fn movement_event(step: MovementStep) -> TriggerEvent {
    match step {
        MovementStep::LeavingMain => TriggerEvent::LeavesMain,
        MovementStep::EnteringBench => TriggerEvent::EntersBench,
        MovementStep::LeavingBench => TriggerEvent::LeavesBench,
        MovementStep::EnteringMain => TriggerEvent::EntersMain,
    }
}

/// The Summon `player` controls at `position`, if any.
fn summon_at(state: &GameState, player: PlayerId, position: Position) -> Option<&SummonInstance> {
    match position {
        Position::Main => state.players.get(player).main.as_ref(),
        Position::Bench(slot) => state.players.get(player).bench[slot.index()].as_ref(),
    }
}

/// The printed `Entity` for `player`'s Summon at `position`'s topmost card,
/// if any.
fn card_at(state: &GameState, player: PlayerId, position: Position) -> Option<&Entity> {
    let summon = summon_at(state, player, position)?;
    state.cards.get(summon.chain.top().def)
}

/// Every Trigger ability `player`'s Summon at `position` prints that
/// matches `event`, in authored order — zero, one, or many. Reads with
/// `all`, not `get`: the container expresses multiplicity through the read,
/// not the data, and this is the read that needs every match, not just the
/// first.
fn matching_abilities(
    state: &GameState,
    player: PlayerId,
    position: Position,
    event: TriggerEvent,
) -> Vec<EntityId> {
    let Some(card) = card_at(state, player, position) else {
        return Vec::new();
    };
    card.all::<Trigger>()
        .into_iter()
        .filter(|ability| ability.get::<TriggerEvent>() == Some(&event))
        .map(|ability| ability.id)
        .collect()
}

/// The respondability and effects of the Trigger ability named `ability` on
/// `player`'s Summon at `position`, re-read fresh rather than trusted from
/// discovery — the same fresh-lookup-by-id shape
/// `engine::skills::activate_skill` uses for a Skill. `None` when the
/// Summon, its card, or that ability itself is no longer there: a queued
/// ability that has since vanished is not a break, it fires nothing, the
/// same as a candidate that never existed.
fn ability_at(
    state: &GameState,
    player: PlayerId,
    position: Position,
    ability: EntityId,
) -> Option<(bool, Vec<EffectLeaf>)> {
    let card = card_at(state, player, position)?;
    let trigger_entity = card
        .all::<Trigger>()
        .into_iter()
        .find(|entity| entity.id == ability)?;
    Some((
        trigger_entity.get::<Respondable>().is_some(),
        trigger_entity
            .all::<EffectLeaf>()
            .into_iter()
            .cloned()
            .collect(),
    ))
}

/// Every `player`-controlled position holding a live Summon, Main before
/// Bench in index order (rules §41's within-one-player rule).
fn controlled_positions(state: &GameState, player: PlayerId) -> Vec<Position> {
    let mut positions = Vec::new();
    if state.players.get(player).main.is_some() {
        positions.push(Position::Main);
    }
    positions.extend(
        destruction::occupied_bench_slots(state, player)
            .into_iter()
            .map(Position::Bench),
    );
    positions
}

/// Every `(player, position, ability)` able to fire `event` right now,
/// across `players` in the order given, Main before Bench within each
/// player (rules §41), and in authored order among several abilities on the
/// same card.
fn candidates(
    state: &GameState,
    players: &[PlayerId],
    event: TriggerEvent,
) -> Vec<(PlayerId, Position, EntityId)> {
    players
        .iter()
        .flat_map(|&player| {
            controlled_positions(state, player)
                .into_iter()
                .flat_map(move |position| {
                    matching_abilities(state, player, position, event)
                        .into_iter()
                        .map(move |ability| (player, position, ability))
                })
        })
        .collect()
}

/// Queue one ordered `WorkItem::FireTrigger` per candidate to the front of
/// `work`, in rules §41 order. Used mid-drain, when the remaining steps of
/// the destruction chain that is already queued must stay contiguous but
/// resolve after these (see `engine::destruction`'s module doc comment).
pub(crate) fn discover_front(
    state: &GameState,
    players: &[PlayerId],
    event: TriggerEvent,
) -> GameState {
    let mut state = state.clone();
    for (player, position, ability) in candidates(&state, players, event).into_iter().rev() {
        state
            .work
            .push_front(WorkItem::FireTrigger(player, position, event, ability));
    }
    state
}

/// Queue one ordered `WorkItem::FireTrigger` per candidate to the back of
/// `work`, in rules §41 order. Used when nothing else is queued ahead of
/// these yet (Upkeep).
pub(crate) fn discover_back(
    state: &GameState,
    players: &[PlayerId],
    event: TriggerEvent,
) -> GameState {
    let mut state = state.clone();
    for (player, position, ability) in candidates(&state, players, event) {
        state
            .work
            .push_back(WorkItem::FireTrigger(player, position, event, ability));
    }
    state
}

/// The implicit target(s) a trigger's effects apply against, since a
/// Trigger ability carries no explicit target field: a `Heal` targets the
/// triggering Summon itself; a `DealDamage` targets the opposing Main,
/// matching how a normal Attack and every vanilla Spell fixture already
/// name their target.
fn implicit_targets(position: Position, effects: &[EffectLeaf]) -> Vec<Position> {
    if effects
        .iter()
        .any(|leaf| matches!(leaf, EffectLeaf::DealDamage(_)))
    {
        vec![Position::Main]
    } else {
        vec![position]
    }
}

/// Which ability is firing, for whom, and why — bundled so `fire` stays
/// under the file's argument-count cap the same way `SkillActivation`
/// already does for `activate_skill`.
#[derive(Clone, Copy)]
struct FiringTrigger {
    player: PlayerId,
    position: Position,
    event: TriggerEvent,
    ability: EntityId,
}

/// Fire one Triggered Ability: emit `TriggerFired`, then either apply its
/// effects immediately (rules §39) or push it onto the Stack as a new
/// segment and open a window for the opponent of its controller (rules
/// §37–38).
fn fire(
    state: &GameState,
    firing: FiringTrigger,
    trigger: (bool, Vec<EffectLeaf>),
) -> (GameState, Vec<GameEvent>) {
    let FiringTrigger {
        player,
        position,
        event,
        ability,
    } = firing;
    let (respondable, effects) = trigger;
    let mut state = state.clone();
    let mut events = vec![GameEvent::TriggerFired {
        controller: player,
        position,
        event,
        ability,
    }];
    let targets = implicit_targets(position, &effects);

    if respondable {
        state.stack_segment_bases.push(state.stack.len());
        state.stack.push(StackItem::Trigger {
            controller: player,
            source: position,
            ability,
            event,
            targets,
            effects,
        });
        state.turn.window = Some(stack::window_after_play(player));
    } else {
        let (next_state, leaf_events) = resolution::apply_leaves(
            &state,
            EffectSource::Trigger {
                controller: player,
                position,
                ability,
            },
            &targets,
            &effects,
        );
        state = next_state;
        events.extend(leaf_events);
    }

    (state, events)
}

/// `WorkItem::MovementTrigger` (rules §28): mark a Main arrival, then queue
/// one `WorkItem::FireTrigger` per Trigger ability this Summon carries that
/// matches the step's mapped event, to the front of `work` so each fires
/// before the next queued step.
pub(crate) fn movement_trigger(
    state: &GameState,
    step: MovementStep,
    player: PlayerId,
    position: Position,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    if step == MovementStep::EnteringMain
        && position == Position::Main
        && let Some(summon) = state.players.get_mut(player).main.as_mut()
    {
        summon.entered_main_this_turn = true;
    }

    let event = movement_event(step);
    for ability in matching_abilities(&state, player, position, event)
        .into_iter()
        .rev()
    {
        state
            .work
            .push_front(WorkItem::FireTrigger(player, position, event, ability));
    }

    (state, Vec::new())
}

/// `WorkItem::FireTrigger`: fire the Trigger ability discovery already
/// found and queued, re-reading it fresh rather than trusting a stored
/// copy.
pub(crate) fn fire_queued(
    state: &GameState,
    player: PlayerId,
    position: Position,
    event: TriggerEvent,
    ability: EntityId,
) -> (GameState, Vec<GameEvent>) {
    match ability_at(state, player, position, ability) {
        Some(trigger) => fire(
            state,
            FiringTrigger {
                player,
                position,
                event,
                ability,
            },
            trigger,
        ),
        None => (state.clone(), Vec::new()),
    }
}

#[cfg(test)]
mod tests;
