//! Resignation, driven end to end through `apply`. Resignation sits after
//! the status gate and before the actor gate (`check_actor`), so every case
//! here proves the bypass directly: a resignation from someone who is not
//! the active player, not the window holder, and not the named pending
//! answerer must all still succeed, must change nothing in the resulting
//! state but `status`, and must emit exactly the one `GameEnded` fact.
//! Shares `tests.rs`'s fixtures through `super::*`, like `board_economy.rs`
//! and its siblings.

use super::*;
use crate::domain::cards::{Breakage, ComponentKind};

fn resign(player: PlayerId) -> GameAction {
    GameAction::Resign { player }
}

fn expect_resignation_win(outcome: &ActionOutcome, winner: PlayerId) {
    assert_eq!(
        outcome.state.status,
        GameStatus::Ended(GameOutcome {
            winner,
            reason: LossReason::Resignation,
        })
    );
    assert_eq!(
        outcome.events,
        vec![GameEvent::GameEnded {
            winner,
            reason: LossReason::Resignation,
        }]
    );
}

#[test]
fn a_resignation_from_the_non_active_player_still_succeeds() {
    // `base_state()` rests with `PlayerId::One` active and no window or
    // pending decision open, so `PlayerId::Two` is not the required actor
    // — the ordinary actor gate would reject anything else `Two` submits.
    let state = base_state();

    let outcome = apply(&state, &resign(PlayerId::Two)).expect("resignation is always legal");

    expect_resignation_win(&outcome, PlayerId::One);
}

#[test]
fn a_resignation_from_outside_an_open_priority_window_still_succeeds() {
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });

    // `PlayerId::Two` does not hold the open window; the ordinary actor
    // gate would reject anything else `Two` submits here.
    let outcome = apply(&state, &resign(PlayerId::Two)).expect("resignation is always legal");

    expect_resignation_win(&outcome, PlayerId::One);
}

#[test]
fn a_resignation_from_outside_a_pending_decision_still_succeeds() {
    let mut state = base_state();
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });

    // `PlayerId::Two` is not the player a pending decision names; the
    // ordinary actor gate would reject anything else `Two` submits here.
    let outcome = apply(&state, &resign(PlayerId::Two)).expect("resignation is always legal");

    expect_resignation_win(&outcome, PlayerId::One);
}

#[test]
fn resignation_emits_exactly_one_event() {
    let state = base_state();

    let outcome = apply(&state, &resign(PlayerId::One)).expect("resignation is always legal");

    assert_eq!(outcome.events.len(), 1);
}

#[test]
fn resignation_changes_only_status_and_leaves_the_rest_of_the_state_untouched() {
    let mut state = base_state();
    state.stack.push(StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Main,
    });
    state.stack_segment_bases.push(0);
    state
        .work
        .push_back(crate::domain::state::WorkItem::BeginMainPhase);
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::Two,
    });
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
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

    let expected = GameState {
        status: GameStatus::Ended(GameOutcome {
            winner: PlayerId::Two,
            reason: LossReason::Resignation,
        }),
        ..state.clone()
    };

    let outcome = apply(&state, &resign(PlayerId::One)).expect("resignation is always legal");

    assert_eq!(outcome.state, expected);
}

#[test]
fn a_resignation_submitted_while_the_game_is_broken_is_rejected() {
    // Proves the `Resign` arm sits after the status gate, not in front of
    // it: a broken game must reject a resignation exactly like it rejects
    // every other action.
    let mut state = base_state();
    state.status = GameStatus::Broken(Breakage {
        rule: "destruction",
        entity: fixtures::id("quarry-whelp"),
        expected: ComponentKind::Life,
    });

    let result = apply(&state, &resign(PlayerId::One));

    assert_eq!(result, Err(ActionError::GameBroken));
}

#[test]
fn an_action_submitted_after_a_resignation_is_rejected_as_game_already_over() {
    let state = base_state();
    let resigned = apply(&state, &resign(PlayerId::Two)).expect("resignation is always legal");

    let result = apply(&resigned.state, &end_turn(PlayerId::One));

    assert_eq!(result, Err(ActionError::GameAlreadyOver));
}

#[test]
fn a_second_resignation_after_the_match_ended_is_also_rejected() {
    let state = base_state();
    let resigned = apply(&state, &resign(PlayerId::Two)).expect("resignation is always legal");

    let result = apply(&resigned.state, &resign(PlayerId::One));

    assert_eq!(result, Err(ActionError::GameAlreadyOver));
}
