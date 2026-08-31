//! Checks the monomorphization pass.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::{Code, Diagnostics, LK0500, LK0501, LK0502};
use lark_mono::{Kind, Program, collect};
use lark_resolve::{MemoryLoader, resolve};

/// Runs the pass over one module, and returns what it found.
fn run(source: &str) -> (Program, Vec<Code>) {
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    let mut out = Diagnostics::new();
    let program = collect(&resolution.graph, &mut out);
    let codes = out.items().iter().map(|item| item.code).collect();
    (program, codes)
}

/// Returns the mangled names that one module emits, in order.
fn names(program: &Program) -> Vec<String> {
    program
        .instances_of("app")
        .iter()
        .map(|item| item.mangled.clone())
        .collect()
}

/// covers: G-1, G-5
#[test]
fn one_definition_exists_per_distinct_argument_set() {
    let (program, codes) = run("struct Data<T> { T value; }\n\
         void f(void) { Data<int> a; Data<char> b; Data<int> c; }\n");
    assert!(codes.is_empty(), "{codes:?}");
    assert_eq!(names(&program), vec!["lk_app__Data__c", "lk_app__Data__i"]);
}

/// covers: G-7
#[test]
fn two_uses_of_one_argument_set_share_a_definition() {
    let (program, _) = run("struct Data<T> { T value; }\n\
         void f(void) { Data<int> a; }\n\
         void g(void) { Data<int> b; }\n");
    assert_eq!(names(&program).len(), 1);
}

/// covers: G-3
#[test]
fn a_generic_record_and_a_generic_function_both_exist() {
    let (program, _) = run("struct Data<T> { T value; }\n\
         T* first<T>(T* items) { return items; }\n\
         void f(void) { Data<int> a; first<char>(0); }\n");
    let kinds: Vec<Kind> = program
        .instances_of("app")
        .iter()
        .map(|item| item.kind)
        .collect();
    assert!(kinds.contains(&Kind::Record));
    assert!(kinds.contains(&Kind::Function));
}

/// covers: G-5
#[test]
fn a_nested_argument_gets_its_own_instantiation() {
    let (program, codes) = run("struct Data<T> { T value; }\n\
         struct Box<T> { T value; }\n\
         void f(void) { Box<Data<int>> nested; }\n");
    assert!(codes.is_empty(), "{codes:?}");
    let found = names(&program);
    assert!(
        found.iter().any(|name| name.contains("Data__i")),
        "{found:?}"
    );
    assert!(found.iter().any(|name| name.contains("Box__")), "{found:?}");
}

/// covers: G-10
#[test]
fn a_conditionally_managed_record_reports_its_arguments() {
    let (program, _) = run("managed struct Person { gc char* name; }\n\
         managed struct Box<T> { T value; }\n\
         void f(void) { Box<int> plain; gc Box<gc Person*>* boxed; }\n");
    let instances = program.instances_of("app");
    let plain = instances.iter().find(|item| item.mangled.ends_with("__i"));
    let boxed = instances
        .iter()
        .find(|item| item.mangled.contains("G6Person"));
    assert_eq!(
        plain.map(|item| item.managed_arguments.clone()),
        Some(vec![false])
    );
    assert_eq!(
        boxed.map(|item| item.managed_arguments.clone()),
        Some(vec![true])
    );
}

/// covers: G-6a
#[test]
fn a_call_with_no_type_arguments_reports_lk0501() {
    let (_, codes) = run("T* first<T>(T* items) { return items; }\n\
         void f(int* p) { first(p); }\n");
    assert!(codes.contains(&LK0501), "{codes:?}");
}

/// covers: G-6
#[test]
fn a_call_with_an_explicit_list_is_quiet() {
    let (_, codes) = run("T* first<T>(T* items) { return items; }\n\
         void f(int* p) { first<int>(p); }\n");
    assert!(!codes.contains(&LK0501), "{codes:?}");
}

/// covers: G-2, L-7
#[test]
fn a_generic_argument_that_names_a_value_reports_lk0502() {
    let (_, codes) = run("int count;\n\
         struct Data<T> { T value; }\n\
         void f(void) { gc Data<count>* p; }\n");
    assert!(codes.contains(&LK0502), "{codes:?}");
}

/// covers: G-2
#[test]
fn a_generic_argument_that_names_a_type_is_quiet() {
    let (_, codes) = run("managed struct Person { int age; }\n\
         struct Data<T> { T value; }\n\
         void f(void) { gc Data<Person>* p; }\n");
    assert!(!codes.contains(&LK0502), "{codes:?}");
}

/// covers: G-8
#[test]
fn a_depth_limit_exists() {
    // The limit stops a chain of instantiations, so a malformed program costs
    // bounded work rather than unbounded work.
    assert_eq!(lark_mono::DEPTH_LIMIT, 32);
    let (_, codes) = run("struct Data<T> { T value; }\nvoid f(void) { Data<int> a; }\n");
    assert!(!codes.contains(&LK0500), "{codes:?}");
}
