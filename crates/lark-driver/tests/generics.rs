//! Rule DQ-2 and rule G-4. An error inside a generic names the instantiation.
//!
//! A generic has no C form of its own, so an error in its body belongs to
//! every use of it. Version 1 has no constraint syntax, which makes this the
//! whole of constraint reporting: the error appears after substitution, and
//! the diagnostic says which use caused it.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::Diagnostics;
use lark_driver::generics;
use lark_resolve::{MemoryLoader, resolve};

/// Runs the passes over one module and returns the diagnostics, attributed.
fn diagnose(source: &str) -> Diagnostics {
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    let mut mono = Diagnostics::new();
    let program = lark_mono::collect(&resolution.graph, &mut mono);
    let mut found = lark_types::check_resolution(&resolution);
    generics::attribute(&mut found, &program);
    found
}

/// covers: DQ-2, G-4
#[test]
fn an_error_in_a_generic_body_names_the_instantiation() {
    let found = diagnose(
        "managed struct Person { int age; }\n\
         T bad_body<T>(T item, void* raw) { gc Person* p = raw; return item; }\n\
         init int main(void) { int a = bad_body<int>(1, 0); return a - 1; }\n",
    );
    let items = found.items();
    assert_eq!(items.len(), 1, "one problem gives one report: {items:?}");

    let report = &items[0];
    // Rule DQ-2. The error location and the instantiation location, both.
    assert!(
        !report.secondary.is_empty(),
        "the report names no instantiation"
    );
    assert!(
        report.secondary[0].text.contains("bad_body<int>"),
        "the label does not name the instantiation: {}",
        report.secondary[0].text
    );
    // Rule G-4. The note says why there was no earlier answer.
    assert!(
        report.notes.iter().any(|note| note.contains("G-4")),
        "the report does not cite rule G-4: {:?}",
        report.notes
    );
}

/// Two instantiations both appear, so a reader sees every use that fails.
/// covers: DQ-2
#[test]
fn every_instantiation_appears_up_to_the_limit() {
    let found = diagnose(
        "managed struct Person { int age; }\n\
         T bad_body<T>(T item, void* raw) { gc Person* p = raw; return item; }\n\
         init int main(void) {\n\
             int a = bad_body<int>(1, 0);\n\
             char b = bad_body<char>('x', 0);\n\
             return a - 1 + b - 'x';\n\
         }\n",
    );
    let items = found.items();
    assert_eq!(items.len(), 1, "one problem gives one report");
    assert_eq!(
        items[0].secondary.len(),
        2,
        "both instantiations must appear: {:?}",
        items[0].secondary
    );
}

/// An error outside every generic gets no instantiation label.
#[test]
fn an_error_outside_a_generic_gains_nothing() {
    let found = diagnose(
        "managed struct Person { int age; }\n\
         struct Box<T> { T item; }\n\
         int f(void* raw) { gc Person* p = raw; return p->age; }\n\
         init int main(void) { Box<int> b; b.item = 1; return f(0) - b.item; }\n",
    );
    let items = found.items();
    assert!(!items.is_empty(), "the file must report the conversion");
    for report in items {
        assert!(
            report.secondary.is_empty()
                || !report
                    .secondary
                    .iter()
                    .any(|label| label.text.contains("instantiates")),
            "an unrelated error gained an instantiation label"
        );
    }
}
