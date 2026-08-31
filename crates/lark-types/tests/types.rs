//! Checks type construction and the phase 3 type rules.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::{Code, LK0200, LK0210, LK0211, LK0301, LK0310, LK0340, LK0400, LK0440, LK0711};
use lark_resolve::{MemoryLoader, resolve};
use lark_syntax::SyntaxKind;
use lark_types::{Lowering, TypeStore, check_resolution};

/// Returns the type that the first declaration of a source names.
fn type_of_first_declaration(source: &str) -> String {
    let parsed = lark_syntax::parse(source, &lark_syntax::NoNames);
    let root = parsed.syntax();
    let Some(item) = root
        .descendants()
        .find(|node| matches!(node.kind(), SyntaxKind::DECLARATION | SyntaxKind::FN_DEF))
    else {
        panic!("{source:?} holds no declaration");
    };
    let Some(specifiers) = item
        .children()
        .find(|child| child.kind() == SyntaxKind::DECL_SPECIFIERS)
    else {
        panic!("{source:?} holds no specifiers");
    };

    let mut store = TypeStore::new();
    let common = store.common();
    let mut lowering = Lowering {
        store: &mut store,
        common,
    };
    let info = lowering.specifiers(&specifiers);

    let declarator = item
        .children()
        .find(|child| child.kind() == SyntaxKind::DECLARATOR)
        .or_else(|| {
            item.children()
                .find(|child| child.kind() == SyntaxKind::INIT_DECLARATOR)
                .and_then(|init| {
                    init.children()
                        .find(|child| child.kind() == SyntaxKind::DECLARATOR)
                })
        });

    let built = match declarator {
        Some(node) => lowering.declarator(&info, &node).0,
        None => info.base,
    };
    lowering.store.display(built)
}

/// Returns every diagnostic code that the type checks produce for a source.
fn codes(source: &str) -> Vec<Code> {
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    check_resolution(&resolution)
        .items()
        .iter()
        .map(|item| item.code)
        .collect()
}

/// Runs the monomorphization pass and returns the codes it reported.
///
/// Rule G-11 needs the instantiation, which only that pass produces.
fn instantiation_codes(source: &str) -> Vec<Code> {
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    let mut out = lark_diag::Diagnostics::new();
    let _ = lark_mono::collect(&resolution.graph, &mut out);
    out.items().iter().map(|item| item.code).collect()
}

// -- the C type system -----------------------------------------------------

#[test]
fn a_keyword_run_builds_the_c_type_it_names() {
    assert_eq!(type_of_first_declaration("int x;"), "int");
    assert_eq!(type_of_first_declaration("unsigned int x;"), "unsigned int");
    assert_eq!(type_of_first_declaration("unsigned x;"), "unsigned int");
    assert_eq!(type_of_first_declaration("long x;"), "long");
    assert_eq!(type_of_first_declaration("long long int x;"), "long long");
    assert_eq!(
        type_of_first_declaration("unsigned long x;"),
        "unsigned long"
    );
    assert_eq!(type_of_first_declaration("short x;"), "short");
    assert_eq!(type_of_first_declaration("char x;"), "char");
    assert_eq!(
        type_of_first_declaration("unsigned char x;"),
        "unsigned char"
    );
    assert_eq!(type_of_first_declaration("_Bool x;"), "_Bool");
    assert_eq!(type_of_first_declaration("float x;"), "float");
    assert_eq!(type_of_first_declaration("double x;"), "double");
    assert_eq!(type_of_first_declaration("long double x;"), "long double");
    assert_eq!(type_of_first_declaration("void f(void);"), "void()");
}

#[test]
fn a_pointer_and_an_array_bind_in_the_c_order() {
    // `[]` binds tighter than `*`, so this is an array of pointers.
    assert_eq!(type_of_first_declaration("int *p[10];"), "int*[10]");
    // The parentheses make this a pointer to an array.
    assert_eq!(type_of_first_declaration("int (*p)[10];"), "int[10]*");
    assert_eq!(type_of_first_declaration("int x[3][4];"), "int[4][3]");
    assert_eq!(type_of_first_declaration("char **p;"), "char**");
}

