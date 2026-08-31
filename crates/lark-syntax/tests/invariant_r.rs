//! Checks invariant R over every real source file in the repository.
//!
//! Rule L-13 states that the tokens of a file join back into the file. A unit
//! test proves it for small samples. This test proves it for every fixture and
//! every example that the project holds.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};

/// Returns the root of the repository.
fn repository_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    root.canonicalize().unwrap_or(root)
}

/// Returns every Lark file and every C file under the source directories.
fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = ["tests", "examples", "docs"]
        .iter()
        .map(|name| root.join(name))
        .collect();

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("lark" | "c")
            ) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// covers: L-13
#[test]
fn every_source_file_in_the_repository_holds_invariant_r() {
    let root = repository_root();
    let files = source_files(&root);
    assert!(
        !files.is_empty(),
        "no source file found under {}",
        root.display()
    );

    for path in files {
        let Ok(source) = std::fs::read_to_string(&path) else {
            panic!("cannot read {}", path.display());
        };
        let lexed = lark_syntax::tokenize(&source);
        assert_eq!(
            lexed.join(&source),
            source,
            "invariant R fails for {}",
            path.display()
        );

        let mut next = 0;
        for token in &lexed.tokens {
            assert_eq!(
                token.span.start,
                next,
                "a gap before offset {} in {}",
                token.span.start,
                path.display()
            );
            next = token.span.end;
        }
        assert_eq!(
            next as usize,
            source.len(),
            "the tokens stop early in {}",
            path.display()
        );
    }
}

#[test]
fn the_example_tour_holds_no_lexer_error() {
    let root = repository_root();
    let path = root.join("examples/tour.lark");
    let Ok(source) = std::fs::read_to_string(&path) else {
        panic!("cannot read {}", path.display());
    };
    let lexed = lark_syntax::tokenize(&source);
    assert!(
        lexed.errors.is_empty(),
        "the tour holds lexer errors: {:?}",
        lexed.errors
    );
}
