//! Checks the emitted C against the rules in chapter 09.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_codegen::{Emitted, Options};
use lark_resolve::{MemoryLoader, resolve};

/// Emits the first module of a set, with the first one as the root.
fn emit_modules(modules: &[(&str, &str)]) -> Emitted {
    let Some((root_name, root_text)) = modules.first() else {
        panic!("a fixture needs at least one module");
    };
    let loader = MemoryLoader::new(modules.iter().map(|(name, text)| (*name, *text)));
    let path = PathBuf::from(format!("{root_name}.lark"));
    let resolution = resolve(&loader, root_name, &path, root_text);

    let options = Options {
        source_name: Some(format!("{root_name}.lark")),
        ..Options::default()
    };
    let Some(root) = resolution.root else {
        panic!("the root module must exist");
    };
    let mut diagnostics = lark_diag::Diagnostics::new();
    let program = lark_mono::collect(&resolution.graph, &mut diagnostics);
    match lark_codegen::emit(&resolution.graph, root, &options, &program) {
        Some(emitted) => emitted,
        None => panic!("the emitter must produce output"),
    }
}

/// Emits one module.
fn emit(source: &str) -> Emitted {
    emit_modules(&[("app", source)])
}

/// covers: X-2
#[test]
fn the_output_keeps_the_comments_and_the_formatting() {
    let emitted = emit("// a note\nint  main( void )  {\n    return   0;\n}\n");
    assert!(emitted.c.contains("// a note"), "{}", emitted.c);
    assert!(emitted.c.contains("int  main( void )  {"), "{}", emitted.c);
    assert!(emitted.c.contains("    return   0;"), "{}", emitted.c);
}

/// covers: X-3
#[test]
fn every_item_carries_a_line_directive() {
    let emitted = emit("int a;\nint b;\nint c;\n");
    assert_eq!(emitted.line_map.len(), 3);
    assert_eq!(emitted.line_map[0].source, 1);
    assert_eq!(emitted.line_map[1].source, 2);
    assert_eq!(emitted.line_map[2].source, 3);
    assert!(emitted.c.contains("#line 1 \"app.lark\""), "{}", emitted.c);
}

/// covers: X-5
#[test]
fn a_name_that_the_programmer_wrote_reaches_the_output_unchanged() {
    let emitted = emit("export int compute_total(int a) { return a; }\n");
    assert!(
        emitted.c.contains("int compute_total(int a)"),
        "{}",
        emitted.c
    );
    assert!(
        emitted.header.contains("int compute_total(int a);"),
        "{}",
        emitted.header
    );
}

/// covers: X-5b
#[test]
fn a_private_definition_becomes_static() {
    let emitted = emit("int helper(void) { return 1; }\nint counter = 0;\n");
    assert!(
        emitted.c.contains("static int helper(void)"),
        "{}",
        emitted.c
    );
    assert!(
        emitted.c.contains("static int counter = 0;"),
        "{}",
        emitted.c
    );
}

/// covers: X-5b
#[test]
fn an_exported_definition_stays_external() {
    let emitted = emit("export int helper(void) { return 1; }\n");
    assert!(!emitted.c.contains("static int helper"), "{}", emitted.c);
}

/// covers: X-5b
#[test]
fn the_entry_point_never_becomes_static() {
    let emitted = emit("int main(void) { return 0; }\n");
    assert!(!emitted.c.contains("static int main"), "{}", emitted.c);
}

/// covers: X-5b
#[test]
fn a_prototype_never_becomes_static() {
    let emitted = emit("int elsewhere(void);\nextern int shared;\n");
    assert!(!emitted.c.contains("static int elsewhere"), "{}", emitted.c);
    assert!(!emitted.c.contains("static extern"), "{}", emitted.c);
}