#[test]
fn a_function_type_carries_its_parameters() {
    assert_eq!(
        type_of_first_declaration("int f(char a, long b);"),
        "int(char, long)"
    );
    assert_eq!(type_of_first_declaration("int f(void);"), "int()");
    assert_eq!(
        type_of_first_declaration("int printf(const char* f, ...);"),
        "int(char*, ...)"
    );
    // A pointer to a function returning int.
    assert_eq!(type_of_first_declaration("int (*f)(void);"), "int()*");
}

#[test]
fn an_array_parameter_decays_to_a_pointer() {
    assert_eq!(
        type_of_first_declaration("void f(int a[10]);"),
        "void(int*)"
    );
}

#[test]
fn a_record_and_a_type_name_build_a_named_type() {
    assert_eq!(
        type_of_first_declaration("struct Point { int x; } p;"),
        "struct Point"
    );
    assert_eq!(
        type_of_first_declaration("union U { int x; } u;"),
        "union U"
    );
    assert_eq!(type_of_first_declaration("enum E { A } e;"), "enum E");
    assert_eq!(type_of_first_declaration("Person p;"), "Person");
}

// -- rule T-1a, the gc qualifier -------------------------------------------

/// covers: T-1, T-1a
#[test]
fn gc_in_the_specifiers_marks_the_outermost_pointer() {
    assert_eq!(type_of_first_declaration("gc char* name;"), "gc char*");
    assert_eq!(type_of_first_declaration("gc Person* p;"), "gc Person*");
}

/// covers: T-1a
#[test]
fn a_second_gc_marks_the_next_level_in() {
    assert_eq!(type_of_first_declaration("gc T** p;"), "gc T**");
    assert_eq!(type_of_first_declaration("gc gc T** p;"), "gc gc T**");
}

/// covers: T-1a
#[test]
fn gc_after_a_star_marks_that_pointer() {
    assert_eq!(type_of_first_declaration("T* gc p;"), "gc T*");
}

/// covers: T-3
#[test]
fn a_generic_use_carries_its_arguments() {
    assert_eq!(
        type_of_first_declaration("gc Data<int>* count;"),
        "gc Data<int>*"
    );
}

// -- rule T-2 --------------------------------------------------------------

/// covers: T-2
#[test]
fn gc_on_a_type_that_is_not_a_pointer_reports_lk0200() {
    assert!(codes("gc int x;").contains(&LK0200));
    assert!(codes("void f(void) { gc int x; }").contains(&LK0200));
}

/// covers: T-2
#[test]
fn gc_on_a_pointer_is_quiet() {
    assert!(!codes("gc int* x;").contains(&LK0200));
    assert!(!codes("gc gc int** x;").contains(&LK0200));
}

/// covers: T-2
#[test]
fn more_gc_markers_than_pointer_levels_reports_lk0200() {
    assert!(codes("gc gc int* x;").contains(&LK0200));
}

// -- rules T-9 and T-11, auto ----------------------------------------------

/// covers: T-9
#[test]
fn auto_with_no_initializer_reports_lk0210() {
    assert!(codes("void f(void) { auto x; }").contains(&LK0210));
}

/// covers: T-9
#[test]
fn auto_with_an_initializer_is_quiet() {
    assert!(!codes("void f(void) { auto x = 5; }").contains(&LK0210));
}

/// covers: L-5
#[test]
fn auto_before_a_type_is_the_c_storage_class_and_needs_no_initializer() {
    let reported = codes("void f(void) { auto int x; }");
    assert!(!reported.contains(&LK0210), "{reported:?}");
    assert!(!reported.contains(&LK0211), "{reported:?}");
}

/// covers: T-11
#[test]
fn auto_at_file_scope_reports_lk0211() {
    assert!(codes("auto x = 5;").contains(&LK0211));
}

/// covers: T-11
#[test]
fn auto_in_a_global_block_is_quiet() {
    let reported = codes("@global main_globals { auto x = 5; }");
    assert!(!reported.contains(&LK0211), "{reported:?}");
}

/// covers: T-11
#[test]
fn auto_on_a_parameter_reports_lk0211() {
    assert!(codes("void f(auto x);").contains(&LK0211));
}

/// covers: T-11
#[test]
fn auto_on_a_struct_member_reports_lk0211() {
    assert!(codes("struct S { auto x; };").contains(&LK0211));
}

