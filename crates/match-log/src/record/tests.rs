#![allow(clippy::expect_used)]

use std::{cell::RefCell, collections::VecDeque, io, rc::Rc, sync::Arc};

use serde_json::Value;
use summoners_core::domain::{
    cards::{Breakage, CardSet, ComponentKind, EntityId},
    ids::PlayerId,
    state::{
        GameOutcome, GameStatus, LossReason, ManaBank, PerPlayer, Phase, PlayerState, TurnState,
    },
};

use super::*;

fn state() -> GameState {
    let player = PlayerState {
        main: None,
        bench: [None, None, None],
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        enchantments: vec![],
    };
    GameState {
        players: PerPlayer::new(player.clone(), player),
        coin: None,
        turn: TurnState {
            active_player: PlayerId::One,
            phase: Phase::Main,
            window: None,
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: PerPlayer::new(false, false),
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: VecDeque::new(),
        pending: None,
        status: GameStatus::Playing,
        cards: Arc::new(CardSet::new(vec![])),
    }
}

fn action(player: PlayerId) -> GameAction {
    GameAction::EndTurn { player }
}

fn start(writer: SharedWriter) -> RecordedMatch<SharedWriter> {
    RecordedMatch::start(writer, HeaderMetadataV1::new(), vec![], state())
        .expect("the initial checkpoint is writable")
}

#[test]
fn terminal_initial_states_are_rejected_before_any_write() {
    let statuses = [
        (
            GameStatus::Ended(GameOutcome {
                winner: PlayerId::One,
                reason: LossReason::EmptyDeckDraw,
            }),
            RecordingStopped::GameAlreadyEnded,
        ),
        (
            GameStatus::Broken(Breakage {
                rule: "destruction",
                entity: EntityId::parse(&"bb".repeat(16)).expect("valid test entity ID"),
                expected: ComponentKind::Life,
            }),
            RecordingStopped::GameBroken,
        ),
    ];

    for (status, expected) in statuses {
        let writer = SharedWriter::default();
        let probe = writer.clone();
        let mut initial = state();
        initial.status = status;

        let result = RecordedMatch::start(writer, HeaderMetadataV1::new(), vec![], initial);

        assert!(matches!(result, Err(RecordingError::Stopped(reason)) if reason == expected));
        assert!(probe.bytes().is_empty());
    }
}

#[test]
fn initial_status_check_accepts_only_playing() {
    let playing = state();
    assert_eq!(check_initial_status(&playing), Ok(()));

    let mut ended = state();
    ended.status = GameStatus::Ended(GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::EmptyDeckDraw,
    });
    assert_eq!(
        check_initial_status(&ended),
        Err(RecordingStopped::GameAlreadyEnded)
    );

    let mut broken = state();
    broken.status = GameStatus::Broken(Breakage {
        rule: "destruction",
        entity: EntityId::parse(&"cc".repeat(16)).expect("valid test entity ID"),
        expected: ComponentKind::Life,
    });
    assert_eq!(
        check_initial_status(&broken),
        Err(RecordingStopped::GameBroken)
    );
}

#[test]
fn terminal_event_check_rejects_all_inconsistent_shapes() {
    let expected = GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::EmptyDeckDraw,
    };
    let matching = GameEvent::GameEnded {
        winner: expected.winner,
        reason: expected.reason,
    };
    let mismatched = GameEvent::GameEnded {
        winner: PlayerId::Two,
        reason: LossReason::ThirdMainLoss,
    };

    assert_eq!(check_terminal_events(GameStatus::Playing, &[]), Ok(None));
    assert_eq!(
        check_terminal_events(GameStatus::Playing, std::slice::from_ref(&matching)),
        Err(TerminalEventError::UnexpectedGameEnded)
    );
    assert_eq!(
        check_terminal_events(GameStatus::Ended(expected), &[]),
        Err(TerminalEventError::MissingGameEnded)
    );
    assert_eq!(
        check_terminal_events(
            GameStatus::Ended(expected),
            &[matching.clone(), matching.clone()]
        ),
        Err(TerminalEventError::MultipleGameEnded)
    );
    assert_eq!(
        check_terminal_events(GameStatus::Ended(expected), &[mismatched]),
        Err(TerminalEventError::OutcomeMismatch)
    );
    assert_eq!(
        check_terminal_events(GameStatus::Ended(expected), &[matching]),
        Ok(Some(expected))
    );
}

fn json_lines(writer: &SharedWriter) -> Vec<Value> {
    let bytes = writer.bytes();
    std::str::from_utf8(&bytes)
        .expect("records are UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("record is JSON"))
        .collect()
}

fn records_named<'a>(lines: &'a [Value], name: &str) -> Vec<&'a Value> {
    lines.iter().filter(|line| line["record"] == name).collect()
}

