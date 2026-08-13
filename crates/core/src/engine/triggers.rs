//! Triggered Ability discovery and firing (rules §28, §36–41).
//!
//! Discovery never processes a trigger inline: `discover_front` and
//! `discover_back` each read the ordered candidates able to fire one event
//! right now and queue one `WorkItem::FireTrigger` per candidate — to the
//! front when a destruction chain is already mid-drain (rules §41 still
//! resolves ahead of the rest of that chain), to the back when nothing else
//! is queued yet (Upkeep). The existing work-queue drain
//! (`engine::resolution::drain`) then fires them one at a time, so a
//! respondable trigger that opens a Priority window mid-discovery pauses
//! the rest of the queue exactly the way any other paused decision does —
//! no extra state is needed to remember what is left.
//!
//! A movement trigger (rules §28) never has more than one candidate — the
//! one Summon that just moved — so `movement_trigger` fires it directly
//! instead of going through discovery.

use crate::domain::cards::{EffectLeaf, Query, QueryResult, TriggerEvent, find_def};
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

/// The event, respondability, and effects of the `CardNode::Trigger` on
/// `player`'s Summon at `position`, if it exists and matches `event`.
fn trigger_at(
    state: &GameState,
    player: PlayerId,
    position: Position,
    event: TriggerEvent,
) -> Option<(bool, Vec<EffectLeaf>)> {
    let summon = summon_at(state, player, position)?;
    let def = find_def(&state.cards, summon.chain.top().def)?;
    match def.find(Query::Trigger) {
        Some(QueryResult::Trigger {
            event: found,
            respondable,
            effects,
        }) if found == event => Some((respondable, effects)),
        _ => None,
    }
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

/// Every `(player, position)` able to fire `event` right now, across
/// `players` in the order given, Main before Bench within each player
/// (rules §41).
fn candidates(
    state: &GameState,
    players: &[PlayerId],
    event: TriggerEvent,
) -> Vec<(PlayerId, Position)> {
    players
        .iter()
        .flat_map(|&player| {
            controlled_positions(state, player)
                .into_iter()
                .filter(move |&position| trigger_at(state, player, position, event).is_some())
                .map(move |position| (player, position))
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
    for (player, position) in candidates(&state, players, event).into_iter().rev() {
        state
            .work
            .push_front(WorkItem::FireTrigger(player, position, event));
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
    for (player, position) in candidates(&state, players, event) {
        state
            .work
            .push_back(WorkItem::FireTrigger(player, position, event));
    }
    state
}

/// The implicit target(s) a trigger's effects apply against, since
/// `CardNode::Trigger` carries no explicit target field: a `Heal` targets
/// the triggering Summon itself; a `DealDamage` targets the opposing Main,
/// matching how a normal Attack and every vanilla Spell fixture already
/// name their target.
fn implicit_targets(position: Position, effects: &[EffectLeaf]) -> Vec<Position> {
    if effects
        .iter()
        .any(|leaf| matches!(leaf, EffectLeaf::DealDamage { .. }))
    {
        vec![Position::Main]
    } else {
        vec![position]
    }
}

/// Fire one Triggered Ability: emit `TriggerFired`, then either apply its
/// effects immediately (rules §39) or push it onto the Stack as a new
/// segment and open a window for the opponent of its controller (rules
/// §37–38).
fn fire(
    state: &GameState,
    player: PlayerId,
    position: Position,
    event: TriggerEvent,
    trigger: (bool, Vec<EffectLeaf>),
) -> (GameState, Vec<GameEvent>) {
    let (respondable, effects) = trigger;
    let mut state = state.clone();
    let mut events = vec![GameEvent::TriggerFired {
        controller: player,
        position,
        event,
    }];
    let targets = implicit_targets(position, &effects);

    if respondable {
        state.stack_segment_bases.push(state.stack.len());
        state.stack.push(StackItem::Trigger {
            controller: player,
            source: position,
            event,
            targets,
            effects,
        });
        state.turn.window = Some(stack::window_after_play(player));
    } else {
        let (next_state, leaf_events) =
            resolution::apply_leaves(&state, player, &targets, &effects);
        state = next_state;
        events.extend(leaf_events);
    }

    (state, events)
}

/// `WorkItem::MovementTrigger` (rules §28): mark a Main arrival, then fire
/// whatever Trigger this Summon carries for the step's mapped event, if
/// any.
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
    match trigger_at(&state, player, position, event) {
        Some(trigger) => fire(&state, player, position, event, trigger),
        None => (state, Vec::new()),
    }
}

/// `WorkItem::FireTrigger`: fire the Trigger discovery already found and
/// queued, re-reading it fresh rather than trusting a stored copy.
pub(crate) fn fire_queued(
    state: &GameState,
    player: PlayerId,
    position: Position,
    event: TriggerEvent,
) -> (GameState, Vec<GameEvent>) {
    match trigger_at(state, player, position, event) {
        Some(trigger) => fire(state, player, position, event, trigger),
        None => (state.clone(), Vec::new()),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::fixtures;
    use crate::domain::ids::{BenchSlot, CardInstanceId, Position};
    use crate::domain::state::{
        CardRef, GameStatus, ManaBank, PerPlayer, Phase, PlayerState, TurnState, UpgradeChain,
    };
    use std::collections::VecDeque;

    fn card(def: &'static str) -> CardRef {
        CardRef {
            instance: CardInstanceId(1),
            def: fixtures::id(def),
        }
    }

    fn summon_of(def: &'static str, owner: PlayerId) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(card(def), vec![]),
            damage: 0,
            ready: true,
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
            main: Some(summon_of("quarry-whelp", owner)),
            bench: [None, None, None],
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            has_coin: false,
            enchantments: vec![],
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
            status: GameStatus::Playing,
            cards: fixtures::card_set(),
        }
    }

    #[test]
    fn movement_event_maps_every_step_to_its_fixed_event() {
        assert_eq!(
            movement_event(MovementStep::LeavingMain),
            TriggerEvent::LeavesMain
        );
        assert_eq!(
            movement_event(MovementStep::EnteringBench),
            TriggerEvent::EntersBench
        );
        assert_eq!(
            movement_event(MovementStep::LeavingBench),
            TriggerEvent::LeavesBench
        );
        assert_eq!(
            movement_event(MovementStep::EnteringMain),
            TriggerEvent::EntersMain
        );
    }

    #[test]
    fn movement_trigger_fires_an_immediate_heal_on_entering_main() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            damage: 20,
            ..summon_of("hearth-warden", PlayerId::One)
        });

        let (state, events) = movement_trigger(
            &state,
            MovementStep::EnteringMain,
            PlayerId::One,
            Position::Main,
        );

        assert_eq!(
            events,
            vec![
                GameEvent::TriggerFired {
                    controller: PlayerId::One,
                    position: Position::Main,
                    event: TriggerEvent::EntersMain,
                },
                GameEvent::Healed {
                    position: Position::Main,
                    amount: 15,
                },
            ]
        );
        let healed = state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main");
        assert_eq!(healed.damage, 5);
        assert!(healed.entered_main_this_turn);
    }

    #[test]
    fn movement_trigger_is_a_no_op_with_no_matching_trigger_node() {
        let state = base_state();

        let (next_state, events) = movement_trigger(
            &state,
            MovementStep::LeavingMain,
            PlayerId::One,
            Position::Main,
        );

        assert_eq!(next_state, state);
        assert!(events.is_empty());
    }

    #[test]
    fn movement_trigger_still_sets_entered_main_with_no_matching_trigger_node() {
        let state = base_state();

        let (state, events) = movement_trigger(
            &state,
            MovementStep::EnteringMain,
            PlayerId::One,
            Position::Main,
        );

        assert!(events.is_empty());
        assert!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .entered_main_this_turn
        );
    }

    #[test]
    fn fire_queued_opens_a_respondable_window_for_the_opponent_of_the_controller() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(summon_of("spite-thorn", PlayerId::Two));

        let (state, events) = fire_queued(
            &state,
            PlayerId::Two,
            Position::Main,
            TriggerEvent::AnySummonDestroyed,
        );

        assert_eq!(
            events,
            vec![GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
            }]
        );
        assert_eq!(state.stack_segment_bases, vec![0]);
        assert_eq!(
            state.stack,
            vec![StackItem::Trigger {
                controller: PlayerId::Two,
                source: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                targets: vec![Position::Main],
                effects: vec![EffectLeaf::DealDamage {
                    amount: 15,
                    immutable: false,
                }],
            }]
        );
        assert_eq!(
            state.turn.window,
            Some(crate::domain::state::StackWindow {
                holder: PlayerId::One,
                prior_pass: false,
            })
        );
    }

    #[test]
    fn discover_back_orders_opponent_of_active_first_then_main_then_bench() {
        let mut state = base_state();
        state.turn.active_player = PlayerId::One;
        state.players.get_mut(PlayerId::One).main = Some(summon_of("spite-thorn", PlayerId::One));
        state.players.get_mut(PlayerId::Two).main = Some(summon_of("spite-thorn", PlayerId::Two));
        state.players.get_mut(PlayerId::Two).bench[0] =
            Some(summon_of("spite-thorn", PlayerId::Two));

        let state = discover_back(
            &state,
            &destruction::ordered_players(&state),
            TriggerEvent::AnySummonDestroyed,
        );

        let queued: Vec<_> = state.work.into_iter().collect();
        assert_eq!(
            queued,
            vec![
                WorkItem::FireTrigger(
                    PlayerId::Two,
                    Position::Main,
                    TriggerEvent::AnySummonDestroyed
                ),
                WorkItem::FireTrigger(
                    PlayerId::Two,
                    Position::Bench(BenchSlot::First),
                    TriggerEvent::AnySummonDestroyed
                ),
                WorkItem::FireTrigger(
                    PlayerId::One,
                    Position::Main,
                    TriggerEvent::AnySummonDestroyed
                ),
            ]
        );
    }

    #[test]
    fn discover_front_pushes_ahead_of_work_already_queued() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(summon_of("spite-thorn", PlayerId::One));
        state.work = VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)]);

        let state = discover_front(&state, &[PlayerId::One], TriggerEvent::AnySummonDestroyed);

        let queued: Vec<_> = state.work.into_iter().collect();
        assert_eq!(
            queued,
            vec![
                WorkItem::FireTrigger(
                    PlayerId::One,
                    Position::Main,
                    TriggerEvent::AnySummonDestroyed
                ),
                WorkItem::LossCheck(PlayerId::Two),
            ]
        );
    }

    #[test]
    fn implicit_targets_heal_the_trigger_source_and_damage_the_opposing_main() {
        assert_eq!(
            implicit_targets(
                Position::Bench(BenchSlot::First),
                &[EffectLeaf::Heal { amount: 5 }]
            ),
            vec![Position::Bench(BenchSlot::First)]
        );
        assert_eq!(
            implicit_targets(
                Position::Bench(BenchSlot::First),
                &[EffectLeaf::DealDamage {
                    amount: 5,
                    immutable: false
                }]
            ),
            vec![Position::Main]
        );
    }
}
