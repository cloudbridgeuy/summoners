#![allow(clippy::expect_used)]

use summoners_cards::built_in_catalog;
use summoners_core::domain::{
    actions::GameAction,
    events::GameEvent,
    ids::{PlayerId, Position},
    state::{
        GameOutcome, GameStatus, LossReason, ManaSource, PendingInput, StackItem, StackWindow,
        WorkItem,
    },
};
use summoners_match_log::{
    HeaderMetadataV1, RecordedMatch, RecordedStep, RecordingError, RecordingStopped, StateDigestV1,
    StateProjectionV1, TranscriptV1, replay::verify_transcript,
};

mod support;

#[test]
fn resignation_records_and_replays_the_exact_stopped_state_for_every_pending_choice() {
    let catalog = built_in_catalog().expect("the embedded catalog loads");
    for player in [PlayerId::One, PlayerId::Two] {
        let answerer = player.opponent();
        for pending in [
            PendingInput::ManaProduction {
                player: answerer,
                source: ManaSource::Player,
            },
            PendingInput::Promotion { player: answerer },
            PendingInput::PrizePick { chooser: answerer },
        ] {
            let mut initial = support::initial_state();
            initial.turn.active_player = answerer;
            initial.turn.window = Some(StackWindow {
                holder: answerer,
                prior_pass: true,
            });
            initial.stack.push(StackItem::Attack {
                attacker: answerer,
                target: Position::Main,
            });
            initial.stack_segment_bases.push(0);
            initial.work.push_back(WorkItem::BeginMainPhase);
            initial.pending = Some(pending);
            let outcome = GameOutcome {
                winner: answerer,
                reason: LossReason::Resignation,
            };
            let mut expected = initial.clone();
            expected.status = GameStatus::Ended(outcome);
            let mut bytes = Vec::new();
            {
                let mut recording =
                    RecordedMatch::start(&mut bytes, HeaderMetadataV1::new(), vec![], initial)
                        .expect("recording starts");
                assert_eq!(
                    recording
                        .submit(&GameAction::Resign { player })
                        .expect("resignation records"),
                    RecordedStep::Accepted {
                        events: vec![GameEvent::GameEnded {
                            winner: answerer,
                            reason: LossReason::Resignation,
                        }],
                    }
                );
                assert_eq!(recording.state(), &expected);
            }
            let completed_bytes = bytes.clone();
            // Repeat with a live handle to prove subsequent submissions append nothing.
            let mut second_bytes = Vec::new();
            {
                let parsed =
                    TranscriptV1::parse(completed_bytes.as_slice()).expect("complete transcript");
                let initial = parsed
                    .match_created
                    .initial_state
                    .into_game_state(catalog.library().core_cards())
                    .expect("initial state rebuilds");
                let mut recording = RecordedMatch::start(
                    &mut second_bytes,
                    HeaderMetadataV1::new(),
                    vec![],
                    initial,
                )
                .expect("recording starts");
                recording
                    .submit(&GameAction::Resign { player })
                    .expect("resignation records");
                for action in [
                    GameAction::Resign { player: answerer },
                    GameAction::EndTurn { player: answerer },
                ] {
                    assert!(matches!(
                        recording.submit(&action),
                        Err(RecordingError::Stopped(RecordingStopped::MatchCompleted))
                    ));
                }
            }
            assert_eq!(second_bytes, completed_bytes);
            let parsed = TranscriptV1::parse(bytes.as_slice()).expect("complete transcript parses");
            let projection = StateProjectionV1::from_state(&expected);
            assert_eq!(parsed.final_state.final_state, projection);
            assert_eq!(
                parsed.final_state.state_digest,
                StateDigestV1::compute(&projection).expect("digest computes")
            );
            assert_eq!(parsed.match_completed.winner, outcome.winner.into());
            assert_eq!(parsed.match_completed.reason, outcome.reason.into());
            verify_transcript(bytes.as_slice(), catalog.library()).expect("replay agrees");
        }
    }
}
