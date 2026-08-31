//! The `lark` command line tool.
//!
//! ```text
//! lark build <file.lark>    resolve, check, emit C, and link
//! lark check <file.lark>    resolve and check, and emit nothing
//! lark emit  <file.lark>    print the emitted C
//!
//! lark add <name>@<version>          add a dependency through the index
//! lark add <git-url> [--tag <tag>]   add a dependency directly
//! lark update [<name>]              refetch and rewrite the lock file
//! lark tree                         print the dependency graph
//! lark vendor                       copy every dependency into ./vendor
//! lark publish                      print the index entry to submit
//!
//! lark fmt <file.lark> ...          rewrite each file in the canonical style
//! lark fmt --check <file.lark> ...  report a file that is not formatted
//!
//! `check`, `build`, and `emit` accept `--section.field=value`, which sets a
//! configuration field for that run. See rule F-1.
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lark_driver::Config;

mod pkg;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    // A package command belongs to a project, not to one module, so it takes
    // no file.
    if pkg::is_package_command(&command) {
        let rest: Vec<String> = args.collect();
        return pkg::run(&command, &rest);
    }
    if command == "fmt" {
        let rest: Vec<String> = args.collect();
        return format_files(&rest);
    }
    // Rule F-1. An argument that starts with `--` sets a configuration field,
    // and the rest is the file.
    let (overrides, rest): (Vec<String>, Vec<String>) =
        args.partition(|item| item.starts_with("--"));
    let Some(file) = rest.into_iter().next() else {
        print_usage();
        return ExitCode::FAILURE;
    };
    let path = PathBuf::from(file);
    let overrides: Vec<String> = overrides
        .iter()
        .map(|item| item.trim_start_matches("--").to_owned())
        .collect();

    let directory = path.parent().unwrap_or(Path::new("."));
    let config = match Config::load_with(directory, &overrides) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("cannot read lark.toml: {error}");
            return ExitCode::FAILURE;
        }
    };

    match command.as_str() {
        "check" => run(lark_driver::check(&path, &config), false),
        "build" => run(lark_driver::build(&path, &config), true),
        "emit" => emit(&path, &config),
        other => {
            eprintln!("unknown command `{other}`");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

/// Prints the result of a pass, and returns the exit code.
fn run(result: Result<lark_driver::Build, lark_driver::BuildError>, linked: bool) -> ExitCode {
    match result {
        Ok(build) => {
            let report = build.report();
            if !report.is_empty() {
                eprint!("{report}");
            }
            if build.failed() {
                return ExitCode::FAILURE;
            }
            if linked && let Some(binary) = &build.binary {
                println!("{}", binary.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Prints the emitted C for every module.
fn emit(path: &Path, config: &Config) -> ExitCode {
    match lark_driver::check(path, config) {
        Ok(build) => {
            let report = build.report();
            if !report.is_empty() {
                eprint!("{report}");
            }
            if build.failed() {
                return ExitCode::FAILURE;
            }
            for output in &build.outputs {
                println!(
                    "/* ==== {} ==== */",
                    lark_codegen::names::header_file(&output.name)
                );
                print!("{}", output.emitted.header);
                println!("/* ==== {}.c ==== */", output.name);
                print!("{}", output.emitted.c);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Prints how to call the tool.
/// Rewrites every named file in the canonical style.
///
/// Rule Z-1. There is one style and nothing to configure. `--check` reports a
/// file that is not formatted and changes nothing, which is what a gate runs.
fn format_files(arguments: &[String]) -> ExitCode {
    let check = arguments.iter().any(|item| item == "--check");
    let files: Vec<&String> = arguments
        .iter()
        .filter(|item| !item.starts_with("--"))
        .collect();
    if files.is_empty() {
        eprintln!("usage: lark fmt [--check] <file.lark> ...");
        return ExitCode::FAILURE;
    }

    let mut changed = 0usize;
    let mut failed = false;
    for name in files {
        let path = PathBuf::from(name);
        let Ok(source) = std::fs::read_to_string(&path) else {
            eprintln!("cannot read {name}");
            failed = true;
            continue;
        };
        let formatted = lark_fmt::format(&source);
        if formatted == source {
            continue;
        }
        changed += 1;
        if check {
            println!("{name}");
            continue;
        }
        if std::fs::write(&path, formatted).is_err() {
            eprintln!("cannot write {name}");
            failed = true;
        }
    }

    if failed {
        return ExitCode::FAILURE;
    }
    if check && changed > 0 {
        eprintln!("{changed} file(s) are not formatted. Run `lark fmt` to fix them.");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn print_usage() {
    eprintln!("usage: lark <build|check|emit> <file.lark>");
    eprintln!("       lark fmt [--check] <file.lark> ...");
    eprintln!("       lark <add|update|tree|vendor|publish> [arguments]");
}