/// covers: X-4, X-4a
#[test]
fn an_exported_type_lives_only_in_the_header() {
    let emitted = emit("export struct Point { int x; }\n");
    assert!(
        emitted.header.contains("struct Point {"),
        "{}",
        emitted.header
    );
    assert!(!emitted.c.contains("struct Point {"), "{}", emitted.c);
    // Rule X-4b names the generated header.
    assert!(
        emitted.c.contains("Point is in app.lark.h"),
        "{}",
        emitted.c
    );
}

/// covers: X-4a
#[test]
fn an_exported_variable_gets_an_extern_declaration() {
    let emitted = emit("export int total = 7;\n");
    assert!(
        emitted.header.contains("extern int total;"),
        "{}",
        emitted.header
    );
    assert!(!emitted.header.contains("= 7"), "{}", emitted.header);
    assert!(emitted.c.contains("int total = 7;"), "{}", emitted.c);
}

/// covers: X-4
#[test]
fn a_private_declaration_stays_out_of_the_header() {
    let emitted = emit("struct Hidden { int x; }\nint secret(void) { return 0; }\n");
    // The layout and the function both stay in the module.
    assert!(
        !emitted.header.contains("struct Hidden {"),
        "{}",
        emitted.header
    );
    assert!(!emitted.header.contains("secret"), "{}", emitted.header);
}

/// covers: N-2, X-5
#[test]
fn an_import_becomes_an_include_and_a_path_keeps_its_name() {
    let emitted = emit_modules(&[
        (
            "app",
            "@import stdio\nint main(void) { return stdio::value(); }\n",
        ),
        ("stdio", "export int value(void);\n"),
    ]);
    // Rule X-4b. An import includes the generated header, not a header
    // that a programmer wrote under the same stem.
    assert!(
        emitted.c.contains("#include \"stdio.lark.h\""),
        "{}",
        emitted.c
    );
    assert!(emitted.c.contains("return value();"), "{}", emitted.c);
    assert!(!emitted.c.contains("stdio::"), "{}", emitted.c);
}

#[test]
fn a_lark_marker_never_reaches_the_output() {
    let emitted = emit(
        "export managed struct Person { gc char* name; }\n\
         gc_leaf void handle(void* data);\n\
         init void main(void) { return; }\n",
    );
    for marker in ["export", "managed", "gc_leaf", "init "] {
        assert!(
            !emitted.c.contains(marker),
            "the emitter kept `{marker}`\n{}",
            emitted.c
        );
    }
    assert!(emitted.header.contains("char* name;"), "{}", emitted.header);
}

/// covers: O-25
#[test]
fn a_definition_with_no_semicolon_gets_one_in_the_output() {
    let emitted = emit("struct Point { int x; }\n");
    assert!(emitted.c.contains("};"), "{}", emitted.c);
}

#[test]
fn the_header_carries_an_include_guard() {
    let emitted = emit("export int f(void);\n");
    assert!(
        emitted.header.contains("#ifndef LARK_APP_H"),
        "{}",
        emitted.header
    );
    assert!(
        emitted.header.contains("#define LARK_APP_H"),
        "{}",
        emitted.header
    );
    assert!(
        emitted.header.trim_end().ends_with("#endif"),
        "{}",
        emitted.header
    );
}

// -- foreign calls ---------------------------------------------------------

/// covers: M-19, M-21
#[test]
fn an_unmarked_extern_call_takes_a_transition() {
    let emitted = emit(
        "managed struct Person { int age; }\n\
         int outside(int value);\n\
         init int main(void) { gc Person* p = new Person { .age = 1 }; return outside(1); }\n",
    );
    assert!(emitted.c.contains("lark_enter_safe()"), "{}", emitted.c);
    assert!(
        emitted.c.contains("lk_leave__i(outside(1))"),
        "{}",
        emitted.c
    );
}

/// covers: M-20
#[test]
fn a_leaf_call_takes_no_transition() {
    let emitted = emit(
        "managed struct Person { int age; }\n\
         gc_leaf int outside(int value);\n\
         init int main(void) { gc Person* p = new Person { .age = 1 }; return outside(1); }\n",
    );
    assert!(!emitted.c.contains("lark_enter_safe()"), "{}", emitted.c);
    assert!(emitted.c.contains("outside(1)"), "{}", emitted.c);
}

