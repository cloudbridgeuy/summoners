//! Pure planning for a recording command's output file lifecycle.
//!
//! A recording command writes to a `.partial` sibling of the requested
//! output path while a match is active, and only renames it to the
//! requested path once recording reaches a complete `match_completed`
//! record. The path decision itself is pure: the caller probes whether the
//! output path already exists and passes that fact in, so this module never
//! touches the file system.

use std::{
    io,
    path::{Path, PathBuf},
};

use crate::error::ShellError;

/// The partial and final paths one recording run should write to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputPlan {
    /// Where the recording is written while the match is still active.
    pub partial: PathBuf,
    /// Where the partial recording is renamed to once it completes.
    pub output: PathBuf,
}

/// Decide the partial and final paths for one recording run.
///
/// Refuses to plan over an existing output path unless `force` is set,
/// reporting that refusal as a file failure that names the path and says
/// to use `--force`. `exists` is the caller's own probe of the output
/// path; this function performs no file-system work itself.
pub fn plan_output(
    command: &'static str,
    output: &Path,
    force: bool,
    exists: bool,
) -> Result<OutputPlan, ShellError> {
    if exists && !force {
        return Err(ShellError::Io {
            command,
            path: output.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "the output file already exists; rerun with --force to overwrite it",
            ),
        });
    }
    Ok(OutputPlan {
        partial: partial_path(output),
        output: output.to_path_buf(),
    })
}

fn partial_path(output: &Path) -> PathBuf {
    let mut name = output.as_os_str().to_owned();
    name.push(".partial");
    PathBuf::from(name)
}

/// Whether two already-resolved paths name the same file.
///
/// Pure: it takes each path already reduced to whatever canonical form
/// the caller could produce, the same split `plan_output` uses for its
/// `exists` probe. Pass `None` when resolving a path failed — most often
/// because it does not exist yet. A path that could not be resolved never
/// collides with anything, so a genuinely missing input or output is left
/// for its own file-system error to report instead of being mistaken for
/// a collision.
#[must_use]
pub fn same_file(from: Option<&Path>, output: Option<&Path>) -> bool {
    matches!((from, output), (Some(from), Some(output)) if from == output)
}

/// Refuse to plan an output path that names the same file as an input the
/// command also reads (a scenario given via `--from`, most often).
///
/// Both paths are canonicalized before comparison, so a relative path and
/// an absolute path naming the same file are still caught. A path that does
/// not resolve — usually because it does not exist yet — never collides
/// with anything; a genuinely missing input is left for its own file-system
/// error instead of being mistaken for a collision. Shared by every command
/// that both reads a transcript and writes one elsewhere.
pub fn refuse_same_file(
    command: &'static str,
    from: &Path,
    output: &Path,
) -> Result<(), ShellError> {
    let from_resolved = std::fs::canonicalize(from).ok();
    let output_resolved = std::fs::canonicalize(output).ok();

    if same_file(from_resolved.as_deref(), output_resolved.as_deref()) {
        let resolved = from_resolved.as_deref().unwrap_or(from);
        return Err(ShellError::Usage(format!(
            "{command}: --output must not name the same file as --from: both {} and {} resolve to {}",
            from.display(),
            output.display(),
            resolved.display()
        )));
    }
    Ok(())
}