/// covers: DQ-4
#[test]
fn a_type_check_is_quiet_inside_a_syntax_error() {
    // The generic list has no closing angle, so the parser reports first and
    // the type checks stay silent.
    let reported = codes("gc Data<int broken;");
    assert!(!reported.contains(&LK0200), "{reported:?}");
}

// -- rule T-10, what auto infers -------------------------------------------

/// Returns the type that `auto` infers from the first initializer in a source.
fn inferred_type(source: &str) -> String {
    let parsed = lark_syntax::parse(source, &lark_syntax::NoNames);
    let root = parsed.syntax();
    let Some(init) = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::INIT_DECLARATOR)
    else {
        panic!("{source:?} holds no initialized declarator");
    };
    let Some(value) = init.children().find(|child| {
        !matches!(
            child.kind(),
            SyntaxKind::DECLARATOR | SyntaxKind::DECL_SPECIFIERS
        )
    }) else {
        panic!("{source:?} holds no initializer");
    };

    let mut store = TypeStore::new();
    let common = store.common();
    let mut infer = lark_types::Infer {
        lowering: Lowering {
            store: &mut store,
            common,
        },
    };
    let built = infer.inferred(&value);
    infer.lowering.store.display(built)
}

/// covers: T-10
#[test]
fn auto_infers_the_type_of_a_literal() {
    assert_eq!(inferred_type("void f(void) { auto x = 5; }"), "int");
    assert_eq!(
        inferred_type("void f(void) { auto x = 5u; }"),
        "unsigned int"
    );
    assert_eq!(inferred_type("void f(void) { auto x = 5L; }"), "long");
    assert_eq!(inferred_type("void f(void) { auto x = 1.5; }"), "double");
    assert_eq!(inferred_type("void f(void) { auto x = 1.5f; }"), "float");
    assert_eq!(inferred_type("void f(void) { auto x = 'c'; }"), "int");
}

/// covers: T-10
#[test]
fn auto_decays_an_array_to_a_pointer() {
    // A string literal is an array of char, and rule T-10 decays it.
    assert_eq!(
        inferred_type("void f(void) { auto s = \"hello\"; }"),
        "char*"
    );
}

/// covers: T-10
#[test]
fn auto_keeps_the_gc_qualifier() {
    // Rule T-10 drops a top level qualifier, and keeps `gc`. Without that,
    // the declaration would cross the managed boundary in silence.
    assert_eq!(
        inferred_type("void f(void) { auto p = new Person { .age = 1 }; }"),
        "gc Person*"
    );
    assert_eq!(
        inferred_type("void f(void) { auto p = new char[16]; }"),
        "gc char*"
    );
    assert_eq!(
        inferred_type("void f(void) { auto p = (gc Person*)q; }"),
        "gc Person*"
    );
}

/// covers: T-10
#[test]
fn auto_takes_the_type_of_an_arithmetic_expression() {
    assert_eq!(inferred_type("void f(void) { auto x = 1 + 2L; }"), "long");
    assert_eq!(
        inferred_type("void f(void) { auto x = 1 + 2.5; }"),
        "double"
    );
    assert_eq!(inferred_type("void f(void) { auto x = 1 < 2; }"), "int");
    assert_eq!(
        inferred_type("void f(void) { auto x = sizeof(int); }"),
        "unsigned long"
    );
}

/// covers: T-10
#[test]
fn auto_reads_through_a_cast_and_a_dereference() {
    assert_eq!(
        inferred_type("void f(void) { auto x = (char*)p; }"),
        "char*"
    );
    assert_eq!(
        inferred_type("void f(void) { auto x = *(char*)p; }"),
        "char"
    );
    assert_eq!(
        inferred_type("void f(void) { auto x = &*(char*)p; }"),
        "char*"
    );
}

// -- the reference example -------------------------------------------------

/// The tour example must type check with no diagnostic.
#[test]
fn the_tour_example_type_checks_with_no_problem() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest.join("../.."));
    let examples = root.join("examples");
    let path = examples.join("tour.lark");

    let Ok(resolution) = lark_resolve::resolve_path(&path, &[examples]) else {
        panic!("cannot read {}", path.display());
    };
    let reported = check_resolution(&resolution);
    let names: Vec<String> = reported
        .items()
        .iter()
        .map(|item| format!("{} {}", item.code, item.message))
        .collect();
    assert!(
        names.is_empty(),
        "the tour must type check cleanly: {names:?}"
    );
}

