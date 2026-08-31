//! Specification rule coverage.
//!
//! Principle P-6 requires that every rule in `docs/spec` maps to at least one
//! test. This module finds the rules, finds the claims, and compares them.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

/// The result of a scan for specification rule coverage.
///
/// See principle P-6 in `docs/test-strategy.md`. Every rule maps to at least
/// one test. A rule with no test appears in the baseline until it gets one.
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    /// Every rule that the specification states.
    pub rules: BTreeSet<String>,
    /// Every rule that a test claims to cover.
    pub covered: BTreeSet<String>,
    /// Every rule that no test claims.
    pub uncovered: BTreeSet<String>,
    /// Every claim that names a rule the specification does not state.
    pub unknown_claims: BTreeSet<String>,
}

/// The name of the file that records the rules with no test.
pub const BASELINE: &str = "tests/rule-coverage-baseline.txt";

/// Scans the specification and the tests, and reports coverage.
///
/// A test claims a rule with a `covers:` marker. The marker holds one rule, or
/// several separated by commas.
///
/// ```text
/// // covers: M-11, M-12
/// ```
///
/// # Errors
///
/// Returns an error when a directory cannot be read.
pub fn scan(root: &Path) -> io::Result<Coverage> {
    let mut coverage = Coverage::default();

    for path in files_under(&root.join("docs/spec"))? {
        if path.extension().is_some_and(|extension| extension == "md") {
            let text = std::fs::read_to_string(&path)?;
            coverage.rules.extend(rules_in(&text));
        }
    }

    for directory in ["tests", "crates", "runtime"] {
        for path in files_under(&root.join(directory))? {
            if is_scannable(&path) {
                let text = std::fs::read_to_string(&path)?;
                coverage.covered.extend(claims_in(&text));
            }
        }
    }

    coverage.unknown_claims = coverage
        .covered
        .difference(&coverage.rules)
        .cloned()
        .collect();
    coverage
        .covered
        .retain(|rule| coverage.rules.contains(rule));
    coverage.uncovered = coverage
        .rules
        .difference(&coverage.covered)
        .cloned()
        .collect();
    Ok(coverage)
}

/// Checks the scan against the baseline file.
///
/// The baseline must list exactly the rules that no test covers. A rule that
/// gains a test leaves the list. A new rule with no test joins it.
///
/// # Errors
///
/// Returns a report when the baseline and the scan disagree, or when a test
/// claims a rule that the specification does not state.
pub fn check(root: &Path, coverage: &Coverage) -> Result<(), String> {
    let path = root.join(BASELINE);
    let mut report = String::new();

    if !coverage.unknown_claims.is_empty() {
        let _ = writeln!(
            report,
            "a test claims a rule that the specification does not state:"
        );
        for rule in &coverage.unknown_claims {
            let _ = writeln!(report, "  {rule}");
        }
    }

    let baseline = read_baseline(&path).unwrap_or_default();

    let grew: Vec<&String> = coverage.uncovered.difference(&baseline).collect();
    if !grew.is_empty() {
        let _ = writeln!(
            report,
            "these rules lost their test, or arrived without one:"
        );
        for rule in grew {
            let _ = writeln!(report, "  {rule}");
        }
    }

    let shrank: Vec<&String> = baseline.difference(&coverage.uncovered).collect();
    if !shrank.is_empty() {
        let _ = writeln!(
            report,
            "these rules gained a test, so the baseline can shrink:"
        );
        for rule in shrank {
            let _ = writeln!(report, "  {rule}");
        }
    }

    if report.is_empty() {
        return Ok(());
    }
    let _ = writeln!(report);
    let _ = writeln!(
        report,
        "{} lists {} rules, and the scan found {} without a test",
        BASELINE,
        baseline.len(),
        coverage.uncovered.len()
    );
    let _ = writeln!(
        report,
        "run the suite again with LARK_BLESS=1 to rewrite the baseline"
    );
    Err(report)
}

