//! Subprocess tests driving the real `summoners` binary's `verify`
//! command, covering every exit code the command line contract promises.

use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

mod support;

use support::{TemporaryDirectory, golden};

fn run_verify<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("verify")
        .args(args)
        .output()
        .expect("the summoners binary runs")
}

fn tampered_terminal_empty_deck(directory: &TemporaryDirectory) -> PathBuf {
    let original = fs::read_to_string(golden("terminal_empty_deck.ndjson"))
        .expect("the terminal empty deck golden is readable");
    let expected = r#"{"sequence":7,"record":"event","step":3,"index":0,"event":{"kind":"priority_passed","player":"two"}}"#;
    let replacement = r#"{"sequence":7,"record":"event","step":3,"index":0,"event":{"kind":"priority_passed","player":"one"}}"#;
    let changed = original.replacen(expected, replacement, 1);
    assert_ne!(changed, original, "one event line must change");

    let path = directory.join("tampered.ndjson");
    fs::write(&path, changed).expect("the tampered fixture is written");
    path
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn verify_succeeds_on_the_terminal_empty_deck_golden() {
    let path = golden("terminal_empty_deck.ndjson");

    let output = run_verify([path.as_os_str()]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let line = stdout(&output);
    assert_eq!(
        line.trim_end(),
        format!("verify: {}: ok (4 steps, 5 events)", path.display())
    );
}

#[test]
fn verify_succeeds_on_the_resignation_golden() {
    let path = golden("resignation.ndjson");

    let output = run_verify([path.as_os_str()]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let line = stdout(&output);
    assert_eq!(
        line.trim_end(),
        format!("verify: {}: ok (1 steps, 1 events)", path.display())
    );
}

#[test]
fn verify_reports_the_first_typed_difference_on_a_tampered_transcript() {
    let directory = TemporaryDirectory::new("tampered");
    let path = tampered_terminal_empty_deck(&directory);

    let output = run_verify([path.as_os_str()]);

    assert_eq!(output.status.code(), Some(3), "stdout: {}", stdout(&output));
    let message = stderr(&output);
    assert!(
        message.starts_with(&format!("verify: {}: ", path.display())),
        "unexpected stderr: {message}"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn verify_reports_a_missing_file_as_exit_four() {
    let directory = TemporaryDirectory::new("missing");
    let path = directory.join("does-not-exist.ndjson");

    let output = run_verify([path.as_os_str()]);

    assert_eq!(output.status.code(), Some(4), "stdout: {}", stdout(&output));
    let message = stderr(&output);
    assert!(
        message.starts_with(&format!("verify: {}: ", path.display())),
        "unexpected stderr: {message}"
    );
}

#[test]
fn verify_rejects_bad_arguments_as_exit_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("verify")
        .output()
        .expect("the summoners binary runs");

    assert_eq!(output.status.code(), Some(2), "stdout: {}", stdout(&output));
}