// ---------------------------------------------------------------------------
// Interfaces, `Self`, and the managed boundary.
// ---------------------------------------------------------------------------

/// An interface value carries a managed pointer, so the placement rules of
/// chapter 03 apply to it exactly as they apply to a `gc T*`.
/// covers: T-13, O-24
#[test]
fn an_interface_value_cannot_live_in_unmanaged_memory() {
    let found = codes(
        "managed struct Person { gc char* name; }\n\
         iface Greet { void say_hi(Self this); }\n\
         impl Greet for Person { void say_hi(Person this) { return; } }\n\
         static Greet global_value;\n",
    );
    assert!(found.contains(&LK0310), "expected LK0310, got {found:?}");
}

/// Inside an `iface`, `Self` names the implementing type. Outside one it is an
/// ordinary identifier that a program can use for anything.
/// covers: T-15, T-16
#[test]
fn self_is_a_type_inside_an_interface_and_a_name_outside_one() {
    // Inside the declaration, `Self` stands where a type stands.
    let inside = codes(
        "managed struct Person { int age; }\n\
         iface Greet { void say_hi(Self this); }\n\
         impl Greet for Person { void say_hi(Person this) { return; } }\n",
    );
    assert!(inside.is_empty(), "a valid interface reported {inside:?}");

    // Outside one, the word is a name like any other. Rule S-2 adds no
    // reserved word, so a C program that uses it keeps its meaning.
    let outside = codes("int Self = 3;\nint f(void) { return Self; }\n");
    assert!(outside.is_empty(), "an ordinary name reported {outside:?}");
}

/// A struct with a `gc` field must carry the `managed` marker, because the
/// collector needs a field map for it.
/// covers: T-4, O-2
#[test]
fn a_struct_with_a_managed_field_needs_the_marker() {
    let found = codes("struct Holder { gc char* text; }\n");
    assert!(found.contains(&LK0400), "expected LK0400, got {found:?}");

    let marked = codes("managed struct Holder { gc char* text; }\n");
    assert!(marked.is_empty(), "a marked struct reported {marked:?}");
}

/// A generic struct is `managed` only when its declaration says so, whatever
/// the type argument turns out to be. The instantiation decides whether the
/// object holds a managed field, so the monomorphizer reports rule G-11.
/// covers: G-9, G-11
#[test]
fn a_generic_struct_needs_the_marker_when_an_instance_holds_a_managed_field() {
    // The declaration says nothing, and the instantiation puts a `gc` field
    // inside it.
    let found = instantiation_codes(
        "managed struct Person { int age; }\n\
         struct Box<T> { T item; }\n\
         init int main(void) { Box<gc Person*> b; return 0; }\n",
    );
    assert!(found.contains(&LK0400), "expected LK0400, got {found:?}");

    // A plain instantiation of the same declaration is fine. Rule G-9 keeps
    // the declaration unmarked whatever the argument is.
    let plain = instantiation_codes(
        "struct Box<T> { T item; }\n\
         init int main(void) { Box<int> b; b.item = 1; return b.item - 1; }\n",
    );
    assert!(plain.is_empty(), "a plain instance reported {plain:?}");

    // A parameter that no field uses puts nothing in the object.
    let method_only = instantiation_codes(
        "managed struct Person { int age; }\n\
         struct Holder<T> { int count; }\n\
         init int main(void) { Holder<gc Person*> h; h.count = 1; return h.count - 1; }\n",
    );
    assert!(
        method_only.is_empty(),
        "an unused parameter reported {method_only:?}"
    );
}

/// A `gc_leaf` function must not take a managed parameter, because a
/// collection cannot run while it holds one.
/// covers: C-8, M-22
#[test]
fn a_leaf_function_takes_no_managed_parameter() {
    let found = codes(
        "managed struct Person { int age; }\n\
         gc_leaf void touch(gc Person* p);\n",
    );
    assert!(found.contains(&LK0340), "expected LK0340, got {found:?}");
}

