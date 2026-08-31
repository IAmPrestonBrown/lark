//! Trial construction for the test binary.
//!
//! One fixture becomes one trial, or four trials when it builds and runs a
//! binary. See principles P-3 and P-4.

use std::path::{Path, PathBuf};

use lark_diag::render_all;
use libtest_mimic::{Failed, Trial};

use crate::annotation::{self, Actual};
use crate::compiler::{Collector, Compile, Config, Cursor, FrontEnd, Input};
use crate::exec::{self, ExecError};
use crate::fixture::{Fixture, Kind, discover_all};
use crate::snapshot::{Verdict, compare};

/// Returns the root of the repository.
///
/// The path comes from the manifest directory of this crate, so it works from
/// any working directory.
pub fn repository_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    root.canonicalize().unwrap_or(root)
}

/// Builds one trial for every fixture under the repository root.
///
/// A fixture that builds and runs a binary gets four trials, one for each
/// configuration in the matrix. See principles P-3 and P-4.
///
/// # Errors
///
/// Returns an error when a fixture directory cannot be read.
pub fn trials(root: &Path) -> std::io::Result<Vec<Trial>> {
    let mut trials = Vec::new();
    for fixture in discover_all(root)? {
        if fixture.kind.runs_a_binary() {
            // A `gc` fixture exists to test the collector, so it runs against
            // every collector. An `exec` fixture proves the language, and the
            // default collector is enough for that. Principle P-3 asks for the
            // whole matrix where the matrix is the point.
            let matrix = if fixture.kind == Kind::Gc {
                Config::full_matrix()
            } else {
                Config::matrix().to_vec()
            };
            for config in matrix {
                // Rule R-1. A collector that lacks what the fixture needs does
                // not run it, the same answer the transpiler gives at build
                // time.
                if !config.collector.meets(&fixture.needs) {
                    continue;
                }
                let name = format!("{} [{}]", fixture.name, config.suffix());
                let kind = owned_kind(fixture.kind);
                let owned = fixture.clone();
                let root = root.to_path_buf();
                trials.push(Trial::test(name, move || run(&root, &owned, config)).with_kind(kind));
            }
        } else {
            let name = fixture.name.clone();
            let kind = owned_kind(fixture.kind);
            let owned = fixture.clone();
            let root = root.to_path_buf();
            trials.push(
                Trial::test(name, move || run(&root, &owned, Config::default())).with_kind(kind),
            );
        }
    }
    Ok(trials)
}

/// Returns the kind label that a test report prints.
fn owned_kind(kind: Kind) -> String {
    kind.name().to_owned()
}

/// Runs one fixture.
///
/// # Errors
///
/// Returns a report when the fixture does not behave as it must.
fn run(root: &Path, fixture: &Fixture, config: Config) -> Result<(), Failed> {
    let raw = std::fs::read_to_string(&fixture.input)
        .map_err(|error| format!("cannot read {}: {error}", fixture.input.display()))?;

    // A language server fixture marks the position with `<|>`. The marker leaves
    // the text before anything parses it.
    let (text, cursor) = split_cursor(&raw, fixture.kind)?;

    // Shared modules live in `examples/`, so any fixture can import `stdio`.
    // A suite can also hold its own libraries under `modules/`.
    let input = Input {
        path: fixture.input.clone(),
        text: text.clone(),
        config,
        search: vec![
            root.join("examples"),
            root.join(fixture.kind.directory()).join("modules"),
        ],
        is_program: fixture.kind.runs_a_binary(),
        cursor,
    };
    let output = FrontEnd.compile(&input);

    match fixture.kind {
        Kind::Parse => check_parse(fixture, &text, &output),
        Kind::C11 => check_c11(fixture, &output),
        Kind::Ui => check_ui(&text, &output),
        Kind::Golden => check_snapshot(fixture, output.c.as_deref(), "emitted C"),
        Kind::Exec | Kind::Gc => check_exec(root, fixture, config, &output),
        Kind::DebugMap => check_debug_map(root, fixture, &text, &output),
        Kind::Lsp => check_snapshot(fixture, output.lsp.as_deref(), "language server answer"),
    }
}

