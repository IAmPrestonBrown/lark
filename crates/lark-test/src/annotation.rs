//! Inline diagnostic annotations in a fixture.
//!
//! A fixture states what the compiler must report, on the line it must report
//! it. The harness matches the two and names every difference.

use std::fmt::Write as _;

use lark_diag::{Code, Severity};

/// One diagnostic that a fixture expects.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Expectation {
    /// The one based line that the diagnostic must point at.
    pub line: u32,
    /// The severity that the fixture expects.
    pub severity: Severity,
    /// The code that the fixture expects.
    pub code: Code,
}

/// One diagnostic that the compiler produced.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Actual {
    /// The one based line that the diagnostic points at.
    pub line: u32,
    /// The severity of the diagnostic.
    pub severity: Severity,
    /// The code of the diagnostic.
    pub code: Code,
}

/// A fixture annotation that the harness cannot read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Malformed {
    /// The one based line of the annotation.
    pub line: u32,
    /// What is wrong with it.
    pub reason: String,
}

/// The annotations that one fixture carries.
#[derive(Clone, Default, Debug)]
pub struct Annotations {
    /// Every diagnostic that the fixture expects.
    pub expected: Vec<Expectation>,
    /// Every annotation that the harness cannot read.
    pub malformed: Vec<Malformed>,
}

/// Reads every `//~` annotation from a fixture.
///
/// An annotation names a severity and a code.
///
/// ```text
/// handle_opaque_data(count);   //~ ERROR LK0301
/// ```
///
/// A caret moves the expectation up one line for each caret.
///
/// ```text
/// handle_opaque_data(count);
/// //~^ ERROR LK0301
/// ```
#[must_use]
pub fn parse(text: &str) -> Annotations {
    let mut annotations = Annotations::default();

    for (index, line) in text.lines().enumerate() {
        let Some(position) = line.find("//~") else {
            continue;
        };
        let rest = &line[position + "//~".len()..];
        let carets = rest.bytes().take_while(|byte| *byte == b'^').count();
        let body = rest[carets..].trim();

        // A line number is one based, and a caret moves the expectation up.
        let source_line = index + 1;
        let Some(target) = source_line.checked_sub(carets) else {
            annotations.malformed.push(Malformed {
                line: to_line(source_line),
                reason: format!("{carets} carets reach above the first line"),
            });
            continue;
        };
        if target == 0 {
            annotations.malformed.push(Malformed {
                line: to_line(source_line),
                reason: format!("{carets} carets reach above the first line"),
            });
            continue;
        }

        match read_body(body) {
            Ok((severity, code)) => {
                annotations.expected.push(Expectation {
                    line: to_line(target),
                    severity,
                    code,
                });
            }
            Err(reason) => {
                annotations.malformed.push(Malformed {
                    line: to_line(source_line),
                    reason,
                });
            }
        }
    }

    annotations.expected.sort_unstable();
    annotations
}

/// Reads the severity and the code from the body of an annotation.
fn read_body(body: &str) -> Result<(Severity, Code), String> {
    let mut words = body.split_whitespace();
    let Some(word) = words.next() else {
        return Err("the annotation names no severity".to_owned());
    };
    let severity = match word {
        "ERROR" => Severity::Error,
        "WARNING" => Severity::Warning,
        other => {
            return Err(format!("`{other}` is not ERROR or WARNING"));
        }
    };
    let Some(word) = words.next() else {
        return Err("the annotation names no code".to_owned());
    };
    let Some(code) = Code::parse(word) else {
        return Err(format!("`{word}` is not a code in the catalogue"));
    };
    Ok((severity, code))
}

/// Converts a `usize` line number into the `u32` that a diagnostic carries.
fn to_line(line: usize) -> u32 {
    u32::try_from(line).unwrap_or(u32::MAX)
}

/// Compares what a fixture expects against what the compiler produced.
///
/// # Errors
///
/// Returns a report when a diagnostic is missing, when one is unexpected, or
/// when the fixture holds an annotation that the harness cannot read.
pub fn check(annotations: &Annotations, produced: &[Actual]) -> Result<(), String> {
    let mut report = String::new();

    for item in &annotations.malformed {
        let _ = writeln!(
            report,
            "  line {}: bad annotation: {}",
            item.line, item.reason
        );
    }

    let mut remaining: Vec<Actual> = produced.to_vec();
    let mut missing = Vec::new();

    for expectation in &annotations.expected {
        let found = remaining.iter().position(|actual| {
            actual.line == expectation.line
                && actual.code == expectation.code
                && actual.severity == expectation.severity
        });
        match found {
            Some(index) => {
                remaining.remove(index);
            }
            None => missing.push(*expectation),
        }
    }

    for item in &missing {
        let _ = writeln!(
            report,
            "  line {}: expected {} {} and the compiler did not report it",
            item.line,
            severity_word(item.severity),
            item.code
        );
    }

    remaining.sort_unstable();
    for item in &remaining {
        let _ = writeln!(
            report,
            "  line {}: the compiler reported {} {} and the fixture does not expect it",
            item.line,
            severity_word(item.severity),
            item.code
        );
    }

    if report.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the diagnostics do not match the annotations:\n{report}"
        ))
    }
}

