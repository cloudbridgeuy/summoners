//! The `replay` command: replay a transcript's recorded actions through a
//! fresh recording, then compare the observed result to the transcript it
//! replayed.

use std::{
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::Path,
};

use summoners_cards::CardLibrary;
use summoners_core::domain::{actions::GameAction, errors::ActionError, state::GameState};
use summoners_core::engine::apply::{ActionOutcome, apply};
use summoners_match_log::compare::{
    ComparisonOptions, TranscriptComparison, compare_parsed_transcripts,
};
use summoners_match_log::replay::{ReplayError, prepare_scenario};
use summoners_match_log::{
    PreparedAction, PreparedScenario, RecordedMatch, RecordingError, RecordingStopped, TranscriptV1,
};

use crate::error::{GameFailure, ShellError};
use crate::output::{OutputPlan, finish, plan_output, refuse_same_file};
use crate::session::{SessionStatus, classify};

const COMMAND: &str = "replay";

/// The step and event counts a completed, matching replay confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaySummary {
    pub steps: u64,
    pub events: u64,
}

/// Whether the loop that submits every recorded action stopped because the
/// game reached a terminal state, or ran out of actions while still
/// playing.
enum LoopOutcome {
    Ended,
    Playing { step: u64 },
}

/// Replay one transcript's recorded actions through the current engine
/// into a fresh recording, then compare the observed result against the
/// transcript it replayed.
///
/// Writes to `<output>.partial` while the match is active, renaming it to
/// `output` only once recording reaches a complete `match_completed`
/// record; a `.partial` file is never presented as a valid transcript.
pub fn run_replay(
    from: &Path,
    output: &Path,
    force: bool,
    library: &CardLibrary,
) -> Result<ReplaySummary, ShellError> {
    run_replay_with(from, output, force, library, apply)
}

/// The same replay `run_replay` performs, but driven by a caller-supplied
/// transition instead of the engine's own `apply`.
///
/// This exists so a test can reproduce one deliberate engine-output
/// difference — the only way to reach the "the observed transcript
/// diverges from the expected one" outcome without hand-writing a
/// transcript that merely slips past the strict parser.
fn run_replay_with(
    from: &Path,
    output: &Path,
    force: bool,
    library: &CardLibrary,
    transition: impl Fn(&GameState, &GameAction) -> Result<ActionOutcome, ActionError>,
) -> Result<ReplaySummary, ShellError> {
    refuse_same_file(COMMAND, from, output)?;
    let transcript = parse_transcript(from)?;
    let scenario =
        prepare_scenario(&transcript, library).map_err(|source| ShellError::Transcript {
            command: COMMAND,
            path: from.to_path_buf(),
            source: Box::new(source),
        })?;

    let exists = output.exists();
    let plan = plan_output(COMMAND, output, force, exists)?;

    let mut recording = start_recording(&plan.partial, &scenario)?;
    let loop_result = record_scenario(&mut recording, &scenario.actions, transition);

    match loop_result {
        Ok(LoopOutcome::Ended) => {
            drop(recording.into_writer());
            complete_replay(from, &transcript, &plan)
        }
        Ok(LoopOutcome::Playing { step }) => Err(ShellError::Game {
            command: COMMAND,
            path: plan.partial,
            reason: GameFailure::ActionsExhausted { step },
        }),
        Err(RecordingError::Stopped(RecordingStopped::GameBroken)) => Err(ShellError::Game {
            command: COMMAND,
            path: plan.partial,
            reason: GameFailure::Broken,
        }),
        Err(source) => Err(ShellError::Recording {
            command: COMMAND,
            path: plan.partial,
            source,
        }),
    }
}

/// Rename the completed partial recording into place, then compare it
/// against the transcript it replayed.
fn complete_replay(
    from: &Path,
    transcript: &TranscriptV1,
    plan: &OutputPlan,
) -> Result<ReplaySummary, ShellError> {
    finish(COMMAND, plan)?;
    let observed = parse_transcript(&plan.output)?;

    match compare_parsed_transcripts(transcript, &observed, ComparisonOptions::default()).map_err(
        |source| ShellError::Comparison {
            command: COMMAND,
            path: plan.output.clone(),
            source,
        },
    )? {
        TranscriptComparison::Equal => Ok(ReplaySummary {
            steps: transcript.match_completed.step_count,
            events: transcript.match_completed.event_count,
        }),
        TranscriptComparison::Different(difference) => Err(ShellError::Difference {
            command: COMMAND,
            expected_path: from.to_path_buf(),
            observed_path: plan.output.clone(),
            difference: Box::new(difference),
        }),
    }
}

/// Submit every prepared action to `recording` in order, continuing past a
/// recorded rejection — a rejection is a normal outcome, not a failure —
/// and stopping as soon as the game reaches a terminal state.
fn record_scenario<W: Write>(
    recording: &mut RecordedMatch<W>,
    actions: &[PreparedAction],
    transition: impl Fn(&GameState, &GameAction) -> Result<ActionOutcome, ActionError>,
) -> Result<LoopOutcome, RecordingError> {
    let mut last_step = 0;
    for prepared in actions {
        last_step = prepared.step;
        recording.submit_with(&prepared.action, |state, action| transition(state, action))?;
        if let SessionStatus::Ended = classify(recording.state()) {
            return Ok(LoopOutcome::Ended);
        }
    }
    Ok(LoopOutcome::Playing { step: last_step })
}