/// Checks the tree snapshot and invariant R.
fn check_parse(
    fixture: &Fixture,
    text: &str,
    output: &crate::compiler::Output,
) -> Result<(), Failed> {
    // A parse fixture normally reports nothing. A fixture that tests recovery
    // states what it expects with the same annotations as a ui fixture.
    if text.contains("//~") {
        check_ui(text, output)?;
    } else if output.diagnostics.has_errors() {
        return Err(report_diagnostics("a parse fixture must produce no error", output).into());
    }

    // Invariant R. The tree prints back to the input, byte for byte.
    match output.roundtrip.as_deref() {
        Some(printed) if printed == text => {}
        Some(printed) => {
            return Err(format!(
                "invariant R fails: the tree prints back {} bytes and the input holds {}",
                printed.len(),
                text.len()
            )
            .into());
        }
        None => return Err("the front end produced no round trip text".into()),
    }

    check_snapshot(fixture, output.tree.as_deref(), "syntax tree")
}

/// Checks that a valid C11 file produces no diagnostic.
fn check_c11(fixture: &Fixture, output: &crate::compiler::Output) -> Result<(), Failed> {
    if output.diagnostics.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} is valid C11, so rule S-1 requires no diagnostic\n{}",
        fixture.input.display(),
        render_all(&output.diagnostics, &output.sources)
    )
    .into())
}

/// Checks the diagnostics against the inline annotations.
fn check_ui(text: &str, output: &crate::compiler::Output) -> Result<(), Failed> {
    let annotations = annotation::parse(text);
    let produced: Vec<Actual> = output
        .diagnostics
        .items()
        .iter()
        .map(|diagnostic| {
            let file = output.sources.file(diagnostic.primary.file);
            Actual {
                line: file.line_col(diagnostic.primary.span.start).line,
                severity: diagnostic.severity,
                code: diagnostic.code,
            }
        })
        .collect();

    annotation::check(&annotations, &produced).map_err(|report| {
        Failed::from(format!(
            "{report}\n{}",
            render_all(&output.diagnostics, &output.sources)
        ))
    })
}

/// Builds the emitted C, runs it, and compares the output.
fn check_exec(
    root: &Path,
    fixture: &Fixture,
    config: Config,
    output: &crate::compiler::Output,
) -> Result<(), Failed> {
    if output.diagnostics.has_errors() {
        return Err(report_diagnostics("an exec fixture must compile", output).into());
    }
    if output.files.is_empty() {
        return Err("the front end emitted no C".into());
    }

    // Each configuration needs its own directory. The four runs happen at the
    // same time, and a shared directory makes them overwrite each other.
    let key = format!("{}-{}", fixture.name, config.suffix());
    let directory = exec::scratch_directory(root, &key);
    let emitted_c = output.c.clone().unwrap_or_default();
    let runtime = root.join("runtime");
    let runtime = if output.uses_runtime {
        Some(runtime.as_path())
    } else {
        None
    };
    // Rule R-3. A program links exactly one collector, and the configuration
    // of this run names it.
    let collector = match config.collector {
        Collector::PreciseMarkSweep => "gc-marksweep/lark_marksweep.c",
        Collector::Arena => "gc-arena/lark_arena.c",
        Collector::Semispace => "gc-semispace/lark_semispace.c",
        Collector::Generational => "gc-generational/lark_generational.c",
    };
    // The fixture can carry a header of its own beside it. Rule X-4b keeps the
    // generated header out of that name.
    let sources: Vec<PathBuf> = fixture
        .input
        .parent()
        .map(|parent| vec![parent.to_path_buf()])
        .unwrap_or_default();
    let result =
        exec::build_and_run_full(&directory, "program", &output.files, runtime, collector, &sources)
        .map_err(|error| match error {
            ExecError::CompileFailed { command, output } => Failed::from(format!(
                "the C compiler rejected the emitted C\n  {command}\n{output}\n\nemitted C:\n{emitted_c}"
            )),
            ExecError::Io(error) => Failed::from(error.to_string()),
        })?;

    if result.code != Some(0) {
        return Err(format!(
            "the program exited with {:?}\nstdout:\n{}\nstderr:\n{}",
            result.code, result.stdout, result.stderr
        )
        .into());
    }

    check_snapshot(fixture, Some(&result.stdout), "program output")
}

