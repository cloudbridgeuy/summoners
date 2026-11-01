//! Pure failure summary for lint output.

/// Maximum number of output lines shown for a failed check.
pub const MAX_SUMMARY_LINES: usize = 20;

/// Remove ANSI CSI escape sequences (`ESC [ ... final-byte`) from `text`.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&next) {
                    break;
                }
            }
        }
    }
    out
}

/// Count diagnostic lines in already ANSI-stripped output as `(errors, warnings)`.
///
/// Errors are lines starting with `error`; warnings are lines starting with
/// `warning`. Cargo summary lines are excluded: `error: could not compile`,
/// `error: aborting due to`, `warning: ... generated N warning(s)` and
/// `warning: build failed`.
pub fn count_diagnostics(plain: &str) -> (usize, usize) {
    let mut errors = 0;
    let mut warnings = 0;
    for line in plain.lines() {
        if line.starts_with("error") {
            if !(line.starts_with("error: could not compile")
                || line.starts_with("error: aborting due to"))
            {
                errors += 1;
            }
        } else if line.starts_with("warning")
            && !line.contains(" generated ")
            && !line.starts_with("warning: build failed")
        {
            warnings += 1;
        }
    }
    (errors, warnings)
}

/// Build the short failure report: name, first `max_lines` output lines, a
/// truncation note, diagnostic counts, and the log path.
pub fn failure_summary(name: &str, output: &str, log_path: &str, max_lines: usize) -> String {
    let plain = strip_ansi(output);
    let total = plain.lines().count();
    let mut out = format!("lint failed at: {name}\n");
    for line in plain.lines().take(max_lines) {
        out.push_str(line);
        out.push('\n');
    }
    if total > max_lines {
        out.push_str(&format!("... {} more lines in log\n", total - max_lines));
    }
    out.push_str(&failure_footer(&plain, log_path));
    out
}

/// Build the `errors: E, warnings: W` and `log:` lines for `output`.
pub fn failure_footer(output: &str, log_path: &str) -> String {
    let (errors, warnings) = count_diagnostics(&strip_ansi(output));
    format!("errors: {errors}, warnings: {warnings}\nlog: {log_path}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_output_has_no_truncation_line() {
        let s = failure_summary("fmt", "a\nb\n", "/tmp/l.log", 20);
        assert_eq!(
            s,
            "lint failed at: fmt\na\nb\nerrors: 0, warnings: 0\nlog: /tmp/l.log\n"
        );
    }

    #[test]
    fn long_output_is_truncated_with_remaining_count() {
        let out: String = (1..=25).map(|i| format!("line {i}\n")).collect();
        let s = failure_summary("clippy", &out, "/l", 20);
        assert!(s.contains("line 20\n"));
        assert!(!s.contains("line 21\n"));
        assert!(s.contains("... 5 more lines in log\n"));
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\x1b[1m\x1b[31merror\x1b[0m: x"), "error: x");
        let s = failure_summary("c", "\x1b[31merror[E0308]\x1b[0m: bad\n", "/l", 20);
        assert!(s.contains("error[E0308]: bad\n"));
        assert!(!s.contains('\x1b'));
        assert!(s.contains("errors: 1, warnings: 0"));
    }

    #[test]
    fn counts_errors_and_warnings_excluding_summaries() {
        let out = "warning: unused\nwarning: unused y\nerror: boom\nerror[E0001]: bad\n  error: indented\n\
warning: `x` (lib) generated 2 warnings\nerror: could not compile `x` (lib) due to 1 previous error\n\
error: aborting due to 1 previous error\n";
        assert_eq!(count_diagnostics(out), (2, 2));
    }

    #[test]
    fn zero_errors() {
        let s = failure_summary("file length", "too long\n", "/l", 20);
        assert!(s.contains("errors: 0, warnings: 0\n"));
    }
}