#[test]
fn prepare_start_builds_header_and_match_created_records() {
    let records = prepare_start(
        HeaderMetadataV1::new(),
        vec![SetRequirementV1 {
            set: "foundations".to_string(),
            revision: 1,
        }],
        &state(),
    )
    .expect("the state is serializable");

    assert!(matches!(records.0, RecordV1::Header(_)));
    assert!(matches!(records.1, RecordV1::MatchCreated(_)));
}

#[test]
fn pure_record_preparation_preserves_sequences_indexes_and_counts() {
    let action = action(PlayerId::One);
    let digest = StateDigestV1("sha256:test".to_string());
    let events = vec![
        GameEvent::PriorityPassed {
            player: PlayerId::One,
        },
        GameEvent::TurnBegan {
            player: PlayerId::Two,
        },
    ];

    assert!(matches!(
        prepare_action(2, 1, &action),
        RecordV1::Action(ActionRecordV1 {
            sequence: 2,
            step: 1,
            ..
        })
    ));
    let records = prepare_accepted_step(3, 1, &events, digest);
    assert!(matches!(
        &records[0],
        RecordV1::Event(EventRecordV1 {
            sequence: 3,
            step: 1,
            index: 0,
            ..
        })
    ));
    assert!(matches!(
        &records[1],
        RecordV1::Event(EventRecordV1 {
            sequence: 4,
            step: 1,
            index: 1,
            ..
        })
    ));
    assert!(matches!(
        &records[2],
        RecordV1::StepCompleted(StepCompletedV1 {
            sequence: 5,
            step: 1,
            event_count: 2,
            ..
        })
    ));
}

#[test]
fn pure_zero_event_preparation_has_a_complete_terminal_record() {
    let records = prepare_accepted_step(3, 1, &[], StateDigestV1("sha256:test".to_string()));

    assert_eq!(records.len(), 1);
    assert!(matches!(
        &records[0],
        RecordV1::StepCompleted(StepCompletedV1 {
            sequence: 3,
            step: 1,
            event_count: 0,
            ..
        })
    ));
}

#[test]
fn rejected_record_preparation_keeps_the_typed_error_and_digest() {
    let digest = StateDigestV1("sha256:unchanged".to_string());
    let record = prepare_rejected_step(3, 1, ActionError::WrongPhase, digest.clone());

    assert_eq!(
        record,
        RecordV1::StepRejected(StepRejectedV1 {
            sequence: 3,
            record: StepRejectedRecordKindV1::StepRejected,
            step: 1,
            error: ErrorV1::WrongPhase,
            state_digest: digest,
        })
    );
}

#[test]
fn completion_preparation_agrees_on_outcome_counts_and_digest() {
    let projection = StateProjectionV1::from_state(&state());
    let digest = StateDigestV1("sha256:final".to_string());
    let outcome = GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::EmptyDeckDraw,
    };

    let records = prepare_completion(
        CompletionCounts {
            sequence: 10,
            steps: 4,
            events: 5,
        },
        outcome,
        projection.clone(),
        digest.clone(),
    );

    assert_eq!(
        records[0],
        RecordV1::FinalState(Box::new(FinalStateV1 {
            sequence: 10,
            record: FinalStateRecordKindV1::FinalState,
            final_state: projection,
            state_digest: digest.clone(),
        }))
    );
    assert_eq!(
        records[1],
        RecordV1::MatchCompleted(MatchCompletedV1 {
            sequence: 11,
            record: MatchCompletedRecordKindV1::MatchCompleted,
            step_count: 4,
            event_count: 5,
            state_digest: digest,
            winner: PlayerId::One.into(),
            reason: LossReason::EmptyDeckDraw.into(),
        })
    );
}

#[test]
fn action_is_written_before_apply_and_zero_events_are_complete() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let mut recording = start(writer);

    let result = recording
        .submit_with(&action(PlayerId::One), |_state, _action| {
            let lines = json_lines(&probe);
            assert_eq!(records_named(&lines, "action").len(), 1);
            Ok(ActionOutcome {
                state: state(),
                events: vec![],
            })
        })
        .expect("the action is accepted");

    assert_eq!(result, RecordedStep::Accepted { events: vec![] });
    let lines = json_lines(&probe);
    let completed = records_named(&lines, "step_completed");
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0]["event_count"], 0);
}

