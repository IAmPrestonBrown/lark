//! Rule Z-5. The debugger prints a managed value as its type.
//!
//! The script reads the object header that rule M-4 puts before every
//! payload, and the descriptor that rule M-5 fills in. It needs no metadata
//! that the compiler does not already emit.
//!
//! The test skips with a loud message when no debugger is on the path, the
//! way `check-asan` skips. Continuous integration runs on a machine that has
//! one.
//!
//! covers: Z-5

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::PathBuf;
use std::process::Command;

use lark_driver::{Config, build};

/// Returns the repository root, from the directory of this crate.
fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

/// Reports whether a program is on the path.
fn have(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn the_debugger_prints_a_managed_value_as_its_type() {
    if !have("lldb") {
        eprintln!(
            "SKIP  lldb is not on the path, so rule Z-5 is not checked here.\n\
                   Continuous integration runs on a machine that has one."
        );
        return;
    }

    let root = repository_root();
    let scratch = std::env::temp_dir().join("lark-debugger-probe");
    let _ = std::fs::remove_dir_all(&scratch);
    let Ok(()) = std::fs::create_dir_all(&scratch) else {
        panic!("cannot make a scratch directory");
    };

    // The fixture and the modules it imports.
    let Ok(program) = std::fs::read_to_string(root.join("tests/debug/managed_values.lark")) else {
        panic!("the fixture is missing");
    };
    let Ok(()) = std::fs::write(scratch.join("probe.lark"), program) else {
        panic!("cannot write the probe");
    };
    let Ok(stdio) = std::fs::read_to_string(root.join("examples/stdio.lark")) else {
        panic!("the stdio module is missing");
    };
    let Ok(()) = std::fs::write(scratch.join("stdio.lark"), stdio) else {
        panic!("cannot write the stdio module");
    };
    let Ok(()) = std::fs::write(
        scratch.join("lark.toml"),
        format!(
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n\n\
             [build]\nout = \"{}\"\nruntime = \"{}\"\n",
            scratch.join("build").display(),
            root.join("runtime").display()
        ),
    ) else {
        panic!("cannot write lark.toml");
    };

    let Ok(config) = Config::load(&scratch) else {
        panic!("cannot read lark.toml");
    };
    let Ok(result) = build(&scratch.join("probe.lark"), &config) else {
        panic!("the build failed");
    };
    assert!(!result.failed(), "the build reported a problem");

    // Rule Z-5. The build writes the script beside the program.
    let script = scratch.join("build").join("lark_lldb.py");
    assert!(script.exists(), "the build wrote no debugger script");

    let commands = scratch.join("commands.txt");
    let Ok(()) = std::fs::write(
        &commands,
        format!(
            "command script import {}\n\
             breakpoint set --name inspect\n\
             run\n\
             frame select 1\n\
             frame variable one\n\
             frame variable team\n\
             gc-stats\n\
             quit\n",
            script.display()
        ),
    ) else {
        panic!("cannot write the debugger commands");
    };

    let Ok(output) = Command::new("lldb")
        .arg("-b")
        .arg("-s")
        .arg(&commands)
        .arg(scratch.join("build").join("probe"))
        .current_dir(&scratch)
        .output()
    else {
        panic!("lldb did not run");
    };
    let text = String::from_utf8_lossy(&output.stdout);

    // A managed value prints as its type, not as an address alone.
    assert!(
        text.contains("Person at 0x"),
        "the value did not print as its type:\n{text}"
    );
    // An array prints its count, which comes from the header.
    assert!(
        text.contains("Person[3] at 0x"),
        "the array did not print its count:\n{text}"
    );
    // The heap command reads the collector through the runtime. The count of
    // allocations is the one number that every collector keeps as it goes. A
    // mark and sweep collector counts the live set during a sweep, and no
    // sweep has run yet.
    assert!(
        text.contains("total_allocations  2"),
        "the heap command did not report the allocations:\n{text}"
    );
    assert!(
        text.contains("collector          precise-marksweep"),
        "the heap command did not name the collector:\n{text}"
    );
}
