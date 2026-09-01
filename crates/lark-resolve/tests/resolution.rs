//! Checks the module graph, the symbol tables, and the reference checks.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::{Code, LK0100, LK0600, LK0610, LK0611, LK0612, LK0613, LK0614};
use lark_resolve::{MemoryLoader, Resolution, SymbolKind, Visibility, resolve};

/// Resolves a set of modules, with the first one as the root.
fn resolve_modules(modules: &[(&str, &str)]) -> Resolution {
    let Some((root_name, root_text)) = modules.first() else {
        panic!("a fixture needs at least one module");
    };
    let loader = MemoryLoader::new(modules.iter().map(|(name, text)| (*name, *text)));
    let path = PathBuf::from(format!("{root_name}.lark"));
    resolve(&loader, root_name, &path, root_text)
}

/// Returns every diagnostic code that the run produced.
fn codes(resolution: &Resolution) -> Vec<Code> {
    resolution
        .diagnostics
        .items()
        .iter()
        .map(|item| item.code)
        .collect()
}

/// Reports whether the run produced a code.
fn reports(resolution: &Resolution, code: Code) -> bool {
    codes(resolution).contains(&code)
}

/// Returns the root module, or fails the test.
fn root(resolution: &Resolution) -> &lark_resolve::Module {
    match resolution.graph.get(0) {
        Some(module) => module,
        None => panic!("the root module must exist"),
    }
}

// -- pass one, the symbol table -------------------------------------------

/// covers: L-8, N-1
#[test]
fn pass_one_records_every_top_level_name() {
    let resolution = resolve_modules(&[(
        "app",
        "managed struct Person { int age; }\n\
         iface Greet { void say_hi(Self this); }\n\
         typedef int Count;\n\
         int total;\n\
         void helper(void) { }\n",
    )]);
    let module = root(&resolution);
    let kind = |name: &str| module.table.get(name).map(|symbol| symbol.kind);
    assert_eq!(kind("Person"), Some(SymbolKind::Type));
    assert_eq!(kind("Greet"), Some(SymbolKind::Iface));
    assert_eq!(kind("Count"), Some(SymbolKind::Type));
    assert_eq!(kind("total"), Some(SymbolKind::Global));
    assert_eq!(kind("helper"), Some(SymbolKind::Function));
}

/// covers: L-8
#[test]
fn a_name_resolves_before_its_declaration() {
    let resolution = resolve_modules(&[(
        "app",
        "gc Person* first;\nmanaged struct Person { int age; }\n",
    )]);
    assert!(
        resolution.diagnostics.items().is_empty(),
        "{:?}",
        codes(&resolution)
    );
}

/// covers: N-5, N-6
#[test]
fn export_marks_a_symbol_and_the_default_is_private() {
    let resolution = resolve_modules(&[(
        "app",
        "export managed struct Person { int age; }\nstruct Hidden { int x; }\n",
    )]);
    let module = root(&resolution);
    let visibility = |name: &str| module.table.get(name).map(|symbol| symbol.visibility);
    assert_eq!(visibility("Person"), Some(Visibility::Exported));
    assert_eq!(visibility("Hidden"), Some(Visibility::Private));
}

/// covers: N-7, N-8, I-6
#[test]
fn a_global_block_declares_module_globals() {
    let resolution = resolve_modules(&[("app", "export @global main_globals { int counter; }\n")]);
    let module = root(&resolution);
    let symbol = module.table.get("counter");
    assert_eq!(symbol.map(|item| item.kind), Some(SymbolKind::Global));
    assert_eq!(
        symbol.map(|item| item.visibility),
        Some(Visibility::Exported)
    );
}

// -- the module graph ------------------------------------------------------

/// covers: N-2, N-3
#[test]
fn an_import_links_to_the_module_it_names() {
    let resolution = resolve_modules(&[
        (
            "app",
            "@import stdio\nvoid f(void) { stdio::printf(\"x\"); }\n",
        ),
        ("stdio", "export int printf(const char* format, ...);\n"),
    ]);
    assert_eq!(resolution.graph.len(), 2);
    assert_eq!(resolution.graph.import_target(0, "stdio"), Some(1));
    assert!(
        resolution.diagnostics.items().is_empty(),
        "{:?}",
        codes(&resolution)
    );
}

/// covers: N-3
#[test]
fn a_missing_module_reports_lk0600() {
    let resolution = resolve_modules(&[("app", "@import nowhere\n")]);
    assert!(reports(&resolution, LK0600), "{:?}", codes(&resolution));
}

/// covers: N-4
#[test]
fn an_import_cycle_is_legal_and_terminates() {
    let resolution = resolve_modules(&[
        ("a", "@import b\nexport int from_a;\n"),
        ("b", "@import a\nexport int from_b;\n"),
    ]);
    assert_eq!(resolution.graph.len(), 2);
    assert!(
        resolution.diagnostics.items().is_empty(),
        "{:?}",
        codes(&resolution)
    );
}

// -- the reference checks --------------------------------------------------

/// covers: N-2
#[test]
fn a_path_to_a_module_that_is_not_imported_reports_lk0613() {
    let resolution = resolve_modules(&[("app", "void f(void) { stdio::printf(\"x\"); }\n")]);
    assert!(reports(&resolution, LK0613), "{:?}", codes(&resolution));
}

/// covers: N-6
#[test]
fn a_path_to_a_private_symbol_reports_lk0611() {
    let resolution = resolve_modules(&[
        ("app", "@import util\nvoid f(void) { util::secret(); }\n"),
        ("util", "void secret(void) { }\n"),
    ]);
    assert!(reports(&resolution, LK0611), "{:?}", codes(&resolution));
}

