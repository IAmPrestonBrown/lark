//! Invariant R over every real file in the repository.
//!
//! The property tests build inputs from a generator. This one uses the files
//! that the project already holds: every fixture, every example, and every C
//! file of the runtime. A file that the parser cannot round trip is a file
//! that a language server or a formatter would corrupt.
//!
//! Test type T2 in docs/test-strategy.md.
//! covers: L-13, S-1

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};

use lark_syntax::{NoNames, parse, tokenize};

/// Returns the repository root, from the directory of this crate.
fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

/// Collects every file under a directory whose extension is in the list.
fn files_under(root: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, extensions, found);
            continue;
        }
        let matches = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| extensions.contains(&value));
        if matches {
            found.push(path);
        }
    }
}

/// Returns every Lark and C file that the repository holds.
fn corpus() -> Vec<PathBuf> {
    let root = repository_root();
    let mut found = Vec::new();
    for directory in ["tests", "examples", "runtime", "docs"] {
        files_under(&root.join(directory), &["lark", "c", "h"], &mut found);
    }
    found.sort();
    found
}

#[test]
fn the_corpus_is_not_empty() {
    let files = corpus();
    // The count guards against a path change that quietly tests nothing.
    assert!(
        files.len() > 40,
        "the corpus holds only {} files, which is too few",
        files.len()
    );
}

/// Invariant R over every file the project holds.
#[test]
fn every_file_in_the_repository_round_trips() {
    let mut checked = 0;
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = parse(&source, &NoNames);
        assert_eq!(
            parsed.text(),
            source,
            "invariant R failed for {}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 40, "only {checked} files were read");
}

/// Rule L-13 over every file the project holds.
#[test]
fn every_file_in_the_repository_lexes_back() {
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lexed = tokenize(&source);
        assert_eq!(
            lexed.join(&source),
            source,
            "rule L-13 failed for {}",
            path.display()
        );
    }
}

/// Parsing the same text twice gives the same tree. A parser that reads a
/// global, or that depends on an allocation address, fails this.
#[test]
fn parsing_is_deterministic() {
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let first = parse(&source, &NoNames);
        let second = parse(&source, &NoNames);
        assert_eq!(
            first.tree_text(),
            second.tree_text(),
            "two parses differ for {}",
            path.display()
        );
        assert_eq!(first.errors().len(), second.errors().len());
    }
}

/// Every error span sits inside the file, and its end is not before its start.
/// A span that runs past the end crashes a renderer that slices the source.
#[test]
fn every_error_span_lies_inside_its_file() {
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let parsed = parse(&source, &NoNames);
        let length = u32::try_from(source.len()).unwrap_or(u32::MAX);
        for error in parsed.errors() {
            assert!(
                error.span.start <= error.span.end,
                "a span runs backwards in {}",
                path.display()
            );
            assert!(
                error.span.end <= length,
                "a span runs past the end of {}",
                path.display()
            );
        }
    }
}