/// covers: M-19
#[test]
fn a_lark_function_takes_no_transition() {
    // A function with a body is not foreign, so no rule applies to it.
    let emitted = emit(
        "managed struct Person { int age; }\n\
         int helper(int value) { return value; }\n\
         init int main(void) { gc Person* p = new Person { .age = 1 }; return helper(1); }\n",
    );
    assert!(!emitted.c.contains("lark_enter_safe()"), "{}", emitted.c);
}

/// covers: M-19
#[test]
fn a_module_with_no_runtime_takes_no_transition() {
    // Constraint D-1. Unmanaged code pays nothing.
    let emitted = emit("int outside(int value);\nint main(void) { return outside(1); }\n");
    assert!(!emitted.c.contains("lark_enter_safe()"), "{}", emitted.c);
    assert!(!emitted.uses_runtime);
}

/// covers: M-19
#[test]
fn a_void_call_uses_the_comma_form() {
    let emitted = emit(
        "managed struct Person { int age; }\n\
         void outside(int value);\n\
         init int main(void) { gc Person* p = new Person { .age = 1 }; outside(1); return 0; }\n",
    );
    assert!(
        emitted
            .c
            .contains("(lark_enter_safe(), outside(1), lark_leave_safe())"),
        "{}",
        emitted.c
    );
}

// ---------------------------------------------------------------------------
// Names and linkage in the emitted C.
// ---------------------------------------------------------------------------

/// An extern C symbol keeps its exact name, with no prefix of any kind.
/// covers: X-6, N-13
#[test]
fn an_extern_c_symbol_keeps_its_name() {
    let emitted = emit_modules(&[
        (
            "app",
            "@import stdio\ninit int main(void) { stdio::printf(\"x\\n\"); return 0; }\n",
        ),
        (
            "stdio",
            "export gc_safe int printf(const char* format, ...);\n",
        ),
    ]);
    // The call keeps the C name, so the linker resolves it from libc.
    assert!(emitted.c.contains("printf(\"x\\n\")"), "{}", emitted.c);
    assert!(!emitted.c.contains("lk_stdio__printf"), "{}", emitted.c);
    assert!(!emitted.c.contains("stdio::printf"), "{}", emitted.c);
}

/// Every generated name that the linker sees carries the module name and a
/// double underscore. A generated local carries the prefix alone.
/// covers: X-7, X-5a
#[test]
fn every_generated_name_uses_the_reserved_space() {
    /// The generated names that live inside one block. See rule X-7.
    const LOCALS: &[&str] = &["lk_frame", "lk_self", "lk_result", "lk_config"];

    let emitted = emit(
        "managed struct Person { gc char* name; }\n\
         iface Greet { void say_hi(Self this); }\n\
         impl Greet for Person { void say_hi(Person this) { return; } }\n\
         init int main(void) { auto p = new Person { .name = \"a\" }; p.say_hi(); return 0; }\n",
    );
    let body = format!("{}{}", emitted.header, emitted.c);
    let mut found = 0;
    for word in body.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if !word.starts_with("lk_") || LOCALS.contains(&word) {
            continue;
        }
        found += 1;
        assert!(
            word.starts_with("lk_app__"),
            "the name `{word}` carries no module name"
        );
        assert!(
            word["lk_app__".len()..].contains("__") || word.matches("__").count() >= 1,
            "the name `{word}` has no double underscore"
        );
    }
    assert!(found > 0, "the program generated no name\n{body}");
}

/// A record definition also emits a `typedef` of the same name, so Lark code
/// can name the type without the `struct` keyword.
/// covers: X-8
#[test]
fn a_record_definition_also_emits_a_typedef() {
    let emitted = emit("struct Point { int x; }\nunion Value { int a; }\nenum Color { red }\n");
    assert!(
        emitted.c.contains("typedef struct Point Point;"),
        "{}",
        emitted.c
    );
    assert!(
        emitted.c.contains("typedef union Value Value;"),
        "{}",
        emitted.c
    );
    assert!(
        emitted.c.contains("typedef enum Color Color;"),
        "{}",
        emitted.c
    );
}

