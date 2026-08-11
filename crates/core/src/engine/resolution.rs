//! The resolution loop's work-queue drain (the design's resolution loop,
//! step 1 only). It pops `WorkItem`s from `state.work` while `pending` and
//! `outcome` stay unset, and executes each one in order. Stack resolution
//! (steps 2–3: draining the Stack once a double pass is recorded) is not
//! built yet — nothing puts anything on the Stack yet either — so this file
//! drains the work queue only. Later work extends `drain` to also resolve
//! the Stack once `work` runs dry.

use crate::domain::events::GameEvent;
use crate::domain::state::{GameState, WorkItem};
use crate::engine::{loss, upkeep};

/// Drain `state.work` until it is empty, a decision pauses the loop
/// (`pending` becomes set), or the game ends (`outcome` becomes set — rules
/// §2: losing is immediate, so nothing queued after that point runs).
/// Returns the resulting state and every event produced along the way, in
/// order.
pub(crate) fn drain(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();

    while state.pending.is_none() && state.outcome.is_none() {
        let Some(item) = state.work.pop_front() else {
            break;
        };
        let (next_state, item_events) = execute(&state, &item);
        state = next_state;
        events.extend(item_events);
    }

    (state, events)
}

/// Run one `WorkItem`, returning the resulting state and the events it
/// produced.
fn execute(state: &GameState, item: &WorkItem) -> (GameState, Vec<GameEvent>) {
    match item {
        WorkItem::ReadyAll => upkeep::ready_all(state),
        WorkItem::DrawCard => execute_draw(state),
        WorkItem::ProduceMana(source) => upkeep::produce_mana(state, *source),

        // Destruction, promotion, movement triggers, ability triggers, and
        // the general loss check (§24, §28, §36–38) have no implementation
        // yet. Draining one of these items today does nothing and produces
        // no event: an honest, documented no-op rather than a panic, so the
        // loop can keep moving once later work starts enqueueing them for
        // real.
        WorkItem::DestructionCheck(_)
        | WorkItem::DiscardDestroyedChain(_)
        | WorkItem::RecordMainLoss(_)
        | WorkItem::RecoverPrize(_)
        | WorkItem::PromoteBenchSummon(_)
        | WorkItem::ResolveMovementConsequences(_)
        | WorkItem::MovementTrigger(_, _)
        | WorkItem::FireTrigger(_, _)
        | WorkItem::LossCheck(_) => (state.clone(), Vec::new()),
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{CardInstanceId, PlayerId, Position};
    use crate::domain::state::{
        CardRef, ManaBank, ManaSource, PendingInput, PerPlayer, Phase, PlayerState, SummonInstance,
        TurnState, UpgradeChain,
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
            deck: vec![CardRef {
                instance: CardInstanceId(10),
                def: CardDefId("quarry-whelp"),
            }],
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
                active_player: PlayerId::Two,
                phase: Phase::Upkeep,
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
    fn drain_runs_the_full_upkeep_sequence_in_order() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ]);

        let (state, events) = drain(&state);

        assert!(state.work.is_empty());
        assert_eq!(
            events,
            vec![
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::CardDrawn {
                    player: PlayerId::Two,
                    card: CardInstanceId(10),
                },
                GameEvent::ManaProduced {
                    player: PlayerId::Two,
                    source: ManaSource::Player,
                    mana_type: crate::domain::ids::ManaType::Matter,
                },
            ]
        );
    }

    #[test]
    fn drain_stops_as_soon_as_produce_mana_pauses_on_a_choice() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(2),
                    def: CardDefId("set-path-adept"),
                },
                vec![],
            ),
            ..whelp(PlayerId::Two)
        });
        state.work = VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ]);

        let (state, events) = drain(&state);

        assert_eq!(
            state.pending,
            Some(PendingInput::ManaProduction {
                player: PlayerId::Two,
                source: ManaSource::Player,
            })
        );
        assert!(state.work.is_empty());
        assert_eq!(
            events.len(),
            2,
            "ReadyAll and DrawCard ran; production paused before its own event"
        );
    }

    #[test]
    fn drain_stops_immediately_once_a_draw_failure_sets_outcome() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).deck = vec![];
        state.work = VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ]);

        let (state, events) = drain(&state);

        assert!(state.outcome.is_some());
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::ProduceMana(ManaSource::Player)]),
            "the loop checks outcome before popping the next item, so the \
             unrun item is left queued rather than executed — harmless, since \
             a finished game rejects every later action outright"
        );
        assert_eq!(
            events,
            vec![
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::GameEnded {
                    winner: PlayerId::One,
                    reason: crate::domain::state::LossReason::EmptyDeckDraw,
                },
            ],
            "CardDrawn never fires and ManaProduced never runs after the loss"
        );
    }

    #[test]
    fn drain_leaves_a_finished_game_untouched_even_with_queued_work() {
        let mut state = base_state();
        state.outcome = Some(crate::domain::state::GameOutcome {
            winner: PlayerId::One,
            reason: crate::domain::state::LossReason::EmptyDeckDraw,
        });
        state.work = VecDeque::from(vec![WorkItem::ReadyAll]);

        let (state, events) = drain(&state);

        assert!(events.is_empty());
        assert_eq!(state.work, VecDeque::from(vec![WorkItem::ReadyAll]));
    }

    #[test]
    fn drain_treats_not_yet_built_work_items_as_silent_no_ops() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![
            WorkItem::LossCheck(PlayerId::One),
            WorkItem::FireTrigger(
                Position::Main,
                crate::domain::cards::TriggerEvent::YourUpkeep,
            ),
        ]);

        let (state, events) = drain(&state);

        assert!(events.is_empty());
        assert!(state.work.is_empty());
        assert_eq!(state.outcome, None);
    }
}
