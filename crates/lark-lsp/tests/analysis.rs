//! Checks what the language server answers.
//!
//! Every case works on code that does not parse, because the parser recovers
//! and always produces a tree.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_lsp::{Analysis, CompletionKind, Query};

/// The marker that a case puts at the cursor.
const CURSOR: &str = "<|>";

/// Builds an analysis from text with a cursor marker.
fn at(source: &str) -> (Analysis, u32) {
    let Some(offset) = source.find(CURSOR) else {
        panic!("a case needs the `{CURSOR}` marker");
    };
    let text = format!("{}{}", &source[..offset], &source[offset + CURSOR.len()..]);
    let path = PathBuf::from("app.lark");
    let analysis = Analysis::new("app", &path, &text, &[]);
    (analysis, u32::try_from(offset).unwrap_or(0))
}

/// Returns the completion labels of one kind.
fn labels(analysis: &Analysis, offset: u32, kind: CompletionKind) -> Vec<String> {
    analysis
        .completions(offset)
        .into_iter()
        .filter(|item| item.kind == kind)
        .map(|item| item.label)
        .collect()
}

const PERSON: &str = "managed struct Person {\n\
                          gc char* name;\n\
                          int age;\n\
                      }\n\
                      iface Greet {\n\
                          void say_hi(Self this);\n\
                      }\n\
                      impl Greet for Person {\n\
                          void say_hi(Person this) { }\n\
                      }\n";

/// covers: O-17
#[test]
fn a_dot_offers_the_fields_and_the_methods_of_the_receiver() {
    let source = format!(
        "{PERSON}init int main(void) {{\n    gc Person* p = new Person {{ .age = 1 }};\n    p.<|>\n}}\n"
    );
    let (analysis, offset) = at(&source);
    // The list sorts by kind and then by label, so a snapshot stays stable.
    assert_eq!(
        labels(&analysis, offset, CompletionKind::Field),
        vec!["age", "name"]
    );
    assert_eq!(
        labels(&analysis, offset, CompletionKind::Method),
        vec!["say_hi"]
    );
}

/// covers: T-10
#[test]
fn a_dot_works_on_an_auto_local() {
    let source = format!(
        "{PERSON}init int main(void) {{\n    auto p = new Person {{ .age = 1 }};\n    p.<|>\n}}\n"
    );
    let (analysis, offset) = at(&source);
    assert!(labels(&analysis, offset, CompletionKind::Method).contains(&"say_hi".to_owned()));
}

/// covers: L-13
#[test]
fn a_dot_works_on_code_that_does_not_parse() {
    let source = format!(
        "{PERSON}init int main(void) {{\n    auto p = new Person {{ .age = 1 }};\n\
         \x20   int x = ;\n    if (\n    p.<|>\n"
    );
    let (analysis, offset) = at(&source);
    assert!(labels(&analysis, offset, CompletionKind::Field).contains(&"name".to_owned()));
}

#[test]
fn a_bare_position_offers_the_module_and_the_locals() {
    let source = format!(
        "{PERSON}init int main(void) {{\n    gc Person* p = new Person {{ .age = 1 }};\n    <|>\n}}\n"
    );
    let (analysis, offset) = at(&source);
    assert!(labels(&analysis, offset, CompletionKind::Local).contains(&"p".to_owned()));
    assert!(labels(&analysis, offset, CompletionKind::Type).contains(&"Person".to_owned()));
    assert!(labels(&analysis, offset, CompletionKind::Interface).contains(&"Greet".to_owned()));
    assert!(labels(&analysis, offset, CompletionKind::Keyword).contains(&"managed".to_owned()));
}

/// covers: L-16
#[test]
fn a_local_that_comes_later_is_not_in_scope() {
    let source = "int main(void) {\n    int early;\n    <|>\n    int late;\n    return 0;\n}\n";
    let (analysis, offset) = at(source);
    let found = labels(&analysis, offset, CompletionKind::Local);
    assert!(found.contains(&"early".to_owned()), "{found:?}");
    assert!(!found.contains(&"late".to_owned()), "{found:?}");
}

/// covers: T-10
#[test]
fn hover_says_what_a_local_is() {
    let source = format!(
        "{PERSON}init int main(void) {{\n    auto p = new Person {{ .age = 1 }};\n    p<|>;\n}}\n"
    );
    let (analysis, offset) = at(&source);
    let Some(hover) = analysis.hover(offset) else {
        panic!("the cursor sits on a local");
    };
    assert_eq!(hover.kind, CompletionKind::Local);
    assert_eq!(hover.detail, "gc Person*");
}

#[test]
fn hover_says_what_a_module_symbol_is() {
    let source =
        "int helper(int value) { return value; }\nint main(void) { return helper<|>(1); }\n";
    let (analysis, offset) = at(source);
    let Some(hover) = analysis.hover(offset) else {
        panic!("the cursor sits on a function");
    };
    assert_eq!(hover.kind, CompletionKind::Function);
}

#[test]
fn definition_finds_a_module_symbol() {
    let source =
        "int helper(int value) { return value; }\nint main(void) { return helper<|>(1); }\n";
    let (analysis, offset) = at(source);
    let Some(location) = analysis.definition(offset) else {
        panic!("the cursor sits on a function");
    };
    // `helper` starts at offset 4 of the first line.
    assert_eq!(location.span.start, 4);
}

/// covers: L-16
#[test]
fn definition_finds_a_local() {
    let source = "int main(void) {\n    int total = 1;\n    return total<|>;\n}\n";
    let (analysis, offset) = at(source);
    let Some(location) = analysis.definition(offset) else {
        panic!("the cursor sits on a local");
    };
    assert_eq!(location.span.start, 25);
}

#[test]
fn a_position_on_nothing_answers_nothing() {
    let source = "int main(void) {\n    return 0;<|>\n}\n";
    let (analysis, offset) = at(source);
    assert!(analysis.hover(offset).is_none());
    assert!(analysis.definition(offset).is_none());
}

#[test]
fn diagnostics_reach_the_server() {
    let source = "void f(void) {\n    gc int broken;<|>\n}\n";
    let (analysis, _) = at(source);
    let codes: Vec<String> = analysis
        .diagnostics()
        .items()
        .iter()
        .map(|item| item.code.to_string())
        .collect();
    assert!(codes.contains(&"LK0200".to_owned()), "{codes:?}");
}

#[test]
fn a_report_renders_every_query() {
    let source =
        "int helper(int value) { return value; }\nint main(void) { return helper<|>(1); }\n";
    let (analysis, offset) = at(source);
    assert!(
        analysis
            .report(Query::Hover, offset)
            .starts_with("fn helper")
    );
    assert!(
        analysis
            .report(Query::Definition, offset)
            .starts_with("app.lark:")
    );
    assert!(
        analysis
            .report(Query::Completion, offset)
            .contains("helper")
    );
}