/// A `gc T*` converts to an interface value `I` when `T` implements `I`. The
/// reverse direction needs a cast, because an interface value can hold any
/// implementing type.
/// covers: T-14
#[test]
fn a_managed_pointer_converts_to_an_interface_value() {
    let forward = codes(
        "managed struct Person { int age; }\n\
         iface Greet { void say_hi(Self this); }\n\
         impl Greet for Person { void say_hi(Person this) { return; } }\n\
         init int main(void) { auto p = new Person { .age = 1 }; Greet g = p; g.say_hi(); return 0; }\n",
    );
    assert!(
        forward.is_empty(),
        "a valid conversion reported {forward:?}"
    );
}

/// A cast that adds `gc` is an assertion by the programmer, so the checker
/// accepts it. Without the cast the conversion is an error.
/// covers: T-7, T-5
#[test]
fn a_cast_that_adds_gc_is_accepted_and_the_bare_assignment_is_not() {
    // Rule T-5. No implicit conversion between a managed and a raw pointer.
    let implicit = codes(
        "managed struct Person { int age; }\n\
         int f(void* raw) { gc Person* p = raw; return p->age; }\n",
    );
    assert!(
        implicit.contains(&LK0301),
        "expected LK0301, got {implicit:?}"
    );

    // Rule T-7. The cast states that the address is a live managed object.
    let cast = codes(
        "managed struct Person { int age; }\n\
         int f(void* raw) { gc Person* p = (gc Person*) raw; return p->age; }\n",
    );
    assert!(cast.is_empty(), "a cast reported {cast:?}");
}

/// An `impl` carries no `export` marker of its own. It is exported with its
/// type, because a type without its methods is not usable.
/// covers: N-9
#[test]
fn an_impl_is_exported_with_its_type() {
    let found = codes(
        "export managed struct Person { int age; }\n\
         export iface Greet { void say_hi(Self this); }\n\
         impl Greet for Person { void say_hi(Person this) { return; } }\n",
    );
    assert!(found.is_empty(), "an exported pair reported {found:?}");
}

/// An exported function must have a C form. An interface value and a managed
/// struct by value both have none, so C code cannot call such a function.
/// covers: C-9, C-11
#[test]
fn an_exported_signature_must_have_a_c_form() {
    let head = "export managed struct Person { int age; }\n\
                export iface Greet { void say_hi(Self this); }\n\
                impl Greet for Person { void say_hi(Person this) { return; } }\n";

    // An interface value is two words, and C has no name for the pair.
    let by_iface = codes(&format!("{head}export int f(Greet g) {{ return 1; }}\n"));
    assert!(
        by_iface.contains(&LK0440),
        "expected LK0440, got {by_iface:?}"
    );

    // A managed struct by value copies the payload and leaves the header.
    let by_value = codes(&format!(
        "{head}export int f(Person p) {{ return p.age; }}\n"
    ));
    assert!(
        by_value.contains(&LK0440),
        "expected LK0440, got {by_value:?}"
    );

    // A pointer to either one is an ordinary C pointer. Rule C-10.
    let by_pointer = codes(&format!(
        "{head}export int f(gc Person* p) {{ return p->age; }}\n"
    ));
    assert!(by_pointer.is_empty(), "a pointer reported {by_pointer:?}");

    // A function that no `export` marks is never called from C.
    let private = codes(&format!("{head}int f(Person p) {{ return p.age; }}\n"));
    assert!(
        private.is_empty(),
        "a private function reported {private:?}"
    );
}

/// An initializer that reads a global from a block which runs later gets a
/// report. The transpiler reorders nothing, so the programmer fixes the order.
/// covers: I-16, I-17
#[test]
fn an_initializer_that_reads_a_later_block_is_reported() {
    let backwards = codes(
        "@global(main, 2) late { int second = 10; }\n\
         @global(main, 1) early { int first = second + 1; }\n\
         init int main(void) { return first + second - 21; }\n",
    );
    assert!(
        backwards.contains(&LK0711),
        "expected LK0711, got {backwards:?}"
    );

    // The same two blocks in the order that rule I-13 asks for are fine.
    let forwards = codes(
        "@global(main, 1) early { int first = 10; }\n\
         @global(main, 2) late { int second = first + 1; }\n\
         init int main(void) { return first + second - 21; }\n",
    );
    assert!(forwards.is_empty(), "a correct order reported {forwards:?}");
}
