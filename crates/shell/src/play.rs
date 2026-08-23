//! The `play` command: an interactive session against a fresh recording.
//!
//! `play` reads only a scenario's header metadata, its required Set
//! revisions, and its initial state; the transcript's own recorded actions
//! are never read. In their place, an operator drives the match one typed
//! line at a time.

use std::io::{BufRead, Write};
use std::{fs::File, io::BufReader, io::BufWriter, path::Path};

use summoners_cards::CardLibrary;
use summoners_core::domain::actions::GameAction;
use summoners_match_log::replay::{ReplayError, prepare_scenario};
use summoners_match_log::{
    PreparedScenario, RecordedMatch, RecordedStep, RecordingError, RecordingStopped,
    StateProjectionV1, TranscriptV1,
};

use crate::error::{GameFailure, ShellError};
use crate::input::{GRAMMAR_HELP, ShellInput, parse_line};
use crate::output::{OutputPlan, finish, plan_output, refuse_same_file};
use crate::session::{SessionStatus, classify};
use crate::view::{compact_view, format_accepted_events, format_rejection};

const COMMAND: &str = "play";

/// The step and event counts a completed interactive session recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaySummary {
    pub steps: u64,
    pub events: u64,
}

/// Whether the session loop stopped because the game reached a terminal
/// outcome, the operator quit, the game broke, the recording itself could
/// no longer proceed, or the input stream itself failed to read.
enum LoopOutcome {
    Ended,
    Quit,
    Broken,
    RecordingFailed(RecordingError),
    ReadFailed(std::io::Error),
}

/// The three streams an interactive session reads and writes: operator
/// input, the compact view and echoed outcomes, and unparseable-line
/// diagnostics. Grouped into one value so the session can be driven by
/// real terminal streams, or, in tests, by in-memory buffers.
pub struct PlayStreams<R, W, E> {
    pub input: R,
    pub output: W,
    pub errors: E,
}

/// Run one interactive session from `from`'s header metadata, required Set
/// revisions, and initial state — never its recorded actions — into a
/// fresh recording at `output_path`.
///
/// Prints the compact view and a `> ` prompt before every read, echoes
/// every accepted action's events and every rejection to `streams.output`,
/// and reports an unparseable line on `streams.errors`; a rejection and an
/// unparseable line both leave the session running. Writes to
/// `<output_path>.partial` while the match is active, renaming it to
/// `output_path` only once the game reaches a terminal outcome. Ending the
/// session any other way — `quit`, end of input, the game breaking, or the
/// input stream itself failing to read — leaves the partial file in place
/// for diagnosis; a `.partial` file is never presented as a valid
/// transcript.
pub fn run_play<R: BufRead, W: Write, E: Write>(
    from: &Path,
    output_path: &Path,
    force: bool,
    library: &CardLibrary,
    mut streams: PlayStreams<R, W, E>,
) -> Result<PlaySummary, ShellError> {
    refuse_same_file(COMMAND, from, output_path)?;
    let transcript = parse_transcript(from)?;
    let scenario =
        prepare_scenario(&transcript, library).map_err(|source| ShellError::Transcript {
            command: COMMAND,
            path: from.to_path_buf(),
            source: Box::new(source),
        })?;

    let exists = output_path.exists();
    let plan = plan_output(COMMAND, output_path, force, exists)?;

    let mut recording = start_recording(&plan.partial, &scenario)?;
    let outcome = session_loop(
        &mut recording,
        &mut streams.input,
        &mut streams.output,
        &mut streams.errors,
    );
    drop(recording.into_writer());

    match outcome {
        LoopOutcome::Ended => complete_play(&plan),
        LoopOutcome::Quit => Err(ShellError::Game {
            command: COMMAND,
            path: plan.partial,
            reason: GameFailure::OperatorQuit,
        }),
        LoopOutcome::Broken => Err(ShellError::Game {
            command: COMMAND,
            path: plan.partial,
            reason: GameFailure::Broken,
        }),
        LoopOutcome::RecordingFailed(source) => Err(ShellError::Recording {
            command: COMMAND,
            path: plan.partial,
            source,
        }),
        LoopOutcome::ReadFailed(source) => Err(ShellError::Io {
            command: COMMAND,
            path: plan.partial,
            source,
        }),
    }
}

/// Rename the completed partial recording into place and read the step and
/// event counts a completed session confirmed.
fn complete_play(plan: &OutputPlan) -> Result<PlaySummary, ShellError> {
    finish(COMMAND, plan)?;
    let observed = parse_transcript(&plan.output)?;
    Ok(PlaySummary {
        steps: observed.match_completed.step_count,
        events: observed.match_completed.event_count,
    })
}

/// Render the view, prompt, read, parse, and act — until the game ends,
/// the operator quits, input reaches its end, recording itself fails, or
/// the input stream itself fails to read.
fn session_loop<W1: Write, R: BufRead, O: Write, E: Write>(
    recording: &mut RecordedMatch<W1>,
    input: &mut R,
    output: &mut O,
    errors: &mut E,
) -> LoopOutcome {
    loop {
        let _ = writeln!(output, "{}", compact_view(recording.state()));
        let _ = write!(output, "> ");
        let _ = output.flush();

        let mut line = String::new();
        match input.read_line(&mut line) {
            // End of input behaves exactly as `quit`.
            Ok(0) => return LoopOutcome::Quit,
            Ok(_) => {}
            Err(source) => return LoopOutcome::ReadFailed(source),
        }

        match parse_line(&line) {
            Ok(ShellInput::Quit) => return LoopOutcome::Quit,
            Ok(ShellInput::Help) => {
                let _ = writeln!(output, "{GRAMMAR_HELP}");
            }
            Ok(ShellInput::State { json: false }) => {
                let _ = writeln!(output, "{}", compact_view(recording.state()));
            }
            Ok(ShellInput::State { json: true }) => {
                let projection = StateProjectionV1::from_state(recording.state());
                if let Ok(json) = serde_json::to_string_pretty(&projection) {
                    let _ = writeln!(output, "{json}");
                }
            }
            Ok(ShellInput::Action(action)) => match GameAction::try_from(action) {
                Ok(action) => {
                    if let Some(stopped) = submit(recording, &action, output) {
                        return stopped;
                    }
                }
                Err(source) => {
                    let _ = writeln!(errors, "input: {source}");
                }
            },
            Err(source) => {
                let _ = writeln!(errors, "input: {source}");
            }
        }
    }
}

/// Submit one action and print its outcome. Returns `Some` when the
/// session must stop — the game ended, broke, or the recording failed —
/// and `None` when the session should keep running.
fn submit<W1: Write, O: Write>(
    recording: &mut RecordedMatch<W1>,
    action: &GameAction,
    output: &mut O,
) -> Option<LoopOutcome> {
    match recording.submit(action) {
        Ok(RecordedStep::Accepted { events }) => {
            for line in format_accepted_events(&events) {
                let _ = writeln!(output, "{line}");
            }
            match classify(recording.state()) {
                SessionStatus::Ended => Some(LoopOutcome::Ended),
                SessionStatus::Broken => Some(LoopOutcome::Broken),
                SessionStatus::Playing => None,
            }
        }
        Ok(RecordedStep::Rejected { error }) => {
            let _ = writeln!(output, "{}", format_rejection(error));
            None
        }
        Err(RecordingError::Stopped(RecordingStopped::GameBroken)) => Some(LoopOutcome::Broken),
        Err(source) => Some(LoopOutcome::RecordingFailed(source)),
    }
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
mod tests;