#[test]
fn accepted_multi_event_step_writes_exact_index_and_order() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let mut recording = start(writer);
    let events = vec![
        GameEvent::PriorityPassed {
            player: PlayerId::Two,
        },
        GameEvent::TurnBegan {
            player: PlayerId::One,
        },
    ];

    let result = recording
        .submit_with(&action(PlayerId::One), |_state, _action| {
            Ok(ActionOutcome {
                state: state(),
                events: events.clone(),
            })
        })
        .expect("the action is accepted");

    assert_eq!(
        result,
        RecordedStep::Accepted {
            events: events.clone()
        }
    );
    let lines = json_lines(&probe);
    let recorded = records_named(&lines, "event");
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0]["index"], 0);
    assert_eq!(recorded[0]["event"]["kind"], "priority_passed");
    assert_eq!(recorded[1]["index"], 1);
    assert_eq!(recorded[1]["event"]["kind"], "turn_began");
}

#[test]
fn rejected_action_returns_typed_error_and_unchanged_digest() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let initial = state();
    let expected_digest = StateDigestV1::compute(&StateProjectionV1::from_state(&initial))
        .expect("the state is serializable");
    let mut recording =
        RecordedMatch::start(writer, HeaderMetadataV1::new(), vec![], initial.clone())
            .expect("the initial checkpoint is writable");

    let result = recording
        .submit(&action(PlayerId::Two))
        .expect("a rejection is a recorded result");

    assert_eq!(
        result,
        RecordedStep::Rejected {
            error: ActionError::NotYourDecision
        }
    );
    assert_eq!(recording.state(), &initial);
    let lines = json_lines(&probe);
    let rejected = records_named(&lines, "step_rejected");
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0]["error"]["kind"], "not_your_decision");
    assert_eq!(rejected[0]["state_digest"], expected_digest.0);
    assert!(records_named(&lines, "event").is_empty());
}

#[test]
fn short_match_transcript_has_one_consistent_terminal_outcome() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let mut recording = start(writer);

    assert_eq!(
        recording
            .submit(&action(PlayerId::One))
            .expect("ending Main opens Priority"),
        RecordedStep::Accepted { events: vec![] }
    );
    assert_eq!(
        recording
            .submit(&action(PlayerId::Two))
            .expect("the illegal action is recorded"),
        RecordedStep::Rejected {
            error: ActionError::WrongPhase
        }
    );
    recording
        .submit(&GameAction::PassPriority {
            player: PlayerId::Two,
        })
        .expect("the defender passes");
    let terminal = recording
        .submit(&GameAction::PassPriority {
            player: PlayerId::One,
        })
        .expect("the active player passes and the empty draw ends the match");

    let RecordedStep::Accepted { events } = terminal else {
        panic!("the terminal action must be accepted");
    };
    assert!(matches!(
        events.last(),
        Some(GameEvent::GameEnded {
            winner: PlayerId::One,
            reason: LossReason::EmptyDeckDraw
        })
    ));
    assert_eq!(
        recording.state().status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::EmptyDeckDraw,
        })
    );

    let lines = json_lines(&probe);
    for (sequence, line) in lines.iter().enumerate() {
        assert_eq!(line["sequence"], sequence as u64);
    }
    let ended = lines
        .iter()
        .filter(|line| line["event"]["kind"] == "game_ended")
        .collect::<Vec<_>>();
    let final_state = records_named(&lines, "final_state");
    let completed = records_named(&lines, "match_completed");
    assert_eq!(ended.len(), 1);
    assert_eq!(final_state.len(), 1);
    assert_eq!(completed.len(), 1);
    assert_eq!(ended[0]["event"]["winner"], "one");
    assert_eq!(ended[0]["event"]["reason"], "empty_deck_draw");
    assert_eq!(
        final_state[0]["final_state"]["status"]["outcome"]["winner"],
        "one"
    );
    assert_eq!(
        final_state[0]["final_state"]["status"]["outcome"]["reason"],
        "empty_deck_draw"
    );
    assert_eq!(completed[0]["winner"], "one");
    assert_eq!(completed[0]["reason"], "empty_deck_draw");
    assert_eq!(completed[0]["step_count"], 4);
    assert_eq!(completed[0]["event_count"], 5);
    assert_eq!(completed[0]["state_digest"], final_state[0]["state_digest"]);
    let expected_projection = StateProjectionV1::from_state(recording.state());
    let expected_digest =
        StateDigestV1::compute(&expected_projection).expect("the final state is serializable");
    assert_eq!(
        final_state[0]["final_state"],
        serde_json::to_value(expected_projection).expect("the projection is JSON")
    );
    assert_eq!(final_state[0]["state_digest"], expected_digest.0);
}

