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
}