/// Rename a completed partial recording to its requested output path.
///
/// Call this only once recording has reached a complete `match_completed`
/// record; a `.partial` file must never be presented as a valid transcript.
pub fn finish(command: &'static str, plan: &OutputPlan) -> Result<(), ShellError> {
    std::fs::rename(&plan.partial, &plan.output).map_err(|source| ShellError::Io {
        command,
        path: plan.output.clone(),
        source,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const COMMAND: &str = "replay";

    #[test]
    fn plan_output_writes_to_a_partial_sibling_when_the_output_is_absent_and_force_is_false() {
        let plan = plan_output(COMMAND, Path::new("match.ndjson"), false, false)
            .expect("a fresh output path is always plannable");

        assert_eq!(
            plan,
            OutputPlan {
                partial: PathBuf::from("match.ndjson.partial"),
                output: PathBuf::from("match.ndjson"),
            }
        );
    }

    #[test]
    fn plan_output_writes_to_a_partial_sibling_when_the_output_is_absent_and_force_is_true() {
        let plan = plan_output(COMMAND, Path::new("match.ndjson"), true, false)
            .expect("a fresh output path is always plannable");

        assert_eq!(
            plan,
            OutputPlan {
                partial: PathBuf::from("match.ndjson.partial"),
                output: PathBuf::from("match.ndjson"),
            }
        );
    }

    #[test]
    fn plan_output_refuses_an_existing_output_without_force() {
        let error = plan_output(COMMAND, Path::new("match.ndjson"), false, true)
            .expect_err("an existing output path is refused without --force");

        assert!(matches!(error, ShellError::Io { .. }));
        let message = error.to_string();
        assert!(
            message.contains("match.ndjson"),
            "unexpected message: {message}"
        );
        assert!(message.contains("--force"), "unexpected message: {message}");
    }

    #[test]
    fn plan_output_replaces_an_existing_output_with_force() {
        let plan = plan_output(COMMAND, Path::new("match.ndjson"), true, true)
            .expect("an existing output path is plannable with --force");

        assert_eq!(
            plan,
            OutputPlan {
                partial: PathBuf::from("match.ndjson.partial"),
                output: PathBuf::from("match.ndjson"),
            }
        );
    }

    #[test]
    fn same_file_is_true_when_both_paths_resolved_and_matched() {
        let resolved = Path::new("/tmp/match.ndjson");

        assert!(same_file(Some(resolved), Some(resolved)));
    }

    #[test]
    fn same_file_is_false_when_the_resolved_paths_differ() {
        assert!(!same_file(
            Some(Path::new("/tmp/a.ndjson")),
            Some(Path::new("/tmp/b.ndjson"))
        ));
    }

    #[test]
    fn same_file_is_false_when_either_path_could_not_be_resolved() {
        let resolved = Path::new("/tmp/match.ndjson");

        assert!(!same_file(None, Some(resolved)));
        assert!(!same_file(Some(resolved), None));
        assert!(!same_file(None, None));
    }

    #[test]
    fn refuse_same_file_accepts_two_different_existing_paths() {
        let directory = std::env::temp_dir().join(format!(
            "summoners-shell-output-unit-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the temporary directory is created");
        let from = directory.join("a.ndjson");
        let output = directory.join("b.ndjson");
        std::fs::write(&from, b"a").expect("the first file is written");
        std::fs::write(&output, b"b").expect("the second file is written");

        let result = refuse_same_file(COMMAND, &from, &output);

        assert!(result.is_ok(), "unexpected error: {result:?}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn refuse_same_file_rejects_a_path_that_resolves_to_the_same_file() {
        let directory = std::env::temp_dir().join(format!(
            "summoners-shell-output-unit-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&directory).expect("the temporary directory is created");
        let path = directory.join("a.ndjson");
        std::fs::write(&path, b"a").expect("the file is written");

        let error = refuse_same_file(COMMAND, &path, &path)
            .expect_err("the same path used twice must be refused");

        assert!(
            matches!(error, ShellError::Usage(_)),
            "unexpected error: {error}"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn refuse_same_file_allows_a_from_path_that_does_not_exist_yet() {
        let directory = std::env::temp_dir().join(format!(
            "summoners-shell-output-unit-{}-{}",
            std::process::id(),
            line!()
        ));
        let missing = directory.join("missing.ndjson");
        let output = directory.join("output.ndjson");

        let result = refuse_same_file(COMMAND, &missing, &output);

        assert!(
            result.is_ok(),
            "an unresolved path must never be mistaken for a collision: {result:?}"
        );
    }
}
