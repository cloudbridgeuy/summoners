//! Recovered resignation behavior, extended across both seats and every
//! pending-input variant. Only status changes, even with unresolved work.

use super::*;
use crate::domain::cards::{Breakage, ComponentKind};
use crate::domain::state::WorkItem;

fn resign(player: PlayerId) -> GameAction {
    GameAction::Resign { player }
}

fn assert_resignation(state: &GameState, player: PlayerId) {
    let before = state.clone();
    let expected_outcome = GameOutcome {
        winner: player.opponent(),
        reason: LossReason::Resignation,
    };
    let expected = GameState {
        status: GameStatus::Ended(expected_outcome),
        ..before.clone()
    };
    let outcome = apply(state, &resign(player)).expect("Playing permits resignation");
    assert_eq!(outcome.state, expected);
    assert_eq!(*state, before);
    assert_eq!(
        outcome.events,
        vec![GameEvent::GameEnded {
            winner: expected_outcome.winner,
            reason: expected_outcome.reason,
        }]
    );
    for actor in [PlayerId::One, PlayerId::Two] {
        for action in [resign(actor), end_turn(actor)] {
            assert_eq!(
                apply(&outcome.state, &action),
                Err(ActionError::GameAlreadyOver)
            );
        }
    }
}

#[test]
fn either_player_can_resign_inside_or_outside_their_turn_and_priority() {
    for player in [PlayerId::One, PlayerId::Two] {
        for active in [PlayerId::One, PlayerId::Two] {
            let mut state = base_state();
            state.turn.active_player = active;
            assert_resignation(&state, player);
            for holder in [PlayerId::One, PlayerId::Two] {
                state.turn.phase = Phase::Combat;
                state.turn.window = Some(StackWindow {
                    holder,
                    prior_pass: false,
                });
                assert_resignation(&state, player);
            }
        }
    }
}

#[test]
fn every_pending_choice_preserves_all_state_except_status() {
    for player in [PlayerId::One, PlayerId::Two] {
        for answerer in [PlayerId::One, PlayerId::Two] {
            for pending in [
                PendingInput::ManaProduction {
                    player: answerer,
                    source: ManaSource::Player,
                },
                PendingInput::ManaProduction {
                    player: answerer,
                    source: ManaSource::Summon(Position::Main),
                },
                PendingInput::Promotion { player: answerer },
                PendingInput::PrizePick { chooser: answerer },
            ] {
                let mut state = base_state();
                state.stack.push(StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                });
                state.stack_segment_bases.push(0);
                state.work.push_back(WorkItem::BeginMainPhase);
                state.pending = Some(pending);
                state.turn.window = Some(StackWindow {
                    holder: answerer,
                    prior_pass: true,
                });
                state.turn.normal_attack_used = true;
                state.turn.normal_retreat_used = true;
                state.players.get_mut(PlayerId::One).mana = ManaBank {
                    matter: 3,
                    mind: 1,
                    spirit: 2,
                };
                state.players.get_mut(PlayerId::Two).main_losses = 1;
                assert_resignation(&state, player);
            }
        }
    }
}

#[test]
fn ended_and_broken_games_reject_either_players_resignation() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (status, error) in [
            (
                GameStatus::Ended(GameOutcome {
                    winner: PlayerId::One,
                    reason: LossReason::EmptyDeckDraw,
                }),
                ActionError::GameAlreadyOver,
            ),
            (
                GameStatus::Broken(Breakage {
                    rule: "destruction",
                    entity: fixtures::id("quarry-whelp"),
                    expected: ComponentKind::Life,
                }),
                ActionError::GameBroken,
            ),
        ] {
            let mut state = base_state();
            state.status = status;
            let before = state.clone();
            assert_eq!(apply(&state, &resign(player)), Err(error));
            assert_eq!(state, before);
        }
    }
}
