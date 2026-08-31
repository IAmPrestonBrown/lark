//! Checks the initialization rules from chapter 07.

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;

use lark_diag::{Code, LK0700, LK0701};
use lark_resolve::{MemoryLoader, resolve};
use lark_types::{check_program, globals};

/// Returns the codes that the whole program check produces.
fn program_codes(source: &str) -> Vec<Code> {
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    check_program(&resolution)
        .items()
        .iter()
        .map(|item| item.code)
        .collect()
}

/// covers: I-1
#[test]
fn a_program_that_uses_managed_memory_needs_an_init_function() {
    let source = "managed struct Person { gc char* name; }\n\
                  int main(void) { gc Person* p = new Person { .name = \"x\" }; return 0; }\n";
    assert!(
        program_codes(source).contains(&LK0700),
        "{:?}",
        program_codes(source)
    );
}

/// covers: I-1
#[test]
fn a_program_with_an_init_function_is_quiet() {
    let source = "managed struct Person { gc char* name; }\n\
                  init int main(void) { gc Person* p = new Person { .name = \"x\" }; return 0; }\n";
    assert!(
        !program_codes(source).contains(&LK0700),
        "{:?}",
        program_codes(source)
    );
}

/// covers: I-1, S-1
#[test]
fn a_program_with_no_managed_memory_needs_no_marker() {
    // Rule S-1 keeps a valid C11 file valid, and such a file carries no marker.
    let source = "#include <stdio.h>\nint main(void) { return 0; }\n";
    assert!(
        program_codes(source).is_empty(),
        "{:?}",
        program_codes(source)
    );
}

/// covers: I-1
#[test]
fn two_init_functions_report_lk0701() {
    let loader = MemoryLoader::new([(
        "app",
        "init int main(void) { return 0; }\ninit int other(void) { return 0; }\n",
    )]);
    let source = "init int main(void) { return 0; }\ninit int other(void) { return 0; }\n";
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    let reported = lark_types::check_resolution(&resolution);
    let codes: Vec<Code> = reported.items().iter().map(|item| item.code).collect();
    assert!(codes.contains(&LK0701), "{codes:?}");
}

/// covers: I-13, I-14
#[test]
fn a_numbered_block_runs_before_an_unnumbered_one() {
    let source = "@global(main) third { int c; }\n\
                  @global(main, 1) second { int b; }\n\
                  @global(main, 0) first { int a; }\n\
                  init int main(void) { return 0; }\n";
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    let Some(module) = resolution.graph.get(0) else {
        panic!("the root module must exist");
    };
    let found = globals::collect(&module.parse.syntax());
    let order: Vec<String> = found
        .attached_to("main")
        .iter()
        .map(|block| block.name.clone())
        .collect();
    assert_eq!(order, vec!["first", "second", "third"]);
}

/// covers: I-6
#[test]
fn a_block_records_the_names_it_declares() {
    let source = "@global data { int one; gc char* two; }\ninit int main(void) { return 0; }\n";
    let loader = MemoryLoader::new([("app", source)]);
    let resolution = resolve(&loader, "app", &PathBuf::from("app.lark"), source);
    let Some(module) = resolution.graph.get(0) else {
        panic!("the root module must exist");
    };
    let found = globals::collect(&module.parse.syntax());
    let block = found.blocks.get("data").expect("the block must exist");
    assert_eq!(block.declares, vec!["one", "two"]);
}