#[test]
fn submit_after_completion_writes_nothing_and_does_not_call_apply() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let mut recording = start(writer);
    let mut ended = state();
    ended.status = GameStatus::Ended(GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::EmptyDeckDraw,
    });
    recording
        .submit_with(&action(PlayerId::One), |_state, _action| {
            Ok(ActionOutcome {
                state: ended,
                events: vec![GameEvent::GameEnded {
                    winner: PlayerId::One,
                    reason: LossReason::EmptyDeckDraw,
                }],
            })
        })
        .expect("the terminal result is recorded");
    let before = probe.bytes();
    let mut calls = 0;

    let result = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        calls += 1;
        unreachable!("apply must not run after completion")
    });

    assert!(matches!(
        result,
        Err(RecordingError::Stopped(RecordingStopped::MatchCompleted))
    ));
    assert_eq!(calls, 0);
    assert_eq!(probe.bytes(), before);
}

#[test]
fn broken_result_stops_without_false_completion() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let mut recording = start(writer);
    let mut broken = state();
    broken.status = GameStatus::Broken(Breakage {
        rule: "destruction",
        entity: EntityId::parse(&"aa".repeat(16)).expect("valid test entity ID"),
        expected: ComponentKind::Life,
    });

    let result = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Ok(ActionOutcome {
            state: broken,
            events: vec![],
        })
    });

    assert!(matches!(
        result,
        Err(RecordingError::Stopped(RecordingStopped::GameBroken))
    ));
    let lines = json_lines(&probe);
    assert_eq!(records_named(&lines, "step_completed").len(), 1);
    assert!(records_named(&lines, "final_state").is_empty());
    assert!(records_named(&lines, "match_completed").is_empty());
    let before = probe.bytes();
    let mut calls = 0;
    let second = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        calls += 1;
        unreachable!("apply must not run after breakage")
    });
    assert!(matches!(
        second,
        Err(RecordingError::Stopped(RecordingStopped::GameBroken))
    ));
    assert_eq!(calls, 0);
    assert_eq!(probe.bytes(), before);
}

#[test]
fn inconsistent_terminal_events_stop_without_false_completion() {
    let writer = SharedWriter::default();
    let probe = writer.clone();
    let mut recording = start(writer);
    let mut ended = state();
    ended.status = GameStatus::Ended(GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::EmptyDeckDraw,
    });

    let result = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Ok(ActionOutcome {
            state: ended,
            events: vec![],
        })
    });

    assert!(matches!(
        result,
        Err(RecordingError::Stopped(
            RecordingStopped::InvalidTerminalEvents(TerminalEventError::MissingGameEnded)
        ))
    ));
    let lines = json_lines(&probe);
    assert!(records_named(&lines, "final_state").is_empty());
    assert!(records_named(&lines, "match_completed").is_empty());
    let before = probe.bytes();
    let mut calls = 0;
    let second = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        calls += 1;
        unreachable!("apply must not run after a terminal event failure")
    });
    assert!(matches!(
        second,
        Err(RecordingError::Stopped(
            RecordingStopped::InvalidTerminalEvents(TerminalEventError::MissingGameEnded)
        ))
    ));
    assert_eq!(calls, 0);
    assert_eq!(probe.bytes(), before);
}

#[test]
fn action_write_failure_stops_before_apply() {
    let writer = SharedWriter::default();
    let control = writer.clone();
    let mut recording = start(writer);
    control.fail_record("action");
    let mut calls = 0;

    let first = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        calls += 1;
        unreachable!("apply must not run after an action write failure")
    });
    assert!(matches!(first, Err(RecordingError::Write(_))));
    assert_eq!(calls, 0);
    control.clear_failures();
    let before = control.bytes();
    let second = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        calls += 1;
        unreachable!("apply must not run after stop")
    });
    assert!(matches!(
        second,
        Err(RecordingError::Stopped(RecordingStopped::RecordingFailed))
    ));
    assert_eq!(calls, 0);
    assert_eq!(control.bytes(), before);
}

#[test]
fn event_write_failure_stops_before_another_action() {
    let writer = SharedWriter::default();
    let control = writer.clone();
    let mut recording = start(writer);
    control.fail_record("event");

    let first = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Ok(ActionOutcome {
            state: state(),
            events: vec![GameEvent::TurnBegan {
                player: PlayerId::One,
            }],
        })
    });
    assert!(matches!(first, Err(RecordingError::Write(_))));
    assert_stopped_without_apply(&mut recording, &control);
}

