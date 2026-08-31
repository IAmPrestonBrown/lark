//! Checks the interface rules from chapter 04.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::{Code, LK0410, LK0411, LK0412, LK0413, LK0420, LK0421, LK0430};
use lark_resolve::{MemoryLoader, resolve};
use lark_types::check_resolution;

/// Returns every diagnostic code that a set of modules produces.
fn codes(modules: &[(&str, &str)]) -> Vec<Code> {
    let Some((root_name, root_text)) = modules.first() else {
        panic!("a fixture needs at least one module");
    };
    let loader = MemoryLoader::new(modules.iter().map(|(name, text)| (*name, *text)));
    let path = PathBuf::from(format!("{root_name}.lark"));
    let resolution = resolve(&loader, root_name, &path, root_text);
    check_resolution(&resolution)
        .items()
        .iter()
        .map(|item| item.code)
        .collect()
}

/// Reports whether one module produces a code.
fn reports(source: &str, code: Code) -> bool {
    codes(&[("app", source)]).contains(&code)
}

/// A well formed interface, an implementation, and the type they use.
const GOOD: &str = "managed struct Person { gc char* name; }\n\
                    iface Greet {\n\
                        void say_hi(Self this);\n\
                        void rename(gc Self* this, gc char* fresh);\n\
                    }\n\
                    impl Greet for Person {\n\
                        void say_hi(Person this) { }\n\
                        void rename(gc Person* this, gc char* fresh) { }\n\
                    }\n";

#[test]
fn a_well_formed_interface_reports_nothing() {
    let reported = codes(&[("app", GOOD)]);
    assert!(reported.is_empty(), "{reported:?}");
}

// -- rule O-12 -------------------------------------------------------------

/// covers: O-12, O-10
#[test]
fn an_interface_function_with_no_receiver_reports_lk0430() {
    let source = "iface Greet { void say_hi(void); }\n";
    assert!(reports(source, LK0430), "{:?}", codes(&[("app", source)]));
}

/// covers: O-11
#[test]
fn both_receiver_forms_are_accepted() {
    let source = "iface Greet {\n\
                      void by_value(Self this);\n\
                      void by_pointer(gc Self* this);\n\
                  }\n";
    assert!(!reports(source, LK0430), "{:?}", codes(&[("app", source)]));
}

// -- rule O-13 -------------------------------------------------------------

/// covers: O-13
#[test]
fn a_missing_function_reports_lk0410() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void say_hi(Self this); void rename(Self this); }\n\
                  impl Greet for Person { void say_hi(Person this) { } }\n";
    assert!(reports(source, LK0410), "{:?}", codes(&[("app", source)]));
}

/// covers: O-13
#[test]
fn an_extra_function_reports_lk0411() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void say_hi(Self this); }\n\
                  impl Greet for Person {\n\
                      void say_hi(Person this) { }\n\
                      void extra(Person this) { }\n\
                  }\n";
    assert!(reports(source, LK0411), "{:?}", codes(&[("app", source)]));
}

/// covers: O-13
#[test]
fn a_complete_implementation_is_quiet() {
    assert!(!reports(GOOD, LK0410));
    assert!(!reports(GOOD, LK0411));
}

// -- rule O-14 -------------------------------------------------------------

/// covers: O-14
#[test]
fn an_implementation_for_a_plain_struct_reports_lk0412() {
    let source = "struct Point { int x; }\n\
                  iface Greet { void say_hi(Self this); }\n\
                  impl Greet for Point { void say_hi(Point this) { } }\n";
    assert!(reports(source, LK0412), "{:?}", codes(&[("app", source)]));
}

/// covers: O-14
#[test]
fn an_implementation_for_a_managed_struct_is_quiet() {
    assert!(!reports(GOOD, LK0412));
}

// -- rule O-15 -------------------------------------------------------------

/// covers: O-15
#[test]
fn an_implementation_away_from_both_names_reports_lk0413() {
    let modules = [
        (
            "app",
            "@import shapes\n@import greeting\n\
             impl Greet for Point { void say_hi(Point this) { } }\n",
        ),
        ("shapes", "export managed struct Point { int x; }\n"),
        (
            "greeting",
            "export iface Greet { void say_hi(Self this); }\n",
        ),
    ];
    assert!(codes(&modules).contains(&LK0413), "{:?}", codes(&modules));
}

/// covers: O-15
#[test]
fn an_implementation_with_its_type_is_quiet() {
    let modules = [
        (
            "app",
            "@import greeting\nmanaged struct Point { int x; }\n\
             impl Greet for Point { void say_hi(Point this) { } }\n",
        ),
        (
            "greeting",
            "export iface Greet { void say_hi(Self this); }\n",
        ),
    ];
    assert!(!codes(&modules).contains(&LK0413), "{:?}", codes(&modules));
}

/// covers: O-15
#[test]
fn an_implementation_with_its_interface_is_quiet() {
    assert!(!reports(GOOD, LK0413));
}

// -- rule O-21 -------------------------------------------------------------

/// covers: O-21
#[test]
fn a_name_that_two_interfaces_declare_reports_lk0421() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void run(Self this); }\n\
                  iface Move { void run(Self this); }\n\
                  impl Greet for Person { void run(Person this) { } }\n\
                  impl Move for Person { void run(Person this) { } }\n\
                  void f(gc Person* p) { p.run(); }\n";
    assert!(reports(source, LK0421), "{:?}", codes(&[("app", source)]));
}

/// covers: O-21
#[test]
fn the_qualified_form_removes_the_ambiguity() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void run(Self this); }\n\
                  iface Move { void run(Self this); }\n\
                  impl Greet for Person { void run(Person this) { } }\n\
                  impl Move for Person { void run(Person this) { } }\n\
                  void f(gc Person* p) { p.Greet::run(); }\n";
    assert!(!reports(source, LK0421), "{:?}", codes(&[("app", source)]));
}

/// covers: O-17
#[test]
fn one_interface_needs_no_prefix() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void run(Self this); }\n\
                  impl Greet for Person { void run(Person this) { } }\n\
                  void f(gc Person* p) { p.run(); }\n";
    assert!(!reports(source, LK0421), "{:?}", codes(&[("app", source)]));
}

// -- rule O-18 -------------------------------------------------------------

/// covers: O-18
#[test]
fn a_stack_receiver_for_a_pointer_method_reports_lk0420() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void rename(gc Self* this); }\n\
                  impl Greet for Person { void rename(gc Person* this) { } }\n\
                  void f(void) { Person local; local.rename(); }\n";
    assert!(reports(source, LK0420), "{:?}", codes(&[("app", source)]));
}

/// covers: O-18
#[test]
fn a_stack_receiver_for_a_value_method_is_quiet() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void say_hi(Self this); }\n\
                  impl Greet for Person { void say_hi(Person this) { } }\n\
                  void f(void) { Person local; local.say_hi(); }\n";
    assert!(!reports(source, LK0420), "{:?}", codes(&[("app", source)]));
}

/// covers: O-18
#[test]
fn a_managed_receiver_for_a_pointer_method_is_quiet() {
    let source = "managed struct Person { int age; }\n\
                  iface Greet { void rename(gc Self* this); }\n\
                  impl Greet for Person { void rename(gc Person* this) { } }\n\
                  void f(void) { gc Person* p = new Person { .age = 1 }; p.rename(); }\n";
    assert!(!reports(source, LK0420), "{:?}", codes(&[("app", source)]));
}
