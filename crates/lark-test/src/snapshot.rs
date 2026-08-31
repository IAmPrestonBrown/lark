//! Snapshot comparison and bless mode.
//!
//! A snapshot test compares output against a file on disk. Bless mode rewrites
//! the file from the output. See principle P-2.

use std::fmt::Write as _;
use std::path::Path;

/// The result of a snapshot comparison.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The actual output matches the expected file.
    Match,
    /// The expected file was written or rewritten, because bless mode is on.
    Blessed,
    /// The actual output does not match. The string holds a unified report.
    Mismatch(String),
}

/// Reports whether bless mode is on.
///
/// Bless mode rewrites every expected file from the actual output. Turn it on
/// with the environment variable `LARK_BLESS=1`.
pub fn bless_mode() -> bool {
    matches!(std::env::var("LARK_BLESS").as_deref(), Ok("1" | "true"))
}

/// Compares actual output against an expected file.
///
/// In bless mode the function writes the file and returns [`Verdict::Blessed`].
/// A missing file in normal mode is a mismatch, and the report says how to
/// create it.
pub fn compare(expected_path: &Path, actual: &str) -> Verdict {
    if bless_mode() {
        if let Some(parent) = expected_path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            return Verdict::Mismatch(format!("cannot create {}: {error}", parent.display()));
        }
        return match std::fs::write(expected_path, actual) {
            Ok(()) => Verdict::Blessed,
            Err(error) => {
                Verdict::Mismatch(format!("cannot write {}: {error}", expected_path.display()))
            }
        };
    }

    let expected = match std::fs::read_to_string(expected_path) {
        Ok(text) => text,
        Err(error) => {
            return Verdict::Mismatch(format!(
                "cannot read {}: {error}\nrun the suite again with LARK_BLESS=1 to create it",
                expected_path.display()
            ));
        }
    };

    if expected == actual {
        Verdict::Match
    } else {
        Verdict::Mismatch(diff_report(expected_path, &expected, actual))
    }
}

/// Builds a line by line report of the first differences.
fn diff_report(path: &Path, expected: &str, actual: &str) -> String {
    const CONTEXT: usize = 40;

    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let mut report = String::new();
    let _ = writeln!(report, "{} does not match the output", path.display());
    let _ = writeln!(
        report,
        "  expected {} lines, got {}",
        expected_lines.len(),
        actual_lines.len()
    );
    let _ = writeln!(report, "  - marks the expected file, + marks the output");
    let _ = writeln!(report);

    let mut shown = 0;
    for index in 0..expected_lines.len().max(actual_lines.len()) {
        let left = expected_lines.get(index).copied();
        let right = actual_lines.get(index).copied();
        if left == right {
            continue;
        }
        if shown == CONTEXT {
            let _ = writeln!(report, "  ... more differences follow");
            break;
        }
        let number = index + 1;
        match left {
            Some(text) => {
                let _ = writeln!(report, "  {number:>4} - {text}");
            }
            None => {
                let _ = writeln!(report, "  {number:>4} - <end of file>");
            }
        }
        match right {
            Some(text) => {
                let _ = writeln!(report, "  {number:>4} + {text}");
            }
            None => {
                let _ = writeln!(report, "  {number:>4} + <end of file>");
            }
        }
        shown += 1;
    }

    let _ = writeln!(report);
    let _ = writeln!(
        report,
        "run the suite again with LARK_BLESS=1 to accept the output"
    );
    report
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Verdict, compare, diff_report};

    fn temp_file(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("lark-test-snapshot-{name}"));
        path
    }

    #[test]
    fn equal_text_matches() {
        let path = temp_file("equal.txt");
        std::fs::write(&path, "abc\n").unwrap();
        assert_eq!(compare(&path, "abc\n"), Verdict::Match);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn different_text_reports_the_line() {
        let path = temp_file("different.txt");
        std::fs::write(&path, "abc\ndef\n").unwrap();
        let Verdict::Mismatch(report) = compare(&path, "abc\nxyz\n") else {
            panic!("a different text must not match");
        };
        assert!(report.contains("2 - def"), "{report}");
        assert!(report.contains("2 + xyz"), "{report}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_reports_how_to_create_it() {
        let path = temp_file("missing-on-purpose.txt");
        std::fs::remove_file(&path).ok();
        let Verdict::Mismatch(report) = compare(&path, "abc\n") else {
            panic!("a missing file must not match");
        };
        assert!(report.contains("LARK_BLESS=1"), "{report}");
    }

    #[test]
    fn the_report_names_a_shorter_output() {
        let report = diff_report(&PathBuf::from("x.tree"), "a\nb\n", "a\n");
        assert!(report.contains("2 + <end of file>"), "{report}");
    }
}