/// A private definition emits as `static`, and `main` never does.
/// covers: N-15, X-5b
#[test]
fn a_private_definition_is_static_and_main_is_not() {
    let emitted = emit(
        "int helper(int a) { return a + 1; }\n\
         export int shared(int a) { return a + 2; }\n\
         init int main(void) { return helper(1) + shared(1) - 5; }\n",
    );
    assert!(emitted.c.contains("static int helper"), "{}", emitted.c);
    assert!(!emitted.c.contains("static int shared"), "{}", emitted.c);
    assert!(!emitted.c.contains("static int main"), "{}", emitted.c);
}

/// A Lark symbol keeps its own name in the emitted C.
/// covers: N-14, X-5
#[test]
fn a_lark_symbol_keeps_its_name() {
    let emitted = emit(
        "export managed struct Person { gc char* name; }\n\
         export void draw(gc Person* p) { return; }\n",
    );
    // Rule X-2 keeps the spacing of the source, so the text is what a
    // programmer wrote, with the marker removed.
    assert!(emitted.c.contains("void draw(Person* p)"), "{}", emitted.c);
    assert!(!emitted.c.contains("lk_app__draw"), "{}", emitted.c);
}

/// An `#include` passes through unchanged, and Lark re-emits no declaration
/// that the header already gives.
/// covers: C-3
#[test]
fn an_include_passes_through_and_nothing_is_re_emitted() {
    let emitted = emit("#include <stdio.h>\nint main(void) { return 0; }\n");
    assert_eq!(
        emitted.c.matches("#include <stdio.h>").count(),
        1,
        "the directive must appear exactly once\n{}",
        emitted.c
    );
    // Lark declares nothing that the header declares.
    assert!(!emitted.c.contains("int printf("), "{}", emitted.c);
}

// ---------------------------------------------------------------------------
// Managed memory in the emitted C.
// ---------------------------------------------------------------------------

/// The payload of a managed struct is a plain C struct, so its layout is the
/// C layout. The header sits before it and is not part of the definition.
/// covers: O-1, O-3, C-12
#[test]
fn a_managed_struct_emits_a_plain_c_struct() {
    let emitted = emit("managed struct Person { gc char* name; int age; }\n");
    // The definition holds the fields in order and nothing else.
    let body = format!("{}{}", emitted.header, emitted.c);
    // Rule X-2 keeps the spacing of the source. The `gc` marker goes, and the
    // rest of the field is what the programmer wrote.
    assert!(body.contains("char* name;"), "{body}");
    assert!(body.contains("int age;"), "{body}");
    // The header is not a field. The collector puts it at a negative offset.
    assert!(!body.contains("lark_header header;"), "{body}");
}

/// A field with no designator is zero, which the emitter states rather than
/// leaves to the allocator.
/// covers: O-5
#[test]
fn a_field_with_no_designator_is_zero() {
    let emitted = emit(
        "managed struct Person { gc char* name; int age; }\n\
         init int main(void) { auto p = new Person { .age = 1 }; return p->age - 1; }\n",
    );
    // The initializer names only `age`, and C zeroes the rest of the literal.
    assert!(emitted.c.contains(".age = 1"), "{}", emitted.c);
    assert!(!emitted.c.contains(".name ="), "{}", emitted.c);
}

/// The poll is a load and a branch behind one macro, so the cost is visible.
/// covers: M-17
#[test]
fn a_loop_carries_one_poll_macro() {
    let emitted = emit(
        "managed struct Node { gc Node* next; }\n\
         init int main(void) { auto n = new Node { }; \
         for (int i = 0; i < 3; i++) { n = new Node { .next = n }; } return 0; }\n",
    );
    assert!(emitted.c.contains("LARK_POLL();"), "{}", emitted.c);
}