/// Compares the line map, and checks that a C error names the Lark source.
///
/// A fixture that carries `// expect-c-error-at: N` must make the C compiler
/// report line `N` of the `.lark` file. That is rule X-3, proved end to end.
fn check_debug_map(
    root: &Path,
    fixture: &Fixture,
    text: &str,
    output: &crate::compiler::Output,
) -> Result<(), Failed> {
    check_snapshot(fixture, output.debug_map.as_deref(), "debug map")?;

    let Some(line) = directive(text, "expect-c-error-at") else {
        return Ok(());
    };
    let directory = exec::scratch_directory(root, &format!("{}-debugmap", fixture.name));
    let Err(error) = exec::build_and_run(&directory, "program", &output.files, None) else {
        return Err(format!("{} must make the C compiler report an error", fixture.name).into());
    };
    let ExecError::CompileFailed { output: report, .. } = error else {
        return Err("the fixture must fail in the C compiler, not in the harness".into());
    };

    let name = fixture
        .input
        .file_name()
        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
    let wanted = format!("{name}:{line}");
    if report.contains(&wanted) {
        return Ok(());
    }
    Err(
        format!("the C compiler must report `{wanted}`, and rule X-3 maps it back\n{report}")
            .into(),
    )
}

/// The marker that a language server fixture puts at the cursor.
const CURSOR: &str = "<|>";

/// Splits a fixture into its text and its cursor.
///
/// # Errors
///
/// Returns a report when a language server fixture holds no marker, or when a
/// marker has no query.
fn split_cursor(raw: &str, kind: Kind) -> Result<(String, Option<Cursor>), Failed> {
    if kind != Kind::Lsp {
        return Ok((raw.to_owned(), None));
    }
    let Some(position) = raw.find(CURSOR) else {
        return Err(format!("a language server fixture needs the `{CURSOR}` marker").into());
    };
    let text = format!("{}{}", &raw[..position], &raw[position + CURSOR.len()..]);

    let Some(word) = directive(raw, "lsp") else {
        return Err("a language server fixture needs a `// lsp: <query>` directive".into());
    };
    let Some(query) = lark_lsp::Query::parse(&word) else {
        return Err(format!("`{word}` is not `completion`, `hover`, or `definition`").into());
    };
    let offset = u32::try_from(position).unwrap_or(0);
    Ok((text, Some(Cursor { offset, query })))
}

/// Returns the value of a `// name: value` directive in a fixture.
fn directive(text: &str, name: &str) -> Option<String> {
    let marker = format!("// {name}:");
    text.lines().find_map(|line| {
        let position = line.find(&marker)?;
        Some(line[position + marker.len()..].trim().to_owned())
    })
}

/// Compares one piece of output against the expected file.
fn check_snapshot(fixture: &Fixture, actual: Option<&str>, what: &str) -> Result<(), Failed> {
    let Some(actual) = actual else {
        return Err(format!("the front end produced no {what}").into());
    };
    let Some(path) = fixture.expected.as_deref() else {
        return Err(format!("{} has no expected file", fixture.name).into());
    };
    match compare(path, actual) {
        Verdict::Match | Verdict::Blessed => Ok(()),
        Verdict::Mismatch(report) => Err(report.into()),
    }
}

/// Builds a failure report that shows every diagnostic.
fn report_diagnostics(headline: &str, output: &crate::compiler::Output) -> String {
    format!(
        "{headline}\n{}",
        render_all(&output.diagnostics, &output.sources)
    )
}
