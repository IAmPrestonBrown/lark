//! What the formatter must never do.
//!
//! A formatter that changes a token changes the program. A formatter that does
//! not settle makes every save a change. Both are tested here over every file
//! that the project holds, not over one example.
//!
//! covers: Z-1, Z-2, Z-3, Z-4

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};

use lark_syntax::{SyntaxKind, tokenize};

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
        if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| extensions.contains(&value))
        {
            found.push(path);
        }
    }
}

/// Returns every Lark and C file that the repository holds.
fn corpus() -> Vec<PathBuf> {
    let root = repository_root();
    let mut found = Vec::new();
    for directory in ["tests", "examples", "runtime"] {
        files_under(&root.join(directory), &["lark", "c", "h"], &mut found);
    }
    found.sort();
    found
}

/// Returns the tokens of a file that are not whitespace or a comment.
///
/// A comment is left out, because the formatter moves one to its own line and
/// trims the space at its end. The text of every other token must survive.
fn code_tokens(source: &str) -> Vec<(SyntaxKind, String)> {
    tokenize(source)
        .tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
            )
        })
        .map(|token| {
            (
                token.kind,
                source[token.span.start as usize..token.span.end as usize].to_owned(),
            )
        })
        .collect()
}

/// Rule Z-2. Formatting changes no token, so the program means the same thing.
#[test]
fn formatting_changes_no_token() {
    let mut checked = 0;
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let formatted = lark_fmt::format(&source);
        assert_eq!(
            code_tokens(&formatted),
            code_tokens(&source),
            "the tokens changed in {}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 40, "only {checked} files were read");
}

/// Rule Z-3. Formatting twice equals formatting once.
#[test]
fn formatting_settles_after_one_pass() {
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let once = lark_fmt::format(&source);
        let twice = lark_fmt::format(&once);
        assert_eq!(
            once,
            twice,
            "the layout did not settle in {}",
            path.display()
        );
    }
}

/// Rule Z-1. The output has no trailing space and ends in one newline.
#[test]
fn the_output_is_tidy() {
    for path in corpus() {
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let formatted = lark_fmt::format(&source);
        for (number, line) in formatted.lines().enumerate() {
            assert!(
                !line.ends_with(' ') && !line.contains('\t'),
                "{}:{} has trailing space or a tab",
                path.display(),
                number + 1
            );
        }
        assert!(
            formatted.ends_with('\n') && !formatted.ends_with("\n\n"),
            "{} does not end in exactly one newline",
            path.display()
        );
        // At most one blank line anywhere.
        assert!(
            !formatted.contains("\n\n\n"),
            "{} holds two blank lines in a row",
            path.display()
        );
    }
}

/// Rule Z-4. A file that does not parse still formats, and keeps its tokens.
#[test]
fn a_file_that_does_not_parse_still_formats() {
    let broken = [
        "int f(void) { return",
        "managed struct P { gc P* next;",
        "if (",
        "auto p = new Person { .name = ",
        "}}}",
        "int x = ;",
    ];
    for source in broken {
        let formatted = lark_fmt::format(source);
        assert_eq!(
            code_tokens(&formatted),
            code_tokens(source),
            "the tokens changed in {source:?}"
        );
        // It settles too.
        assert_eq!(lark_fmt::format(&formatted), formatted);
    }
}

/// Every prefix of a program is what an editor holds while a person types.
/// covers: Z-4
#[test]
fn every_prefix_of_a_program_formats() {
    let program = "\
managed struct Person { gc char* name; int age; }
init int main(void) {
    auto p = new Person { .name = \"Ada\", .age = 36 };
    for (int i = 0; i < 3; i++) { p.age = p.age + 1; }
    return 0;
}
";
    for end in 0..program.len() {
        if !program.is_char_boundary(end) {
            continue;
        }
        let source = &program[..end];
        let formatted = lark_fmt::format(source);
        assert_eq!(
            code_tokens(&formatted),
            code_tokens(source),
            "the tokens changed at length {end}"
        );
    }
}

/// A file that is already formatted reports as formatted.
#[test]
fn an_already_formatted_file_needs_no_change() {
    let source = "int main(void) {\n    return 0;\n}\n";
    assert!(
        lark_fmt::is_formatted(source),
        "{:?}",
        lark_fmt::format(source)
    );
    assert!(!lark_fmt::is_formatted("int main(void){return 0;}"));
}
