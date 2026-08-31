//! Build and run the emitted C.
//!
//! The harness writes the C to a scratch directory, calls the C compiler, runs
//! the binary, and returns what it printed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What a build and run produced.
#[derive(Clone, Debug)]
pub struct RunResult {
    /// Everything the program wrote to standard output.
    pub stdout: String,
    /// Everything the program wrote to standard error.
    pub stderr: String,
    /// The exit code, or `None` when a signal stopped the program.
    pub code: Option<i32>,
}

/// A failure in the build and run path.
#[derive(Debug)]
pub enum ExecError {
    /// The harness cannot write the C file or read a result.
    Io(std::io::Error),
    /// The C compiler rejected the emitted C.
    CompileFailed {
        /// The command that ran.
        command: String,
        /// What the compiler printed.
        output: String,
    },
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::CompileFailed { command, output } => {
                write!(
                    f,
                    "the C compiler rejected the emitted C\n  {command}\n{output}"
                )
            }
        }
    }
}

impl std::error::Error for ExecError {}

impl From<std::io::Error> for ExecError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Returns the C compiler that the harness uses.
///
/// The environment variable `LARK_CC` wins. The default is `cc`.
pub fn c_compiler() -> String {
    std::env::var("LARK_CC").unwrap_or_else(|_| "cc".to_owned())
}

/// Writes every emitted file to a directory, builds them, and runs the binary.
///
/// # Errors
///
/// Returns [`ExecError::CompileFailed`] when the C compiler rejects the input,
/// and [`ExecError::Io`] when a file operation fails.
pub fn build_and_run(
    directory: &Path,
    name: &str,
    files: &[(String, String)],
    runtime: Option<&Path>,
) -> Result<RunResult, ExecError> {
    build_and_run_with(
        directory,
        name,
        files,
        runtime,
        "gc-marksweep/lark_marksweep.c",
    )
}

/// Builds and runs, with one collector source named.
///
/// A program links exactly one collector. Chapter 10 section 4 lists them, and
/// `gc.strategy` names the one that a build uses.
///
/// # Errors
///
/// Returns [`ExecError::CompileFailed`] when the C compiler rejects the input,
/// and [`ExecError::Io`] when a file operation fails.
pub fn build_and_run_with(
    directory: &Path,
    name: &str,
    files: &[(String, String)],
    runtime: Option<&Path>,
    collector: &str,
) -> Result<RunResult, ExecError> {
    build_and_run_full(directory, name, files, runtime, collector, &[])
}

/// Builds and runs, with the directories that hold the source of the fixture.
///
/// Rule X-4b puts a generated header under a distinct name, so a fixture can
/// carry a header of its own. The compiler needs the directory of that header.
///
/// # Errors
///
/// Returns [`ExecError::CompileFailed`] when the C compiler rejects the input,
/// and [`ExecError::Io`] when a file operation fails.
pub fn build_and_run_full(
    directory: &Path,
    name: &str,
    files: &[(String, String)],
    runtime: Option<&Path>,
    collector: &str,
    include_dirs: &[PathBuf],
) -> Result<RunResult, ExecError> {
    // A run starts from an empty directory. A file that an earlier run wrote
    // under a name that this one no longer uses would otherwise stay, and
    // `-iquote` on this directory would find it before the real one.
    if directory.exists() {
        std::fs::remove_dir_all(directory)?;
    }
    std::fs::create_dir_all(directory)?;
    let binary_path = directory.join(name);

    let mut sources = Vec::new();
    for (file_name, text) in files {
        let path = directory.join(file_name);
        std::fs::write(&path, text)?;
        // The emitter names every file itself, so the extension is exact.
        if Path::new(file_name)
            .extension()
            .is_some_and(|value| value == "c")
        {
            sources.push(path);
        }
    }

    let compiler = c_compiler();
    let mut command = Command::new(&compiler);
    command
        .arg("-std=c11")
        .arg("-Wall")
        .arg("-Wextra")
        // `-iquote` applies to a quoted include only, so a module named
        // `pthread` never shadows the system `<pthread.h>`.
        .arg("-iquote")
        .arg(directory)
        .arg("-o")
        .arg(&binary_path);
    // Rule X-4b. A header that the fixture wrote lives beside the fixture, and
    // the generated names never collide with it.
    for directory in include_dirs {
        command.arg("-iquote").arg(directory);
    }
    // A program that uses managed memory links the runtime.
    if let Some(path) = runtime {
        command.arg("-I").arg(path.join("include"));
        command.arg("-I").arg(path.join("core"));
        command.arg("-pthread");
    }
    for source in &sources {
        command.arg(source);
    }
    if let Some(path) = runtime {
        command.arg(path.join("core/lark_core.c"));
        command.arg(path.join(collector));
    }

    let printed = format!(
        "{compiler} -std=c11 -o {} {}",
        binary_path.display(),
        sources
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let build = command.output()?;
    if !build.status.success() {
        return Err(ExecError::CompileFailed {
            command: printed,
            output: String::from_utf8_lossy(&build.stderr).into_owned(),
        });
    }

    let run = Command::new(&binary_path).output()?;
    Ok(RunResult {
        stdout: String::from_utf8_lossy(&run.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&run.stderr).into_owned(),
        code: run.status.code(),
    })
}

/// Returns a scratch directory for one fixture run.
///
/// The directory sits under `target/`, so `cargo clean` removes it.
pub fn scratch_directory(root: &Path, key: &str) -> PathBuf {
    let safe: String = key
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect();
    root.join("target").join("lark-test").join(safe)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{ExecError, build_and_run, scratch_directory};

    /// Scratch files belong under the repository `target/`, so `cargo clean`
    /// removes them.
    fn root() -> PathBuf {
        crate::runner::repository_root()
    }

    #[test]
    fn builds_and_runs_a_program() {
        let directory = scratch_directory(&root(), "harness_self_test_ok");
        let files = [(
            "main.c".to_owned(),
            "#include <stdio.h>\nint main(void) { puts(\"hi\"); return 0; }\n".to_owned(),
        )];
        let result = build_and_run(&directory, "ok", &files, None).unwrap();
        assert_eq!(result.stdout, "hi\n");
        assert_eq!(result.code, Some(0));
    }

    #[test]
    fn reports_the_exit_code() {
        let directory = scratch_directory(&root(), "harness_self_test_code");
        let files = [(
            "main.c".to_owned(),
            "int main(void) { return 3; }\n".to_owned(),
        )];
        let result = build_and_run(&directory, "code", &files, None).unwrap();
        assert_eq!(result.code, Some(3));
    }

    #[test]
    fn reports_a_compiler_failure() {
        let directory = scratch_directory(&root(), "harness_self_test_bad");
        let files = [(
            "main.c".to_owned(),
            "int main(void) { this is not c }\n".to_owned(),
        )];
        let error = build_and_run(&directory, "bad", &files, None).unwrap_err();
        let ExecError::CompileFailed { output, .. } = error else {
            panic!("bad C must fail the build");
        };
        assert!(!output.is_empty());
    }

    #[test]
    fn a_scratch_directory_holds_no_path_separator() {
        let path = scratch_directory(&root(), "ui/group/name");
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        assert_eq!(name, "ui_group_name");
    }
}
