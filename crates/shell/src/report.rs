//! Pure success-line formatting for the shell's commands.

use std::path::Path;

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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

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
}