fn start_recording(
    partial: &Path,
    scenario: &PreparedScenario,
) -> Result<RecordedMatch<BufWriter<File>>, ShellError> {
    let file = File::create(partial).map_err(|source| ShellError::Io {
        command: COMMAND,
        path: partial.to_path_buf(),
        source,
    })?;
    let writer = BufWriter::new(file);
    RecordedMatch::start(
        writer,
        scenario.metadata.clone(),
        scenario.required_sets.clone(),
        scenario.initial_state.clone(),
    )
    .map_err(|source| ShellError::Recording {
        command: COMMAND,
        path: partial.to_path_buf(),
        source,
    })
}

fn parse_transcript(path: &Path) -> Result<TranscriptV1, ShellError> {
    let reader = open(path)?;
    TranscriptV1::parse(reader).map_err(|error| ShellError::Transcript {
        command: COMMAND,
        path: path.to_path_buf(),
        source: Box::new(ReplayError::Parse(error)),
    })
}

fn open(path: &Path) -> Result<BufReader<File>, ShellError> {
    let file = File::open(path).map_err(|source| ShellError::Io {
        command: COMMAND,
        path: path.to_path_buf(),
        source,
    })?;
    Ok(BufReader::new(file))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::{
        cell::Cell,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use summoners_cards::built_in_catalog;
    use summoners_core::domain::events::GameEvent;

    use super::*;

    fn golden(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../match-log/tests/goldens")
            .join(name)
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "summoners-shell-replay-unit-{label}-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("the temporary directory is created");
            Self { path }
        }

        fn join(&self, name: &str) -> PathBuf {
            self.path.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn a_deliberate_event_difference_keeps_the_observed_file_and_reports_it() {
        let catalog = built_in_catalog().expect("the built-in catalog loads");
        let directory = TempDir::new("difference");
        let output = directory.join("observed.ndjson");

        let corrupted = Cell::new(false);
        let transition =
            move |state: &GameState, action: &GameAction| -> Result<ActionOutcome, ActionError> {
                let mut outcome = apply(state, action)?;
                if !corrupted.get()
                    && let Some(event) = outcome
                        .events
                        .iter_mut()
                        .find(|event| matches!(event, GameEvent::PriorityPassed { .. }))
                {
                    if let GameEvent::PriorityPassed { player } = event {
                        *player = player.opponent();
                    }
                    corrupted.set(true);
                }
                Ok(outcome)
            };

        let error = run_replay_with(
            &golden("terminal_empty_deck.ndjson"),
            &output,
            false,
            catalog.library(),
            transition,
        )
        .expect_err("a corrupted event must be reported as a semantic difference");

        match &error {
            ShellError::Difference {
                expected_path,
                observed_path,
                difference,
                ..
            } => {
                assert_eq!(expected_path, &golden("terminal_empty_deck.ndjson"));
                assert_eq!(observed_path, &output);
                assert!(
                    difference.path.as_str().contains("player"),
                    "unexpected difference path: {}",
                    difference.path
                );
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(crate::exit::exit_code(&error), 3);
        assert!(output.exists(), "the complete observed file must be kept");
        assert!(
            !directory.join("observed.ndjson.partial").exists(),
            "the observed file must have been renamed into place"
        );
    }

    #[test]
    fn a_game_that_never_ends_keeps_the_partial_file_and_reports_actions_exhausted() {
        let catalog = built_in_catalog().expect("the built-in catalog loads");
        let directory = TempDir::new("exhausted");
        let output = directory.join("observed.ndjson");

        let never_ends =
            |state: &GameState, _action: &GameAction| -> Result<ActionOutcome, ActionError> {
                Ok(ActionOutcome {
                    state: state.clone(),
                    events: vec![],
                })
            };

        let error = run_replay_with(
            &golden("resignation.ndjson"),
            &output,
            false,
            catalog.library(),
            never_ends,
        )
        .expect_err("a game that never ends must be reported as a game failure");

        match &error {
            ShellError::Game { reason, path, .. } => {
                assert!(matches!(reason, GameFailure::ActionsExhausted { .. }));
                assert_eq!(path, &directory.join("observed.ndjson.partial"));
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(crate::exit::exit_code(&error), 5);
        assert!(
            directory.join("observed.ndjson.partial").exists(),
            "the partial file must be kept for diagnosis"
        );
        assert!(!output.exists(), "no complete output file must be created");
    }

    #[test]
    fn same_output_and_from_is_refused_before_any_file_is_touched() {
        let catalog = built_in_catalog().expect("the built-in catalog loads");
        let directory = TempDir::new("same-file");
        let input = directory.join("in.ndjson");
        fs::copy(golden("resignation.ndjson"), &input).expect("the golden copies into the sandbox");
        let original = fs::read(&input).expect("the copied input is readable");

        let error = run_replay(&input, &input, true, catalog.library())
            .expect_err("replaying a transcript onto itself must be refused");

        assert!(
            matches!(error, ShellError::Usage(_)),
            "unexpected error: {error}"
        );
        assert_eq!(crate::exit::exit_code(&error), 2);
        let after = fs::read(&input).expect("the input is still readable");
        assert_eq!(original, after, "the input file must be left untouched");
        assert!(
            !directory.join("in.ndjson.partial").exists(),
            "no partial file must be created for a refused run"
        );
    }

    #[test]
    fn a_missing_input_still_reports_as_a_missing_file_even_when_output_matches() {
        let catalog = built_in_catalog().expect("the built-in catalog loads");
        let directory = TempDir::new("same-file-missing");
        let missing = directory.join("does-not-exist.ndjson");

        let error = run_replay(&missing, &missing, false, catalog.library())
            .expect_err("a missing input must still be reported as a missing file");

        assert!(
            matches!(error, ShellError::Io { .. }),
            "unexpected error: {error}"
        );
        assert_eq!(crate::exit::exit_code(&error), 4);
    }
}
