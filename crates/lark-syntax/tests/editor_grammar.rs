//! The editor grammar names the keywords that the lexer knows.
//!
//! The Visual Studio Code extension colours a file with a `TextMate` grammar,
//! which is a second list of every keyword. A second list drifts. This test
//! compares it against the lexer, which is the first one.
//!
//! covers: L-3, S-2, Z-7, Z-8

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use lark_syntax::SyntaxKind;

/// Returns the repository root, from the directory of this crate.
fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

/// Returns the text of the grammar file.
fn grammar() -> String {
    let path = repository_root().join("editors/vscode/syntaxes/lark.tmLanguage.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => panic!("cannot read {}: {error}", path.display()),
    }
}

/// Every C11 keyword that the lexer recognizes, in the plain spelling.
///
/// A spelling with two leading underscores is a compiler extension, and rule
/// C-4b maps it to the same keyword. The grammar leaves those uncoloured, so a
/// reader sees them as the extension they are.
fn lexer_keywords() -> BTreeSet<String> {
    // The list is every word that the lexer answers for. Rule C-4b maps a
    // reserved spelling to the same kind, so the plain word covers both.
    const WORDS: &[&str] = &[
        "auto",
        "break",
        "case",
        "char",
        "const",
        "continue",
        "default",
        "do",
        "double",
        "else",
        "enum",
        "extern",
        "float",
        "for",
        "goto",
        "if",
        "inline",
        "int",
        "long",
        "register",
        "restrict",
        "return",
        "short",
        "signed",
        "sizeof",
        "static",
        "struct",
        "switch",
        "typedef",
        "union",
        "unsigned",
        "void",
        "volatile",
        "while",
        "_Alignas",
        "_Alignof",
        "_Atomic",
        "_Bool",
        "_Complex",
        "_Generic",
        "_Imaginary",
        "_Noreturn",
        "_Static_assert",
        "_Thread_local",
    ];
    let mut found = BTreeSet::new();
    for word in WORDS {
        assert!(
            SyntaxKind::c_keyword(word).is_some(),
            "`{word}` is not a keyword that the lexer knows"
        );
        found.insert((*word).to_owned());
    }
    found
}

/// Rule L-3. Every Lark keyword is contextual, and the grammar colours each.
fn lark_keywords() -> BTreeSet<String> {
    [
        "managed", "export", "iface", "impl", "gc_leaf", "gc_safe", "init", "new", "gc", "Self",
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect()
}

/// Every keyword the lexer knows appears in the grammar.
#[test]
fn the_grammar_names_every_c_keyword() {
    let text = grammar();
    let mut missing = Vec::new();
    for word in lexer_keywords() {
        // The grammar writes a word inside an alternation, so a plain search
        // for the word with a boundary on each side is enough.
        if !text.contains(&format!("{word}|")) && !text.contains(&format!("{word})")) {
            missing.push(word);
        }
    }
    assert!(
        missing.is_empty(),
        "the editor grammar is missing: {missing:?}\n\
         regenerate it, or add the word by hand"
    );
}

/// Rule L-3. Every Lark keyword appears too, under a scope of its own.
#[test]
fn the_grammar_names_every_lark_keyword() {
    let text = grammar();
    let mut missing = Vec::new();
    for word in lark_keywords() {
        if !text.contains(&word) {
            missing.push(word);
        }
    }
    assert!(
        missing.is_empty(),
        "the editor grammar is missing: {missing:?}"
    );
}

/// A Lark keyword carries a scope of its own, so a reader tells it from a C
/// keyword by colour.
/// covers: S-2
#[test]
fn a_lark_keyword_has_a_scope_of_its_own() {
    let text = grammar();
    assert!(
        text.contains("keyword.other.lark.declaration"),
        "the Lark keywords share the scope of the C ones"
    );
    assert!(text.contains("keyword.other.lark.new"));
}

/// The grammar and the manifest agree on the file extension and the scope.
#[test]
fn the_manifest_and_the_grammar_agree() {
    let manifest_path = repository_root().join("editors/vscode/package.json");
    let Ok(manifest) = std::fs::read_to_string(&manifest_path) else {
        panic!("cannot read {}", manifest_path.display());
    };
    assert!(
        manifest.contains("\"source.lark\""),
        "the manifest names no scope"
    );
    assert!(
        manifest.contains("\".lark\""),
        "the manifest names no extension"
    );
    assert!(
        grammar().contains("\"scopeName\": \"source.lark\""),
        "the grammar names a different scope"
    );

    // The manifest names the two commands that the extension registers.
    let extension_path = repository_root().join("editors/vscode/src/extension.js");
    let Ok(source) = std::fs::read_to_string(&extension_path) else {
        panic!("cannot read {}", extension_path.display());
    };
    for command in ["lark.restartServer", "lark.format"] {
        assert!(manifest.contains(command), "the manifest omits {command}");
        assert!(source.contains(command), "the extension omits {command}");
    }
}

/// Every setting that the extension reads is one the manifest declares.
///
/// A setting that the manifest omits reads as undefined, and the default in
/// the code then decides. That is a difference a person cannot see.
#[test]
fn every_setting_the_extension_reads_is_declared() {
    let root = repository_root().join("editors/vscode");
    let Ok(manifest) = std::fs::read_to_string(root.join("package.json")) else {
        panic!("cannot read the manifest");
    };
    let Ok(source) = std::fs::read_to_string(root.join("src/extension.js")) else {
        panic!("cannot read the extension");
    };

    let mut missing = Vec::new();
    for line in source.lines() {
        let Some(rest) = line.split("setting(\"").nth(1) else {
            continue;
        };
        let Some(key) = rest.split('"').next() else {
            continue;
        };
        if !manifest.contains(&format!("lark.{key}")) {
            missing.push(key.to_owned());
        }
    }
    assert!(
        missing.is_empty(),
        "the extension reads settings that the manifest does not declare: {missing:?}"
    );
}
