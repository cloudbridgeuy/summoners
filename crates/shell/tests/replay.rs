//! Subprocess tests driving the real `summoners` binary's `replay`
//! command, covering every exit code and file-lifecycle case the command
//! line contract promises.

use std::{
    fs,
    process::{Command, Output},
};

mod support;

use support::{TemporaryDirectory, golden};

fn run_replay<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("replay")
        .args(args)
        .output()
        .expect("the summoners binary runs")
}

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

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn replay_reproduces_the_terminal_empty_deck_golden() {
    let directory = TemporaryDirectory::new("terminal-empty-deck");
    let expected = golden("terminal_empty_deck.ndjson");
    let observed = directory.join("observed.ndjson");

    let output = run_replay([
        "--from".as_ref(),
        expected.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
    ]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(observed.exists(), "the complete observed file must exist");

    let verify_output = run_verify([observed.as_os_str()]);
    assert_eq!(
        verify_output.status.code(),
        Some(0),
        "the observed file must be a valid transcript on its own: {}",
        stderr(&verify_output)
    );
}

#[test]
fn replay_reproduces_the_resignation_golden() {
    let directory = TemporaryDirectory::new("resignation");
    let expected = golden("resignation.ndjson");
    let observed = directory.join("observed.ndjson");

    let output = run_replay([
        "--from".as_ref(),
        expected.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
    ]);

    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    assert!(observed.exists(), "the complete observed file must exist");

    let verify_output = run_verify([observed.as_os_str()]);
    assert_eq!(
        verify_output.status.code(),
        Some(0),
        "the observed file must be a valid transcript on its own: {}",
        stderr(&verify_output)
    );
}

#[test]
fn a_second_run_without_force_refuses_and_leaves_the_existing_file_untouched() {
    let directory = TemporaryDirectory::new("no-force");
    let expected = golden("resignation.ndjson");
    let observed = directory.join("observed.ndjson");

    let first = run_replay([
        "--from".as_ref(),
        expected.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
    ]);
    assert_eq!(first.status.code(), Some(0), "stderr: {}", stderr(&first));
    let original_contents = fs::read(&observed).expect("the first observed file is readable");

    let second = run_replay([
        "--from".as_ref(),
        expected.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
    ]);

    assert_eq!(second.status.code(), Some(4), "stdout: {}", stdout(&second));
    let message = stderr(&second);
    assert!(
        message.contains(&observed.display().to_string()),
        "unexpected stderr: {message}"
    );
    assert!(message.contains("--force"), "unexpected stderr: {message}");

    let contents_after = fs::read(&observed).expect("the existing observed file is still readable");
    assert_eq!(
        original_contents, contents_after,
        "the existing output file must be left untouched"
    );
    assert!(
        !directory.join("observed.ndjson.partial").exists(),
        "no partial file must be left behind by a refused run"
    );
}

#[test]
fn a_second_run_with_force_succeeds() {
    let directory = TemporaryDirectory::new("force");
    let expected = golden("resignation.ndjson");
    let observed = directory.join("observed.ndjson");

    let first = run_replay([
        "--from".as_ref(),
        expected.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
    ]);
    assert_eq!(first.status.code(), Some(0), "stderr: {}", stderr(&first));

    let second = run_replay([
        "--from".as_ref(),
        expected.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
        "--force".as_ref(),
    ]);

    assert_eq!(second.status.code(), Some(0), "stderr: {}", stderr(&second));
    assert!(observed.exists(), "the complete observed file must exist");
}

#[test]
fn an_output_naming_the_same_file_as_from_is_refused_and_leaves_the_input_untouched() {
    let directory = TemporaryDirectory::new("same-file");
    let input = directory.join("in.ndjson");
    fs::copy(golden("resignation.ndjson"), &input).expect("the golden copies into the sandbox");
    let original = fs::read(&input).expect("the copied input is readable");

    let output = run_replay([
        "--from".as_ref(),
        input.as_os_str(),
        "--output".as_ref(),
        input.as_os_str(),
        "--force".as_ref(),
    ]);

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
fn replay_reports_a_missing_input_file_as_exit_four() {
    let directory = TemporaryDirectory::new("missing-input");
    let missing = directory.join("does-not-exist.ndjson");
    let observed = directory.join("observed.ndjson");

    let output = run_replay([
        "--from".as_ref(),
        missing.as_os_str(),
        "--output".as_ref(),
        observed.as_os_str(),
    ]);

    assert_eq!(output.status.code(), Some(4), "stdout: {}", stdout(&output));
    let message = stderr(&output);
    assert!(
        message.starts_with(&format!("replay: {}: ", missing.display())),
        "unexpected stderr: {message}"
    );
}

#[test]
fn replay_rejects_bad_arguments_as_exit_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_summoners"))
        .arg("replay")
        .output()
        .expect("the summoners binary runs");

    assert_eq!(output.status.code(), Some(2), "stdout: {}", stdout(&output));
}
