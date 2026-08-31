//! Checks that the catalogue in the code matches chapter 12 of the specification.
//!
//! The specification is the authority. A code that appears in one place and not
//! in the other is a defect, and this test names it.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lark_diag::{CATALOG, Severity};

/// One row of a catalogue table in the specification.
#[derive(Debug, PartialEq, Eq)]
struct SpecEntry {
    rule: String,
    message: String,
    severity: Severity,
}

fn spec_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/12-diagnostics.md")
}

/// Reads every catalogue row from the specification.
fn read_spec() -> BTreeMap<String, SpecEntry> {
    let path = spec_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("cannot read {}: {error}", path.display()),
    };

    let warnings = read_warning_codes(&text);
    let mut entries = BTreeMap::new();

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("| `LK") {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let code = cells[0].trim_matches('`').to_owned();
        if !is_code(&code) {
            // A range table row, such as `LK01xx`, describes a group.
            continue;
        }
        assert_eq!(cells.len(), 3, "a catalogue row needs three cells: {line}");

        let severity = if warnings.contains(&code) {
            Severity::Warning
        } else {
            Severity::Error
        };
        let entry = SpecEntry {
            rule: cells[1].to_owned(),
            message: cells[2].to_owned(),
            severity,
        };
        assert!(
            entries.insert(code.clone(), entry).is_none(),
            "{code} appears twice"
        );
    }

    assert!(
        !entries.is_empty(),
        "the specification lists no diagnostic code"
    );
    entries
}

/// Reports whether the text has the shape of a diagnostic code.
fn is_code(text: &str) -> bool {
    match text.strip_prefix("LK") {
        Some(digits) => digits.len() == 4 && digits.bytes().all(|byte| byte.is_ascii_digit()),
        None => false,
    }
}

/// Reads the warning code list from section 3 of the specification.
fn read_warning_codes(text: &str) -> Vec<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("**Warning codes:**") {
            return rest
                .split(',')
                .map(|item| item.trim().trim_matches('`').to_owned())
                .filter(|item| !item.is_empty())
                .collect();
        }
    }
    Vec::new()
}

#[test]
fn every_spec_code_exists_in_the_catalogue() {
    let spec = read_spec();
    let mut missing = Vec::new();
    for code in spec.keys() {
        if !CATALOG.iter().any(|entry| entry.code.to_string() == *code) {
            missing.push(code.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "the code lists these but the catalogue does not: {missing:?}"
    );
}

#[test]
fn every_catalogue_code_exists_in_the_spec() {
    let spec = read_spec();
    let mut extra = Vec::new();
    for entry in CATALOG {
        let code = entry.code.to_string();
        if !spec.contains_key(&code) {
            extra.push(code);
        }
    }
    assert!(
        extra.is_empty(),
        "the catalogue lists these but the spec does not: {extra:?}"
    );
}

#[test]
fn the_message_the_rule_and_the_severity_match() {
    let spec = read_spec();
    let mut wrong = Vec::new();
    for entry in CATALOG {
        let code = entry.code.to_string();
        let Some(expected) = spec.get(&code) else {
            continue;
        };
        if entry.message != expected.message {
            wrong.push(format!(
                "{code} message: code has {:?}, spec has {:?}",
                entry.message, expected.message
            ));
        }
        if entry.rule != expected.rule {
            wrong.push(format!(
                "{code} rule: code has {:?}, spec has {:?}",
                entry.rule, expected.rule
            ));
        }
        if entry.severity != expected.severity {
            wrong.push(format!(
                "{code} severity: code has {:?}, spec has {:?}",
                entry.severity, expected.severity
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the catalogue and the spec disagree:\n{}",
        wrong.join("\n")
    );
}
