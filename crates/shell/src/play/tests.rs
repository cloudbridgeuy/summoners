#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    io::{self, BufRead, Cursor, Read},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use summoners_cards::built_in_catalog;

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
            "summoners-shell-play-unit-{label}-{}-{sequence}",
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

/// A `BufRead` whose every read fails, standing in for a real stream
/// failure — undecodable bytes on a terminal, a closed pipe, and so on.
struct FailingInput;

impl Read for FailingInput {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::InvalidData, "stream failure"))
    }
}

impl BufRead for FailingInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Err(io::Error::new(io::ErrorKind::InvalidData, "stream failure"))
    }

    fn consume(&mut self, _amount: usize) {}
}

fn run(
    from: &Path,
    output: &Path,
    script: &str,
) -> (Result<PlaySummary, ShellError>, String, String) {
    let catalog = built_in_catalog().expect("the built-in catalog loads");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let result = run_play(
        from,
        output,
        false,
        catalog.library(),
        PlayStreams {
            input: Cursor::new(script.as_bytes()),
            output: &mut stdout,
            errors: &mut stderr,
        },
    );
    (
        result,
        String::from_utf8(stdout).expect("stdout is UTF-8"),
        String::from_utf8(stderr).expect("stderr is UTF-8"),
    )
}

#[test]
fn resign_completes_the_session_and_renames_the_partial_into_place() {
    let directory = TempDir::new("resign");
    let output = directory.join("played.ndjson");

    let (result, stdout, _stderr) = run(&golden("resignation.ndjson"), &output, "resign one\n");

    let summary = result.expect("a resignation must complete the session");
    assert_eq!(
        summary,
        PlaySummary {
            steps: 1,
            events: 1
        }
    );
    assert!(output.exists(), "the complete file must be written");
    assert!(
        !directory.join("played.ndjson.partial").exists(),
        "the partial file must have been renamed away"
    );
    assert!(
        stdout.contains("event 0: "),
        "the accepted event must be echoed: {stdout}"
    );
}

#[test]
fn quit_leaves_the_partial_file_and_reports_operator_quit() {
    let directory = TempDir::new("quit");
    let output = directory.join("played.ndjson");

    let (result, _stdout, _stderr) = run(&golden("resignation.ndjson"), &output, "quit\n");

    let error = result.expect_err("quitting must not complete the session");
    match &error {
        ShellError::Game { reason, path, .. } => {
            assert_eq!(*reason, GameFailure::OperatorQuit);
            assert_eq!(path, &directory.join("played.ndjson.partial"));
        }
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(crate::exit::exit_code(&error), 5);
    assert!(
        directory.join("played.ndjson.partial").exists(),
        "the partial file must be kept for diagnosis"
    );
    assert!(!output.exists(), "no complete file must be created");
}

#[test]
fn end_of_input_behaves_exactly_as_quit() {
    let directory = TempDir::new("eof");
    let output = directory.join("played.ndjson");

    let (result, _stdout, _stderr) = run(&golden("resignation.ndjson"), &output, "");

    let error = result.expect_err("reaching end of input must not complete the session");
    assert!(matches!(
        error,
        ShellError::Game {
            reason: GameFailure::OperatorQuit,
            ..
        }
    ));
    assert!(directory.join("played.ndjson.partial").exists());
}

#[test]
fn a_read_failure_is_its_own_outcome_and_the_partial_file_is_kept() {
    let directory = TempDir::new("read-failure");
    let output = directory.join("played.ndjson");
    let catalog = built_in_catalog().expect("the built-in catalog loads");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let result = run_play(
        &golden("resignation.ndjson"),
        &output,
        false,
        catalog.library(),
        PlayStreams {
            input: FailingInput,
            output: &mut stdout,
            errors: &mut stderr,
        },
    );

    let error = result.expect_err("a read failure must not complete the session");
    match &error {
        ShellError::Io { command, path, .. } => {
            assert_eq!(*command, COMMAND);
            assert_eq!(path, &directory.join("played.ndjson.partial"));
        }
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(crate::exit::exit_code(&error), 4);
    assert!(
        directory.join("played.ndjson.partial").exists(),
        "the partial file must be kept for diagnosis"
    );
    assert!(!output.exists(), "no complete file must be created");
}

#[test]
fn an_unparseable_line_is_reported_on_the_error_stream_and_the_session_continues() {
    let directory = TempDir::new("bogus");
    let output = directory.join("played.ndjson");

    let (result, _stdout, stderr) = run(
        &golden("resignation.ndjson"),
        &output,
        "bogus\nresign one\n",
    );

    let summary = result.expect("the session must continue past the unparseable line");
    assert_eq!(
        summary,
        PlaySummary {
            steps: 1,
            events: 1
        }
    );
    assert!(
        stderr.contains("input: "),
        "the bad line must be reported on stderr: {stderr}"
    );
}

#[test]
fn a_rejected_action_is_a_normal_outcome_and_the_session_continues() {
    let directory = TempDir::new("rejected");
    let output = directory.join("played.ndjson");

    // Player two may not resign on player one's turn — this must be
    // recorded as a rejection, not stop the session.
    let (result, stdout, _stderr) = run(
        &golden("terminal_empty_deck.ndjson"),
        &output,
        "end-turn two\nresign one\n",
    );

    let summary = result.expect("a rejection must not stop the session");
    assert_eq!(
        summary,
        PlaySummary {
            steps: 2,
            events: 1
        }
    );
    assert!(
        stdout.contains("rejected: "),
        "the rejection must be echoed: {stdout}"
    );
}

#[test]
fn state_and_state_json_print_without_ending_the_session() {
    let directory = TempDir::new("state");
    let output = directory.join("played.ndjson");

    let (result, stdout, _stderr) = run(
        &golden("resignation.ndjson"),
        &output,
        "state\nstate --json\nresign one\n",
    );

    let summary = result.expect("state commands must not end the session");
    assert_eq!(
        summary,
        PlaySummary {
            steps: 1,
            events: 1
        }
    );
    assert!(stdout.contains("status: playing"));
    assert!(
        stdout.contains("\"status\""),
        "state --json must print JSON: {stdout}"
    );
}

#[test]
fn help_prints_the_grammar_and_the_session_continues() {
    let directory = TempDir::new("help");
    let output = directory.join("played.ndjson");

    let (result, stdout, _stderr) =
        run(&golden("resignation.ndjson"), &output, "help\nresign one\n");

    let summary = result.expect("help must not end the session");
    assert_eq!(
        summary,
        PlaySummary {
            steps: 1,
            events: 1
        }
    );
    assert!(
        stdout.contains("play-summon"),
        "help must print the grammar: {stdout}"
    );
}

#[test]
fn same_output_and_from_is_refused_before_any_file_is_touched() {
    let directory = TempDir::new("same-file");
    let input = directory.join("in.ndjson");
    fs::copy(golden("resignation.ndjson"), &input).expect("the golden copies into the sandbox");

    let (result, _stdout, _stderr) = run(&input, &input, "quit\n");

    let error = result.expect_err("playing a transcript onto itself must be refused");
    assert!(
        matches!(error, ShellError::Usage(_)),
        "unexpected error: {error}"
    );
    assert_eq!(crate::exit::exit_code(&error), 2);
    assert!(
        !directory.join("in.ndjson.partial").exists(),
        "no partial file must be created for a refused run"
    );
}