/// A function that cannot reach an allocation emits no poll.
/// covers: M-18
#[test]
fn a_function_that_cannot_allocate_emits_no_poll() {
    let emitted = emit(
        "int add(int a, int b) { for (int i = 0; i < 3; i++) { a = a + b; } return a; }\n\
         init int main(void) { return add(1, 2) - 3; }\n",
    );
    assert!(!emitted.c.contains("LARK_POLL"), "{}", emitted.c);
}

/// The runtime startup is the first statement of the `init` function, before
/// anything else, and it comes before the frame push.
/// covers: I-3, I-4, I-5
#[test]
fn the_startup_is_the_first_statement_of_the_init_function() {
    let emitted = emit(
        "managed struct Node { gc Node* next; }\n\
         init int entry(void) { auto n = new Node { }; return 0; }\n",
    );
    let body = &emitted.c;
    let Some(start) = body.find("lark_startup") else {
        panic!("the startup is missing\n{body}");
    };
    let Some(push) = body.find("lark_frame_push") else {
        panic!("the frame push is missing\n{body}");
    };
    // Rule I-4 step 2 attaches the thread, so a frame push before it reaches
    // no thread at all.
    assert!(
        start < push,
        "the startup must come before the frame push\n{body}"
    );
    // Rule I-5. The marker names the entry point, and it need not be `main`.
    let Some(entry) = body.find("int entry(void)") else {
        panic!("the entry point is missing\n{body}");
    };
    assert!(
        entry < start,
        "the startup must sit inside the function\n{body}"
    );
}

// ---------------------------------------------------------------------------
// Data compatibility with C.
// ---------------------------------------------------------------------------

/// A `gc T*` is a `T*` in the emitted C, and a managed struct payload is a
/// plain C struct. Both match C exactly, so C code reads them.
/// covers: C-10
#[test]
fn a_managed_pointer_is_a_plain_pointer_in_c() {
    let emitted = emit(
        "export managed struct Person { gc char* name; int age; }\n\
         export int age_of(gc Person* p) { return p->age; }\n",
    );
    let body = format!("{}{}", emitted.header, emitted.c);
    // The marker goes, and the pointer stays a pointer.
    assert!(body.contains("int age_of(Person* p)"), "{body}");
    assert!(
        !body.contains("gc "),
        "the marker reached the output\n{body}"
    );
    // A field of a managed type is a plain C pointer too.
    assert!(body.contains("char* name;"), "{body}");
}

/// An interface value is two words, and C has no name for the pair, so the
/// emitter gives it a struct of its own.
/// covers: C-11
#[test]
fn an_interface_value_is_a_two_word_struct() {
    let emitted = emit(
        "export managed struct Person { int age; }\n\
         export iface Greet { void say_hi(Self this); }\n\
         impl Greet for Person { void say_hi(Person this) { return; } }\n",
    );
    let body = format!("{}{}", emitted.header, emitted.c);
    // The two words are the object and the method table.
    assert!(body.contains("void *obj;"), "{body}");
    assert!(body.contains("*vt;"), "{body}");
    // The pair carries the interface name, so a C caller can name it.
    assert!(body.contains("Greet"), "{body}");
}

/// Two modules that export the same name keep it. Lark renames nothing, so the
/// collision reaches the linker exactly as it does between two C files.
/// covers: X-5c
#[test]
fn two_modules_that_export_one_name_both_keep_it() {
    let first = emit_modules(&[("alpha", "export int shared(void) { return 1; }\n")]);
    let second = emit_modules(&[("beta", "export int shared(void) { return 2; }\n")]);
    // Each module emits the name that its source wrote.
    assert!(first.c.contains("int shared(void)"), "{}", first.c);
    assert!(second.c.contains("int shared(void)"), "{}", second.c);
    // Neither carries a module prefix that would hide the collision.
    assert!(!first.c.contains("lk_alpha__shared"), "{}", first.c);
    assert!(!second.c.contains("lk_beta__shared"), "{}", second.c);
}
