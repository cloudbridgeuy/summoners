//! Pure result formatting for the shell's commands.

use std::path::Path;

use summoners_match_log::compare::TranscriptDifference;

use crate::play::PlaySummary;
use crate::replay::ReplaySummary;
use crate::verify::VerifySummary;

/// The stdout line for one successful `verify` run.
#[must_use]
pub fn verify_success(path: &Path, summary: &VerifySummary) -> String {
    format!(
        "verify: {}: ok ({} steps, {} events)",
        path.display(),
        summary.steps,
        summary.events
    )
}

/// The stdout line for one successful `replay` run whose observed
/// transcript is semantically equal to the expected one.
///
/// Labels both paths with the same `expected`/`observed` vocabulary the
/// failure line (`replay_difference`) uses, so a reader never has to guess
/// which path is which.
#[must_use]
pub fn replay_success(from: &Path, output: &Path, summary: &ReplaySummary) -> String {
    format!(
        "replay: expected {}, observed {}: ok ({} steps, {} events)",
        from.display(),
        output.display(),
        summary.steps,
        summary.events
    )
}

/// The stdout line for one interactive `play` session that reached a
/// terminal outcome and was renamed into place.
#[must_use]
pub fn play_success(output: &Path, summary: &PlaySummary) -> String {
    format!(
        "play: {}: ok ({} steps, {} events)",
        output.display(),
        summary.steps,
        summary.events
    )
}

/// The stderr line for one `replay` run whose observed transcript differs
/// from the expected one.
///
/// Labels both paths as `expected` and `observed` so neither reads as part
/// of the trailing difference message, and carries every field the typed
/// `TranscriptDifference` has: sequence, step, event index, stable path,
/// expected value, and actual value.
#[must_use]
pub fn replay_difference(
    command: &'static str,
    expected_path: &Path,
    observed_path: &Path,
    difference: &TranscriptDifference,
) -> String {
    format!(
        "{command}: expected {}, observed {}: sequence {}, step {}, event {}, path {}: expected {}, found {}",
        expected_path.display(),
        observed_path.display(),
        difference.sequence,
        format_optional_step(difference.step),
        format_optional_step(difference.event_index),
        difference.path,
        format_optional_value(difference.expected.as_ref()),
        format_optional_value(difference.actual.as_ref()),
    )
}

fn format_optional_step(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

fn format_optional_value(value: Option<&serde_json::Value>) -> String {
    value.map_or_else(|| "none".to_string(), serde_json::Value::to_string)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::{fs, path::PathBuf};

    use summoners_match_log::compare::{
        ComparisonOptions, TranscriptComparison, compare_transcripts,
    };

    use super::*;

    fn golden(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../match-log/tests/goldens")
            .join(name)
    }

    fn sample_difference() -> TranscriptDifference {
        let expected =
            fs::read_to_string(golden("terminal_empty_deck.ndjson")).expect("golden reads");
        let actual = fs::read_to_string(golden("resignation.ndjson")).expect("golden reads");

        match compare_transcripts(
            expected.as_bytes(),
            actual.as_bytes(),
            ComparisonOptions::default(),
        )
        .expect("two distinct goldens compare without error")
        {
            TranscriptComparison::Different(difference) => difference,
            TranscriptComparison::Equal => panic!("two distinct goldens must not compare equal"),
        }
    }

    #[test]
    fn verify_success_names_the_path_and_the_counts() {
        let summary = VerifySummary {
            steps: 4,
            events: 9,
        };

        assert_eq!(
            verify_success(&PathBuf::from("match.ndjson"), &summary),
            "verify: match.ndjson: ok (4 steps, 9 events)"
        );
    }

    #[test]
    fn replay_success_names_both_paths_and_the_counts() {
        let summary = ReplaySummary {
            steps: 4,
            events: 5,
        };

        assert_eq!(
            replay_success(
                &PathBuf::from("expected.ndjson"),
                &PathBuf::from("observed.ndjson"),
                &summary
            ),
            "replay: expected expected.ndjson, observed observed.ndjson: ok (4 steps, 5 events)"
        );
    }

    #[test]
    fn play_success_names_the_path_and_the_counts() {
        let summary = PlaySummary {
            steps: 3,
            events: 2,
        };

        assert_eq!(
            play_success(&PathBuf::from("played.ndjson"), &summary),
            "play: played.ndjson: ok (3 steps, 2 events)"
        );
    }

    #[test]
    fn replay_difference_names_both_paths_and_the_first_typed_difference() {
        let difference = sample_difference();

        let message = replay_difference(
            "replay",
            &PathBuf::from("expected.ndjson"),
            &PathBuf::from("observed.ndjson"),
            &difference,
        );

        assert!(
            message.starts_with(
                "replay: expected expected.ndjson, observed observed.ndjson: sequence "
            ),
            "unexpected message: {message}"
        );
        assert!(
            message.contains(&format!("sequence {}", difference.sequence)),
            "unexpected message: {message}"
        );
        assert!(
            message.contains(difference.path.as_str()),
            "unexpected message: {message}"
        );
    }
}
