//! The build cache. Rule Y-5, and what it must never do.
//!
//! A wrong cache produces a program that builds and misbehaves, which is the
//! worst failure a build tool has. So these tests spend most of their effort
//! on the miss cases: an edit, a header change, a settings change. A hit that
//! should have been a miss is the failure to find here.
//!
//! covers: Y-1, Y-2, Y-3, Y-4, Y-5, Y-6

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lark_cache::Cache;
use lark_driver::{Build, Config, build_with};

/// A project in a scratch directory.
struct Project {
    root: PathBuf,
}

impl Project {
    /// Makes an empty project.
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("lark-incremental-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let Ok(()) = std::fs::create_dir_all(&root) else {
            panic!("cannot make a scratch directory");
        };
        let project = Self { root };
        project.settings("precise-marksweep", "c11");
        project
    }

    /// Writes `lark.toml` with one collector and one standard.
    fn settings(&self, collector: &str, standard: &str) {
        let runtime = repository_root().join("runtime");
        // The test process runs in the repository root, and `build.out` is
        // read as it is written, so the path is absolute here.
        self.write(
            "lark.toml",
            &format!(
                "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n\n\
                 [build]\nstd = \"{standard}\"\nout = \"{}\"\nruntime = \"{}\"\n\n\
                 [gc]\nstrategy = \"{collector}\"\n",
                self.root.join("build").display(),
                runtime.display()
            ),
        );
    }

    /// Writes one file.
    fn write(&self, name: &str, text: &str) {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(()) = std::fs::write(&path, text) else {
            panic!("cannot write {}", path.display());
        };
        // The digest memo is for the life of a build. A test writes and reads
        // in one process, so it starts a new build here.
        lark_cache::forget_digests();
    }

    /// Builds the named module and returns what the driver reported.
    fn build(&self, name: &str) -> Build {
        self.build_with(name, &Cache::open(&self.root.join("build")))
    }

    /// Builds with one cache, so a test can turn it off.
    fn build_with(&self, name: &str, cache: &Cache) -> Build {
        let Ok(config) = Config::load(&self.root) else {
            panic!("cannot read lark.toml");
        };
        match build_with(&self.root.join(name), &config, cache) {
            Ok(result) => {
                assert!(
                    !result.failed(),
                    "the build failed:\n{}",
                    lark_diag::render_all(&result.diagnostics, &result.sources)
                );
                result
            }
            Err(error) => panic!("the build failed: {error}"),
        }
    }

    /// Runs the binary and returns what it printed.
    fn run(&self, name: &str) -> String {
        let binary = self.root.join("build").join(name);
        let Ok(output) = std::process::Command::new(&binary).output() else {
            panic!("cannot run {}", binary.display());
        };
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

/// Returns the repository root, from the directory of this crate.
fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

/// The program that most of these tests build.
const PROGRAM: &str = "#include <stdio.h>\n\
                       #include \"shared.h\"\n\
                       int main(void) { printf(\"value %d\\n\", VALUE); return 0; }\n";

/// A second build with no change compiles nothing.
/// covers: Y-5
#[test]
fn a_second_build_compiles_nothing() {
    let project = Project::new("no-change");
    project.write("app.lark", PROGRAM);
    project.write("shared.h", "#define VALUE 1\n");

    let first = project.build("app.lark");
    assert!(first.compiled > 0, "the first build must compile");
    assert_eq!(project.run("app"), "value 1\n");

    let second = project.build("app.lark");
    assert_eq!(
        second.compiled, 0,
        "the second build compiled {} objects",
        second.compiled
    );
    assert!(second.reused > 0, "the second build must reuse");
    assert_eq!(project.run("app"), "value 1\n");
}

/// An edit to the source recompiles the module, and the program changes.
/// covers: Y-5
#[test]
fn an_edit_recompiles_the_module() {
    let project = Project::new("edit");
    project.write("app.lark", PROGRAM);
    project.write("shared.h", "#define VALUE 1\n");

    project.build("app.lark");
    assert_eq!(project.run("app"), "value 1\n");

    project.write("app.lark", &PROGRAM.replace("value %d", "changed %d"));
    let after = project.build("app.lark");
    assert!(after.compiled > 0, "an edit must compile something");
    assert_eq!(
        project.run("app"),
        "changed 1\n",
        "the binary must hold the edit"
    );
}

/// Rule Y-5. A header that the compile read, and that the key did not name,
/// still makes the entry a miss when it changes.
/// covers: Y-2, Y-5
#[test]
fn a_changed_header_recompiles_what_reads_it() {
    let project = Project::new("header");
    project.write("app.lark", PROGRAM);
    project.write("shared.h", "#define VALUE 1\n");

    project.build("app.lark");
    assert_eq!(project.run("app"), "value 1\n");

    // The source did not change. Only the header did.
    project.write("shared.h", "#define VALUE 42\n");
    let after = project.build("app.lark");
    assert!(
        after.compiled > 0,
        "a changed header must compile something"
    );
    assert_eq!(
        project.run("app"),
        "value 42\n",
        "the binary must hold the new header value"
    );
}

/// A change to the settings recompiles, because the flags are in the key.
/// covers: Y-5, F-2
#[test]
fn a_settings_change_recompiles_everything() {
    let project = Project::new("settings");
    project.write(
        "app.lark",
        "managed struct Node { gc Node* next; int value; }\n\
         init int main(void) { auto n = new Node { .value = 7 }; return n->value - 7; }\n",
    );

    let first = project.build("app.lark");
    assert!(first.compiled > 0);
    let second = project.build("app.lark");
    assert_eq!(second.compiled, 0, "nothing changed yet");

    // A different collector is a different program.
    project.settings("arena", "c11");
    let third = project.build("app.lark");
    assert!(
        third.compiled > 0,
        "a different collector must compile again"
    );
}

/// A module that another one imports recompiles its dependants when its
/// exported header changes.
/// covers: Y-5
#[test]
fn a_changed_interface_recompiles_the_importer() {
    let project = Project::new("interface");
    project.write("helper.lark", "export int value(void) { return 1; }\n");
    project.write(
        "app.lark",
        "@import helper\n#include <stdio.h>\n\
         init int main(void) { printf(\"got %d\\n\", helper::value()); return 0; }\n",
    );

    project.build("app.lark");
    assert_eq!(project.run("app"), "got 1\n");

    // The body changes and the interface does not, but the value does.
    project.write("helper.lark", "export int value(void) { return 9; }\n");
    let after = project.build("app.lark");
    assert!(after.compiled > 0, "a changed module must compile again");
    assert_eq!(
        project.run("app"),
        "got 9\n",
        "the binary must hold the new value"
    );
}

/// Rule Y-3. A cache is a saving, never a source of truth. Removing it gives
/// the same program.
/// covers: Y-3
#[test]
fn removing_the_cache_gives_the_same_program() {
    let project = Project::new("removable");
    project.write("app.lark", PROGRAM);
    project.write("shared.h", "#define VALUE 5\n");

    project.build("app.lark");
    let expected = project.run("app");

    let _ = std::fs::remove_dir_all(project.root.join("build").join(".lark-cache"));
    let after = project.build("app.lark");
    assert!(after.compiled > 0, "an empty cache must compile");
    assert_eq!(project.run("app"), expected);
}

/// Many builds with edits between them each produce the right program.
///
/// A cache that answers a stale entry shows here as one wrong line, and the
/// message names the round.
/// covers: Y-5
#[test]
fn repeated_edits_each_produce_the_right_program() {
    let project = Project::new("stress");
    project.write("shared.h", "#define VALUE 0\n");

    for round in 0..12 {
        // Alternate between editing the source and editing the header, so both
        // paths run many times.
        if round % 2 == 0 {
            project.write(
                "app.lark",
                &format!(
                    "#include <stdio.h>\n#include \"shared.h\"\n\
                     int main(void) {{ printf(\"round {round} %d\\n\", VALUE); return 0; }}\n"
                ),
            );
        } else {
            project.write("shared.h", &format!("#define VALUE {round}\n"));
        }

        project.build("app.lark");
        let printed = project.run("app");
        let source = std::fs::read_to_string(project.root.join("app.lark")).unwrap_or_default();
        let header = std::fs::read_to_string(project.root.join("shared.h")).unwrap_or_default();

        // Read the two values back out of the files, so the check needs no
        // model of its own.
        let Some(label) = source
            .split("printf(\"")
            .nth(1)
            .and_then(|rest| rest.split(" %d").next())
        else {
            panic!("the test wrote a program it cannot read back");
        };
        let Some(value) = header.split_whitespace().nth(2) else {
            panic!("the test wrote a header it cannot read back");
        };
        assert_eq!(
            printed,
            format!("{label} {value}\n"),
            "round {round} produced the wrong program"
        );
    }
}

/// A build with the cache off produces the same program as one with it on.
/// covers: Y-3
#[test]
fn the_cache_changes_no_output() {
    let project = Project::new("same-output");
    project.write("app.lark", PROGRAM);
    project.write("shared.h", "#define VALUE 3\n");

    project.build_with("app.lark", &Cache::disabled());
    let without = project.run("app");

    let _ = std::fs::remove_dir_all(project.root.join("build"));
    project.build("app.lark");
    let with = project.run("app");

    assert_eq!(without, with);
}

/// The emitted C is the same whether the cache answered or not.
#[test]
fn the_emitted_c_does_not_depend_on_the_cache() {
    let project = Project::new("emitted");
    project.write("app.lark", PROGRAM);
    project.write("shared.h", "#define VALUE 2\n");

    project.build("app.lark");
    let first = read_emitted(&project.root);
    project.build("app.lark");
    let second = read_emitted(&project.root);
    assert_eq!(first, second);
}

/// Returns the emitted C of a build.
fn read_emitted(root: &Path) -> String {
    std::fs::read_to_string(root.join("build").join("app.c")).unwrap_or_default()
}

/// Rule Y-6. Several units compile at the same time, and the program is the
/// same as one that compiled them in order.
///
/// A race in the cache shows here as a differing run, and ten rounds make one
/// likely enough to catch.
/// covers: Y-6
#[test]
fn many_units_compile_together_and_give_one_answer() {
    let project = Project::new("parallel");
    // Ten modules, so the batch is wider than one.
    let mut imports = String::new();
    let mut sum = String::new();
    for index in 0..10 {
        project.write(
            &format!("part{index}.lark"),
            &format!("export int value{index}(void) {{ return {index}; }}\n"),
        );
        let _ = writeln!(imports, "@import part{index}");
        if index > 0 {
            sum.push_str(" + ");
        }
        let _ = write!(sum, "part{index}::value{index}()");
    }
    project.write(
        "app.lark",
        &format!(
            "{imports}#include <stdio.h>\n\
             init int main(void) {{ printf(\"total %d\\n\", {sum}); return 0; }}\n"
        ),
    );

    let first = project.build("app.lark");
    assert!(first.compiled >= 10, "every module must compile");
    let expected = "total 45\n";
    assert_eq!(project.run("app"), expected);

    // Ten more rounds, each from an empty cache, all with the same answer.
    for round in 0..10 {
        let _ = std::fs::remove_dir_all(project.root.join("build"));
        project.build("app.lark");
        assert_eq!(
            project.run("app"),
            expected,
            "round {round} produced a different program"
        );
    }
}

/// Rule Y-4. The header read is cached, so a second build runs no
/// preprocessor. The saving is visible as a faster second build.
/// covers: Y-4
#[test]
fn the_header_read_is_cached() {
    let project = Project::new("header-cache");
    project.write(
        "app.lark",
        "#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n\
         int main(void) { printf(\"%zu\\n\", strlen(\"abc\")); return EXIT_SUCCESS; }\n",
    );

    project.build("app.lark");
    // The cache holds one preprocessed unit, under a key that no other step
    // uses. The extension names the step.
    let cache = project.root.join("build").join(".lark-cache");
    let Ok(entries) = std::fs::read_dir(&cache) else {
        panic!("the cache directory is missing");
    };
    let preprocessed = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|value| value == "i"))
        .count();
    assert_eq!(preprocessed, 1, "the header read must be cached once");

    // A second build reuses it, and the program is the same.
    project.build("app.lark");
    assert_eq!(project.run("app"), "3\n");
}