/// covers: N-6
#[test]
fn a_path_to_a_name_the_module_does_not_declare_reports_lk0611() {
    let resolution = resolve_modules(&[
        ("app", "@import util\nvoid f(void) { util::missing(); }\n"),
        ("util", "export void present(void) { }\n"),
    ]);
    assert!(reports(&resolution, LK0611), "{:?}", codes(&resolution));
}

/// covers: N-2, N-11
#[test]
fn a_bare_name_from_an_imported_module_reports_lk0612() {
    let resolution = resolve_modules(&[
        ("app", "@import shapes\ngc Point* p;\n"),
        ("shapes", "export struct Point { int x; }\n"),
    ]);
    assert!(reports(&resolution, LK0612), "{:?}", codes(&resolution));
    let help = resolution
        .diagnostics
        .items()
        .iter()
        .find(|item| item.code == LK0612)
        .and_then(|item| item.help.clone());
    assert_eq!(help.as_deref(), Some("write `shapes::Point`"));
}

/// covers: N-10
#[test]
fn an_exported_signature_that_names_a_private_type_reports_lk0610() {
    let resolution = resolve_modules(&[(
        "app",
        "struct Hidden { int x; }\nexport void f(gc Hidden* h);\n",
    )]);
    assert!(reports(&resolution, LK0610), "{:?}", codes(&resolution));
}

/// covers: N-10
#[test]
fn an_exported_signature_that_names_an_exported_type_is_quiet() {
    let resolution = resolve_modules(&[(
        "app",
        "export struct Shown { int x; }\nexport void f(gc Shown* h);\n",
    )]);
    assert!(!reports(&resolution, LK0610), "{:?}", codes(&resolution));
}

/// covers: L-15, L-6
#[test]
fn a_generic_base_that_names_no_type_reports_lk0100() {
    let resolution = resolve_modules(&[("app", "gc Missing<int>* p;\n")]);
    assert!(reports(&resolution, LK0100), "{:?}", codes(&resolution));
}

/// covers: L-15
#[test]
fn an_unread_include_silences_the_unknown_type_check() {
    let resolution = resolve_modules(&[("app", "#include <stdio.h>\ngc Missing<int>* p;\n")]);
    assert!(!reports(&resolution, LK0100), "{:?}", codes(&resolution));
}

/// covers: O-21
#[test]
fn a_qualified_method_call_is_not_a_module_path() {
    let resolution = resolve_modules(&[(
        "app",
        "iface Greet { void say_hi(Self this); }\nvoid f(void) { x.Greet::say_hi(); }\n",
    )]);
    assert!(!reports(&resolution, LK0613), "{:?}", codes(&resolution));
}

// -- pass two, the oracle --------------------------------------------------

/// covers: L-6
#[test]
fn pass_two_reads_a_generic_call_with_a_known_type() {
    let resolution = resolve_modules(&[(
        "app",
        "struct Person { int age; }\nvoid f(void) { swap<Person>(&a, &b); }\n",
    )]);
    let tree = root(&resolution).parse.tree_text();
    assert!(
        tree.contains("GENERIC_ARGS"),
        "the oracle must find Person\n{tree}"
    );
}

/// covers: L-6
#[test]
fn pass_two_keeps_a_comparison_when_the_name_is_a_value() {
    let resolution = resolve_modules(&[(
        "app",
        "int a; int b; int c;\nvoid f(void) { g(a<b>(c)); }\n",
    )]);
    let tree = root(&resolution).parse.tree_text();
    assert!(
        !tree.contains("GENERIC_ARGS"),
        "a value before an angle is a comparison\n{tree}"
    );
}

// -- the reference example -------------------------------------------------

/// Resolves the tour example from disk, with `examples/` on the search path.
#[test]
fn the_tour_example_resolves_with_no_problem() {
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

    let reported: Vec<String> = resolution
        .diagnostics
        .items()
        .iter()
        .map(|item| format!("{} {}", item.code, item.message))
        .collect();
    assert!(
        reported.is_empty(),
        "the tour must resolve cleanly: {reported:?}"
    );
    assert_eq!(resolution.graph.len(), 2, "the tour imports stdio");
}

/// covers: N-20
#[test]
fn a_namespace_block_holds_no_type_definition() {
    // A function and a variable belong in a block.
    const NAMES_ONLY: &str = "struct Point { int x; }\nnamespace detail { struct Point origin; }\n";
    const CLEAN: &str = "namespace detail {\n\
         int helper(int n) { return n + 1; }\n\
         int counter = 0;\n\
     }\n";
    let loader = MemoryLoader::new([("app", CLEAN)]);
    let clean = resolve(&loader, "app", &PathBuf::from("app.lark"), CLEAN);
    assert!(!reports(&clean, LK0614), "{:?}", codes(&clean));

    // Rule N-20. A type takes its namespace from the directory instead.
    for source in [
        "namespace detail { struct Point { int x; } }\n",
        "namespace detail { union Either { int a; } }\n",
        "namespace detail { enum Colour { RED } }\n",
        "namespace detail { typedef int Small; }\n",
        "namespace detail { iface Show { void show(Self this); } }\n",
    ] {
        let loader = MemoryLoader::new([("app", source)]);
        let found = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
        assert!(
            reports(&found, LK0614),
            "{source:?} gave {:?}",
            codes(&found)
        );
    }

    // A declaration that names a type rather than defining one is fine.
    let loader = MemoryLoader::new([("app", NAMES_ONLY)]);
    let found = resolve(&loader, "app", &PathBuf::from("app.lark"), NAMES_ONLY);
    assert!(!reports(&found, LK0614), "{:?}", codes(&found));
}