#[test]
fn result_write_failure_stops_before_another_action() {
    let writer = SharedWriter::default();
    let control = writer.clone();
    let mut recording = start(writer);
    control.fail_record("step_completed");

    let first = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Ok(ActionOutcome {
            state: state(),
            events: vec![],
        })
    });
    assert!(matches!(first, Err(RecordingError::Write(_))));
    assert_stopped_without_apply(&mut recording, &control);
}

#[test]
fn rejected_result_write_failure_stops_before_another_action() {
    let writer = SharedWriter::default();
    let control = writer.clone();
    let mut recording = start(writer);
    control.fail_record("step_rejected");

    let first = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Err(ActionError::WrongPhase)
    });
    assert!(matches!(first, Err(RecordingError::Write(_))));
    assert_stopped_without_apply(&mut recording, &control);
}

#[test]
fn accepted_checkpoint_flush_failure_stops_before_another_action() {
    let writer = SharedWriter::default();
    let control = writer.clone();
    let mut recording = start(writer);
    control.fail_flush(2);

    let first = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Ok(ActionOutcome {
            state: state(),
            events: vec![],
        })
    });
    assert!(matches!(first, Err(RecordingError::Flush(_))));
    assert_stopped_without_apply(&mut recording, &control);
}

#[test]
fn final_checkpoint_flush_failure_stops_before_another_action() {
    let writer = SharedWriter::default();
    let control = writer.clone();
    let mut recording = start(writer);
    control.fail_flush(3);
    let mut ended = state();
    ended.status = GameStatus::Ended(GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::EmptyDeckDraw,
    });

    let first = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        Ok(ActionOutcome {
            state: ended,
            events: vec![GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: LossReason::EmptyDeckDraw,
            }],
        })
    });
    assert!(matches!(first, Err(RecordingError::Flush(_))));
    let lines = json_lines(&control);
    assert_eq!(records_named(&lines, "final_state").len(), 1);
    assert_eq!(records_named(&lines, "match_completed").len(), 1);
    assert_stopped_without_apply(&mut recording, &control);
}

fn assert_stopped_without_apply(
    recording: &mut RecordedMatch<SharedWriter>,
    control: &SharedWriter,
) {
    control.clear_failures();
    let before = control.bytes();
    let mut calls = 0;
    let result = recording.submit_with(&action(PlayerId::One), |_state, _action| {
        calls += 1;
        unreachable!("apply must not run after stop")
    });
    assert!(matches!(
        result,
        Err(RecordingError::Stopped(RecordingStopped::RecordingFailed))
    ));
    assert_eq!(calls, 0);
    assert_eq!(control.bytes(), before);
}

#[test]
fn write_record_maps_a_sink_failure_to_the_write_variant() {
    let mut writer = AlwaysFails;
    let error = write_record(&mut writer, b"record").expect_err("the write fails");
    assert!(matches!(error, RecordingError::Write(_)));
}

#[test]
fn flush_checkpoint_maps_a_sink_failure_to_the_flush_variant() {
    let mut writer = FlushFails;
    let error = flush_checkpoint(&mut writer).expect_err("the flush fails");
    assert!(matches!(error, RecordingError::Flush(_)));
}

#[derive(Clone, Default)]
struct SharedWriter(Rc<RefCell<WriterState>>);

#[derive(Default)]
struct WriterState {
    bytes: Vec<u8>,
    flush_count: usize,
    fail_record: Option<String>,
    fail_flush: Option<usize>,
}

impl SharedWriter {
    fn bytes(&self) -> Vec<u8> {
        self.0.borrow().bytes.clone()
    }

    fn fail_record(&self, record: &str) {
        self.0.borrow_mut().fail_record = Some(record.to_string());
    }

    fn fail_flush(&self, flush: usize) {
        self.0.borrow_mut().fail_flush = Some(flush);
    }

    fn clear_failures(&self) {
        let mut state = self.0.borrow_mut();
        state.fail_record = None;
        state.fail_flush = None;
    }
}

impl io::Write for SharedWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut state = self.0.borrow_mut();
        let fails = state.fail_record.as_ref().is_some_and(|record| {
            serde_json::from_slice::<Value>(buffer)
                .ok()
                .is_some_and(|value| value["record"] == record.as_str())
        });
        if fails {
            return Err(io::Error::other("injected record write failure"));
        }
        state.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.flush_count += 1;
        if state.fail_flush == Some(state.flush_count) {
            return Err(io::Error::other("injected flush failure"));
        }
        Ok(())
    }
}

struct AlwaysFails;

impl io::Write for AlwaysFails {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FlushFails;

impl io::Write for FlushFails {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("injected flush failure"))
    }
}