/// Writes the baseline file from a scan.
///
/// # Errors
///
/// Returns an error when the file cannot be written.
pub fn write_baseline(root: &Path, coverage: &Coverage) -> io::Result<()> {
    let path = root.join(BASELINE);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = String::new();
    text.push_str("# Rules with no test. See principle P-6 in docs/test-strategy.md.\n");
    text.push_str("# The list must only shrink. Rewrite it with LARK_BLESS=1.\n");
    for rule in &coverage.uncovered {
        let _ = writeln!(text, "{rule}");
    }
    std::fs::write(path, text)
}

/// Reads the baseline file.
fn read_baseline(path: &Path) -> io::Result<BTreeSet<String>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

/// Returns every rule identifier that a specification chapter states.
fn rules_in(text: &str) -> BTreeSet<String> {
    const MARKER: &str = "**Rule ";
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(position) = rest.find(MARKER) {
        rest = &rest[position + MARKER.len()..];
        if let Some(rule) = read_rule_id(rest) {
            found.insert(rule);
        }
    }
    found
}

/// Returns every rule identifier that a test claims.
fn claims_in(text: &str) -> BTreeSet<String> {
    const MARKER: &str = "covers:";
    let mut found = BTreeSet::new();
    for line in text.lines() {
        let Some(position) = line.find(MARKER) else {
            continue;
        };
        for item in line[position + MARKER.len()..].split(',') {
            if let Some(rule) = read_rule_id(item.trim()) {
                found.insert(rule);
            }
        }
    }
    found
}

/// Reads a rule identifier from the start of a text, such as `M-11`.
///
/// Returns `None` when the text does not start with one.
fn read_rule_id(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_uppercase() {
        index += 1;
    }
    if index == 0 || index > 3 || bytes.get(index) != Some(&b'-') {
        return None;
    }
    let letters = index;
    index += 1;
    let digits_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index == digits_start {
        return None;
    }
    if index < bytes.len() && bytes[index].is_ascii_lowercase() {
        index += 1;
    }
    // A rule identifier ends at a character that cannot be part of one.
    if index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'-') {
        return None;
    }
    let _ = letters;
    Some(text[..index].to_owned())
}

/// Reports whether a file holds text that the scan reads.
fn is_scannable(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("rs" | "lark" | "c" | "h" | "tree" | "toml")
    )
}

/// Returns every file under a directory, including subdirectories.
fn files_under(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    if !directory.is_dir() {
        return Ok(found);
    }
    let mut stack = vec![directory.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::{claims_in, read_rule_id, rules_in};

    #[test]
    fn reads_a_plain_rule_heading() {
        let rules = rules_in("**Rule M-11.** Every slot is null before the push.\n");
        assert!(rules.contains("M-11"), "{rules:?}");
    }

    #[test]
    fn reads_a_rule_heading_with_a_name_after_it() {
        let rules = rules_in("**Rule L-6 (the innermost binding rule).** Resolve the name.\n");
        assert!(rules.contains("L-6"), "{rules:?}");
    }

    #[test]
    fn reads_a_two_letter_prefix() {
        let rules = rules_in("**Rule DQ-1.** Every diagnostic reports the file.\n");
        assert!(rules.contains("DQ-1"), "{rules:?}");
    }

    #[test]
    fn ignores_text_that_is_not_a_rule() {
        assert_eq!(read_rule_id("hello"), None);
        assert_eq!(read_rule_id("M-"), None);
        assert_eq!(read_rule_id("MMMM-1"), None);
        assert_eq!(read_rule_id("M-1x2"), None);
    }

    #[test]
    fn reads_a_claim_with_several_rules() {
        let claims = claims_in("// covers: M-11, M-12,G-10\n");
        assert!(claims.contains("M-11"));
        assert!(claims.contains("M-12"));
        assert!(claims.contains("G-10"));
        assert_eq!(claims.len(), 3);
    }

    #[test]
    fn a_line_with_no_marker_claims_nothing() {
        assert!(claims_in("// this line names M-11 but does not claim it\n").is_empty());
    }
}
