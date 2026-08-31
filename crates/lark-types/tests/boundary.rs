//! Checks the rules that keep the managed world and the unmanaged world apart.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::{Code, LK0301, LK0310, LK0311, LK0340, LK0400};
use lark_resolve::{MemoryLoader, resolve};
use lark_types::check_resolution;

/// Returns every diagnostic code that a source produces.
fn codes(source: &str) -> Vec<Code> {
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    check_resolution(&resolution)
        .items()
        .iter()
        .map(|item| item.code)
        .collect()
}

/// Reports whether a source produces a code.
fn reports(source: &str, code: Code) -> bool {
    codes(source).contains(&code)
}

// -- rule O-2 --------------------------------------------------------------

/// covers: O-2
#[test]
fn a_struct_with_a_managed_field_needs_the_marker() {
    assert!(reports("struct Person { gc char* name; }", LK0400));
    assert!(!reports("managed struct Person { gc char* name; }", LK0400));
}

/// covers: O-2
#[test]
fn a_struct_with_no_managed_field_needs_no_marker() {
    assert!(!reports("struct Point { int x; int y; }", LK0400));
}

/// covers: O-2
#[test]
fn a_struct_that_an_implementation_targets_needs_the_marker() {
    let source = "struct Point { int x; }\n\
                  iface Greet { void say_hi(Self this); }\n\
                  impl Greet for Point { void say_hi(Point this) { } }\n";
    assert!(reports(source, LK0400));

    let marked = "managed struct Point { int x; }\n\
                  iface Greet { void say_hi(Self this); }\n\
                  impl Greet for Point { void say_hi(Point this) { } }\n";
    assert!(!reports(marked, LK0400));
}

// -- rule T-5 --------------------------------------------------------------

/// covers: T-5
#[test]
fn a_managed_argument_to_a_raw_parameter_reports_lk0301() {
    let source = "gc_leaf void handle(void* data);\n\
                  void f(void) { gc char* p; handle(p); }\n";
    assert!(reports(source, LK0301), "{:?}", codes(source));
}

/// covers: T-5, T-6
#[test]
fn an_explicit_cast_crosses_the_boundary() {
    let source = "gc_leaf void handle(void* data);\n\
                  void f(void) { gc char* p; handle((void*)p); }\n";
    assert!(!reports(source, LK0301), "{:?}", codes(source));
}

/// covers: T-5
#[test]
fn a_raw_argument_to_a_managed_parameter_reports_lk0301() {
    let source = "gc_safe void keep(gc void* data);\n\
                  void f(void) { void* p; keep(p); }\n";
    assert!(reports(source, LK0301), "{:?}", codes(source));
}

/// covers: T-6
#[test]
fn a_cast_into_the_managed_world_is_quiet() {
    let source = "gc_safe void keep(gc void* data);\n\
                  void f(void) { void* p; keep((gc void*)p); }\n";
    assert!(!reports(source, LK0301), "{:?}", codes(source));
}

/// covers: T-5
#[test]
fn a_raw_initializer_for_a_managed_declaration_reports_lk0301() {
    let source = "void f(void) { void* raw; gc char* p = raw; }\n";
    assert!(reports(source, LK0301), "{:?}", codes(source));
}

/// covers: O-4, T-10
#[test]
fn a_new_expression_satisfies_a_managed_parameter() {
    let source = "managed struct Person { gc char* name; }\n\
                  gc_safe void keep(gc void* data);\n\
                  void f(void) { keep(new Person { .name = \"x\" }); }\n";
    assert!(!reports(source, LK0301), "{:?}", codes(source));
}

/// covers: T-5
#[test]
fn a_new_expression_for_a_raw_parameter_reports_lk0301() {
    let source = "managed struct Person { gc char* name; }\n\
                  gc_leaf void handle(void* data);\n\
                  void f(void) { handle(new Person { .name = \"x\" }); }\n";
    assert!(reports(source, LK0301), "{:?}", codes(source));
}

/// covers: T-8
#[test]
fn a_string_literal_needs_no_cast() {
    let source = "managed struct Person { gc char* name; }\n\
                  void f(void) { gc char* n = \"hello\"; }\n";
    assert!(!reports(source, LK0301), "{:?}", codes(source));
}

// -- rule M-2 --------------------------------------------------------------

/// covers: M-1, M-2
#[test]
fn a_managed_pointer_at_file_scope_reports_lk0310() {
    assert!(reports("gc char* global_name;", LK0310));
}

/// covers: M-1
#[test]
fn a_managed_pointer_in_a_global_block_is_quiet() {
    assert!(!reports("@global main_globals { gc char* name; }", LK0310));
}

/// covers: M-1
#[test]
fn a_managed_local_is_quiet() {
    assert!(!reports("void f(void) { gc char* name; }", LK0310));
}

/// covers: M-2
#[test]
fn a_managed_prototype_parameter_is_not_a_global() {
    assert!(!reports("gc_safe void keep(gc void* data);", LK0310));
}

// -- rule M-3 --------------------------------------------------------------

/// covers: M-3
#[test]
fn a_managed_struct_in_malloc_memory_reports_lk0311() {
    let source = "managed struct Person { gc char* name; }\n\
                  void* malloc(unsigned long size);\n\
                  void f(void) { void* p = malloc(sizeof(struct Person)); }\n";
    assert!(reports(source, LK0311), "{:?}", codes(source));
}

/// covers: M-3
#[test]
fn a_plain_struct_in_malloc_memory_is_quiet() {
    let source = "struct Point { int x; }\n\
                  void* malloc(unsigned long size);\n\
                  void f(void) { void* p = malloc(sizeof(struct Point)); }\n";
    assert!(!reports(source, LK0311), "{:?}", codes(source));
}

/// covers: M-3
#[test]
fn a_pointer_to_a_managed_struct_in_malloc_memory_is_quiet() {
    let source = "managed struct Person { gc char* name; }\n\
                  void* malloc(unsigned long size);\n\
                  void f(void) { void* p = malloc(sizeof(struct Person*)); }\n";
    assert!(!reports(source, LK0311), "{:?}", codes(source));
}

// -- rule M-22 -------------------------------------------------------------

/// covers: M-22
#[test]
fn a_leaf_function_with_a_managed_parameter_reports_lk0340() {
    assert!(reports("gc_leaf void handle(gc void* data);", LK0340));
}

/// covers: M-20, M-22
#[test]
fn a_leaf_function_with_a_raw_parameter_is_quiet() {
    assert!(!reports("gc_leaf void handle(void* data);", LK0340));
}

/// covers: M-19
#[test]
fn a_safe_function_can_take_a_managed_parameter() {
    assert!(!reports("gc_safe void keep(gc void* data);", LK0340));
}