/// Returns the annotation word for a severity.
fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Warning => "WARNING",
        _ => "ERROR",
    }
}

#[cfg(test)]
mod tests {
    use lark_diag::{LK0301, LK0400, Severity};

    use super::{Actual, Expectation, check, parse};

    #[test]
    fn reads_an_annotation_on_the_same_line() {
        let annotations = parse("a\nb //~ ERROR LK0301\nc\n");
        assert_eq!(
            annotations.expected,
            vec![Expectation {
                line: 2,
                severity: Severity::Error,
                code: LK0301
            }]
        );
        assert!(annotations.malformed.is_empty());
    }

    #[test]
    fn a_caret_moves_the_expectation_up_one_line() {
        let annotations = parse("a\nb\n//~^ ERROR LK0301\n");
        assert_eq!(annotations.expected[0].line, 2);
    }

    #[test]
    fn two_carets_move_the_expectation_up_two_lines() {
        let annotations = parse("a\nb\nc\n//~^^ ERROR LK0301\n");
        assert_eq!(annotations.expected[0].line, 2);
    }

    #[test]
    fn reports_a_caret_that_reaches_above_the_first_line() {
        let annotations = parse("//~^ ERROR LK0301\n");
        assert!(annotations.expected.is_empty());
        assert_eq!(annotations.malformed.len(), 1);
        assert!(
            annotations.malformed[0]
                .reason
                .contains("above the first line")
        );
    }

    #[test]
    fn reports_a_code_that_is_not_in_the_catalogue() {
        let annotations = parse("a //~ ERROR LK9999\n");
        assert_eq!(annotations.malformed.len(), 1);
        assert!(annotations.malformed[0].reason.contains("not a code"));
    }

    #[test]
    fn reports_a_severity_that_is_not_a_word_the_harness_knows() {
        let annotations = parse("a //~ FATAL LK0301\n");
        assert_eq!(annotations.malformed.len(), 1);
        assert!(
            annotations.malformed[0]
                .reason
                .contains("not ERROR or WARNING")
        );
    }

    #[test]
    fn a_matching_set_passes() {
        let annotations = parse("a //~ ERROR LK0301\n");
        let produced = [Actual {
            line: 1,
            severity: Severity::Error,
            code: LK0301,
        }];
        assert!(check(&annotations, &produced).is_ok());
    }

    #[test]
    fn a_missing_diagnostic_fails() {
        let annotations = parse("a //~ ERROR LK0301\n");
        let Err(report) = check(&annotations, &[]) else {
            panic!("a missing diagnostic must fail");
        };
        assert!(report.contains("did not report it"), "{report}");
    }

    #[test]
    fn an_unexpected_diagnostic_fails() {
        let annotations = parse("a\n");
        let produced = [Actual {
            line: 1,
            severity: Severity::Error,
            code: LK0301,
        }];
        let Err(report) = check(&annotations, &produced) else {
            panic!("an unexpected diagnostic must fail");
        };
        assert!(report.contains("does not expect it"), "{report}");
    }

    #[test]
    fn a_diagnostic_on_the_wrong_line_fails() {
        let annotations = parse("a //~ ERROR LK0301\nb\n");
        let produced = [Actual {
            line: 2,
            severity: Severity::Error,
            code: LK0301,
        }];
        let Err(report) = check(&annotations, &produced) else {
            panic!("a diagnostic on the wrong line must fail");
        };
        assert!(report.contains("line 1"), "{report}");
        assert!(report.contains("line 2"), "{report}");
    }

    #[test]
    fn a_diagnostic_with_the_wrong_code_fails() {
        let annotations = parse("a //~ ERROR LK0301\n");
        let produced = [Actual {
            line: 1,
            severity: Severity::Error,
            code: LK0400,
        }];
        assert!(check(&annotations, &produced).is_err());
    }

    #[test]
    fn two_annotations_on_one_line_both_match() {
        let annotations = parse("a //~ ERROR LK0301\n//~^ ERROR LK0400\n");
        let produced = [
            Actual {
                line: 1,
                severity: Severity::Error,
                code: LK0301,
            },
            Actual {
                line: 1,
                severity: Severity::Error,
                code: LK0400,
            },
        ];
        assert!(check(&annotations, &produced).is_ok());
    }
}
