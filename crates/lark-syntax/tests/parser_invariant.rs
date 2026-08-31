//! Checks invariant R at the tree level, over every real source file.
//!
//! Rule L-13 and the language server both need the tree to hold the whole
//! file. A file that does not parse must still round trip.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};

use lark_syntax::{NoNames, parse};

fn repository_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    root.canonicalize().unwrap_or(root)
}

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
fn every_source_file_round_trips_through_the_tree() {
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
        let parsed = parse(&source, &NoNames);
        assert_eq!(
            parsed.text(),
            source,
            "invariant R fails for {}",
            path.display()
        );
    }
}

/// covers: L-13
#[test]
fn a_file_that_does_not_parse_still_round_trips() {
    let broken = [
        "int main(void) {",
        "}}}",
        "struct { ;;; }",
        "@ @ @",
        "int x = ;",
        "iface {",
        "impl for {",
        "new",
        "gc gc gc",
        "((((",
        "a ? b",
        "for(;;",
        "\"unterminated",
        "/* unterminated",
    ];
    for source in broken {
        let parsed = parse(source, &NoNames);
        assert_eq!(parsed.text(), source, "invariant R fails for {source:?}");
    }
}
