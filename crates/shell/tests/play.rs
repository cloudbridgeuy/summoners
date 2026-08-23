//! Subprocess tests driving the real `summoners` binary's `play` command,
//! covering every session-lifecycle case the command line contract
//! promises: completion, quit, end of input, and a malformed line.

use std::{
    fs,
    io::Write,
    process::{Command, Output, Stdio},
};

use summoners_cards::built_in_catalog;
use summoners_match_log::StateProjectionV1;
use summoners_match_log::replay::prepare_scenario;

mod support;

use support::{TemporaryDirectory, golden};

fn run_play(from: &std::path::Path, output: &std::path::Path, script: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("play")
        .arg("--from")
        .arg(from)
        .arg("--output")
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the summoners binary runs");

    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        stdin
            .write_all(script.as_bytes())
            .expect("the script writes to stdin");
    }

    child
        .wait_with_output()
        .expect("the summoners binary finishes")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn resign_completes_the_session_and_renames_the_partial_into_place() {
    let directory = TemporaryDirectory::new("resign");
    let from = golden("resignation.ndjson");
    let played = directory.join("played.ndjson");

    let output = run_play(&from, &played, "resign one\n");

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(played.exists(), "the complete played file must exist");
    assert!(
        !directory.join("played.ndjson.partial").exists(),
        "no partial file must be left behind by a completed session"
    );

    let text = stdout(&output);
    assert!(
        text.contains("event 0: "),
        "the accepted event must be echoed: {text}"
    );

    let verify_output = Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("verify")
        .arg(&played)
        .output()
        .expect("the summoners binary runs");
    assert_eq!(
        verify_output.status.code(),
        Some(0),
        "the played file must be a valid transcript on its own: {}",
        String::from_utf8(verify_output.stderr).expect("stderr is UTF-8")
    );
}

#[test]
fn quit_leaves_the_partial_file_and_creates_no_complete_file() {
    let directory = TemporaryDirectory::new("quit");
    let from = golden("resignation.ndjson");
    let played = directory.join("played.ndjson");

    let output = run_play(&from, &played, "quit\n");

    assert_eq!(output.status.code(), Some(5), "stdout: {}", stdout(&output));
    assert!(!played.exists(), "no complete file must be created");
    assert!(
        directory.join("played.ndjson.partial").exists(),
        "the partial file must be kept for diagnosis"
    );
}

#[test]
fn end_of_input_behaves_exactly_as_quit() {
    let directory = TemporaryDirectory::new("eof");
    let from = golden("resignation.ndjson");
    let played = directory.join("played.ndjson");

    let output = run_play(&from, &played, "");

    assert_eq!(output.status.code(), Some(5), "stdout: {}", stdout(&output));
    assert!(!played.exists(), "no complete file must be created");
    assert!(
        directory.join("played.ndjson.partial").exists(),
        "the partial file must be kept for diagnosis"
    );
}

#[test]
fn an_unknown_verb_prints_an_input_error_and_the_session_continues() {
    let directory = TemporaryDirectory::new("bogus");
    let from = golden("resignation.ndjson");
    let played = directory.join("played.ndjson");

    let output = run_play(&from, &played, "bogus\nresign one\n");

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let message = stderr(&output);
    assert!(
        message.contains("input: "),
        "the bad line must be reported on stderr: {message}"
    );
    assert!(played.exists(), "the session must still complete");
}

#[test]
fn state_prints_the_compact_view() {
    let directory = TemporaryDirectory::new("state");
    let from = golden("resignation.ndjson");
    let played = directory.join("played.ndjson");

    let output = run_play(&from, &played, "state\nquit\n");

    let text = stdout(&output);
    assert!(text.contains("turn: "), "unexpected stdout: {text}");
    assert!(text.contains("decides: "), "unexpected stdout: {text}");
    assert!(
        text.contains("status: playing"),
        "unexpected stdout: {text}"
    );
}

#[test]
fn state_json_prints_the_recorder_state_projection() {
    let directory = TemporaryDirectory::new("state-json");
    let from = golden("resignation.ndjson");
    let played = directory.join("played.ndjson");

    let output = run_play(&from, &played, "state --json\nquit\n");
    let text = stdout(&output);

    let start = text.find('{').expect("the JSON block starts with a brace");
    let end = text.rfind('}').expect("the JSON block ends with a brace") + 1;
    let json = &text[start..end];
    let observed: StateProjectionV1 =
        serde_json::from_str(json).expect("the printed block is valid JSON");

    let catalog = built_in_catalog().expect("the built-in catalog loads");
    let transcript_bytes = fs::read(&from).expect("the golden reads");
    let transcript = summoners_match_log::TranscriptV1::parse(transcript_bytes.as_slice())
        .expect("the golden parses");
    let scenario =
        prepare_scenario(&transcript, catalog.library()).expect("the golden's scenario prepares");
    let expected = StateProjectionV1::from_state(&scenario.initial_state);

    assert_eq!(observed, expected);
}

#[test]
fn an_output_naming_the_same_file_as_from_is_refused_and_leaves_the_input_untouched() {
    let directory = TemporaryDirectory::new("same-file");
    let input = directory.join("in.ndjson");
    fs::copy(golden("resignation.ndjson"), &input).expect("the golden copies into the sandbox");
    let original = fs::read(&input).expect("the copied input is readable");

    let output = run_play(&input, &input, "quit\n");

    assert_eq!(output.status.code(), Some(2), "stdout: {}", stdout(&output));
    let after = fs::read(&input).expect("the input file is still readable");
    assert_eq!(
        original, after,
        "an output that names the same file as the input must leave it byte-identical"
    );
    assert!(
        !directory.join("in.ndjson.partial").exists(),
        "no partial file must be created for a refused run"
    );
}

#[test]
fn play_rejects_bad_arguments_as_exit_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("play")
        .output()
        .expect("the summoners binary runs");

    assert_eq!(output.status.code(), Some(2), "stdout: {}", stdout(&output));
}
