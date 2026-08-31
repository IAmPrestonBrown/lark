//! Reads C headers through the platform preprocessor.
//!
//! Decision D005 keeps a preprocessor out of Lark. The front end calls the
//! platform compiler with `-E` and reads what comes back. Rule C-1 names the
//! step, and rule N-12 gives the resulting names to the module that wrote the
//! directive.
//!
//! The output feeds two consumers. The parser needs the type names, because
//! rule L-6 cannot tell `(name) * x` from a cast without them. The resolver
//! needs every name, because rule L-15 turns a complete table into stricter
//! checks.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use lark_syntax::NoNames;

mod collect;
mod include;
mod reader;

pub use collect::{Decl, Headers};
pub use include::{Include, includes_of};
pub use reader::Reader;

/// How to call the preprocessor.
#[derive(Debug, Clone)]
pub struct Options {
    /// The compiler to run. The `build.cc` setting supplies it.
    pub cc: String,
    /// The language standard, as in `c11`.
    pub std: String,
    /// Directories to search, each passed with `-I`.
    pub include_dirs: Vec<PathBuf>,
    /// Macros to define, each passed with `-D`.
    pub defines: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            cc: "cc".to_owned(),
            std: "c11".to_owned(),
            include_dirs: Vec::new(),
            defines: Vec::new(),
        }
    }
}

/// Why a header set did not load.
#[derive(Debug)]
pub enum Error {
    /// The preprocessor did not start.
    Spawn(String),
    /// The preprocessor ran and reported a problem.
    Failed {
        /// The exit status, rendered.
        status: String,
        /// What the compiler wrote to its error stream.
        message: String,
    },
    /// The output was not valid text.
    Encoding,
    /// A temporary file did not open.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(what) => write!(f, "cannot run the preprocessor: {what}"),
            Self::Failed { status, message } => {
                write!(f, "the preprocessor failed ({status})")?;
                if !message.is_empty() {
                    write!(f, ": {}", message.trim())?;
                }
                Ok(())
            }
            Self::Encoding => f.write_str("the preprocessor wrote text that is not UTF-8"),
            Self::Io(what) => write!(f, "cannot write the preprocessor input: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Reads every header that a source file includes.
///
/// The function gathers the `#include` directives, writes them to one C file,
/// and preprocesses that file. It never preprocesses the Lark source, because
/// Lark syntax is not C and the header names are the only part that matters.
///
/// # Errors
///
/// Returns an error when the preprocessor does not run, or when it reports a
/// problem such as a header that does not exist.
pub fn read(source: &str, base: &Path, options: &Options) -> Result<Headers, Error> {
    read_cached(source, base, options, &lark_cache::Cache::disabled())
}

/// Reads every header, through a cache that survives between builds.
///
/// Rule Y-4. The preprocessor is the slowest step of the front end, and its
/// answer depends on the include lines, the settings, and the header files
/// themselves. The first two go in the key. The third goes in the witness
/// list, so a header that changes on disk makes the entry a miss.
///
/// # Errors
///
/// Returns an error when the preprocessor does not run, or when it reports a
/// problem such as a header that does not exist.
pub fn read_cached(
    source: &str,
    base: &Path,
    options: &Options,
    cache: &lark_cache::Cache,
) -> Result<Headers, Error> {
    let directives = includes_of(source);
    if directives.is_empty() {
        return Ok(Headers::default());
    }

    let key = key_for(&directives, base, options);
    if let Some(path) = cache.get(&key, "i")
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        return Ok(collect_from(&text));
    }

    let (text, headers_read) = preprocess_with_dependencies(&directives, base, options)?;
    let entry = lark_cache::Entry::watching(&headers_read);
    let _ = cache.put(&key, "i", text.as_bytes(), &entry);
    Ok(collect_from(&text))
}

/// Returns the key that names one header read.
fn key_for(directives: &[Include], base: &Path, options: &Options) -> lark_cache::Key {
    let neighbour = neighbour_directory(base);
    lark_cache::Fingerprint::new()
        .with("step", "preprocess")
        .with("cc", &options.cc)
        .with("std", &options.std)
        .with_all("include", directives.iter().map(|item| item.text.as_str()))
        .with_all(
            "dir",
            options
                .include_dirs
                .iter()
                .map(|path| path.display().to_string()),
        )
        .with_all("define", options.defines.iter().map(String::as_str))
        .with("neighbour", &neighbour.display().to_string())
        .finish()
}

/// Collects the names that a preprocessed translation unit declares.
///
/// The text must already be free of directives. The caller normally gets it
/// from [`read`], and a test supplies it directly.
#[must_use]
pub fn collect_from(preprocessed: &str) -> Headers {
    // A preprocessed unit declares every name before it uses the name, so one
    // pass is enough. Rule L-8 needs two passes only for a Lark module, where
    // the order is free.
    let parsed = lark_syntax::parse(preprocessed, &NoNames);
    let mut headers = Headers::default();
    collect::walk(&parsed.syntax(), &mut headers);
    collect::macros(preprocessed, &mut headers);
    headers
}

/// Returns the directory that a quoted include searches first.
///
/// A relative path with one component has an empty parent, and an empty
/// argument to `-iquote` names nothing. The directory of the file is the
/// working directory in that case.
fn neighbour_directory(base: &Path) -> PathBuf {
    match base.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// Runs the preprocessor and reports every header that it read.
///
/// Rule Y-4. The header list goes in the witness of the cache entry, so a
/// header that changes on disk makes the entry a miss. `-MD` writes the list,
/// which is what every build system reads for the same purpose.
fn preprocess_with_dependencies(
    directives: &[Include],
    base: &Path,
    options: &Options,
) -> Result<(String, Vec<PathBuf>), Error> {
    let mut input = String::new();
    for directive in directives {
        input.push_str(&directive.text);
        input.push('\n');
    }

    let path = scratch_path(base);
    std::fs::write(&path, input.as_bytes()).map_err(|e| Error::Io(e.to_string()))?;

    let mut command = Command::new(&options.cc);
    command
        .arg("-E")
        // `-dD` keeps every `#define` in the output. A header names much of its
        // interface with a macro, as `stdout` names `__stdoutp`, so the name
        // table needs the macros as well as the declarations.
        .arg("-dD")
        .arg(format!("-std={}", options.std))
        .arg("-x")
        .arg("c");
    for dir in &options.include_dirs {
        command.arg("-I").arg(dir);
    }
    for define in &options.defines {
        command.arg(format!("-D{define}"));
    }
    // The directory of the source comes first, so a quoted include finds a
    // neighbouring header. Rule N-3 orders a Lark import the same way.
    command.arg("-iquote").arg(neighbour_directory(base));

    // `-MD` writes every header that the run read, beside the output.
    let depends = path.with_extension("d");
    command.arg("-MD").arg("-MF").arg(&depends);
    command.arg(&path);

    let output = command
        .output()
        .map_err(|e| Error::Spawn(format!("{}: {e}", options.cc)));
    let _ = std::fs::remove_file(&path);
    let output = output?;

    let read = std::fs::read_to_string(&depends)
        .map_or_else(|_| Vec::new(), |text| dependency_paths(&text));
    let _ = std::fs::remove_file(&depends);

    if !output.status.success() {
        return Err(Error::Failed {
            status: output.status.to_string(),
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let text = String::from_utf8(output.stdout).map_err(|_| Error::Encoding)?;
    Ok((text, read))
}

/// Reads the header list that `-MD` wrote.
///
/// The file is a make rule: a target, a colon, and then every file that the
/// run read. A long rule wraps with a backslash at the end of a line.
fn dependency_paths(text: &str) -> Vec<PathBuf> {
    let Some((_, list)) = text.split_once(':') else {
        return Vec::new();
    };
    list.replace("\\\n", " ")
        .split_whitespace()
        .filter(|item| *item != "\\")
        .map(PathBuf::from)
        // The generated file itself is not a header, and it is already gone.
        .filter(|path| path.extension().is_some_and(|value| value != "c"))
        .collect()
}

/// Builds a path for the generated C file.
///
/// Two calls can run at the same time, so the name carries a counter as well
/// as the process id. Without the counter one call overwrites the input of
/// another and both read the wrong header set.
fn scratch_path(base: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stem = base
        .file_stem()
        .map_or_else(|| "unit".to_owned(), |s| s.to_string_lossy().into_owned());
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut path = std::env::temp_dir();
    path.push(format!("lark-cpp-{stem}-{}-{serial}.c", std::process::id()));
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_with_no_include_needs_no_preprocessor() {
        let headers = read(
            "int main(void) { return 0; }",
            Path::new("x.lark"),
            &Options::default(),
        );
        assert!(headers.is_ok());
        assert!(headers.unwrap_or_default().is_empty());
    }

    #[test]
    fn a_typedef_becomes_a_type_name() {
        let headers = collect_from("typedef unsigned long size_t;");
        assert!(headers.is_type("size_t"));
        assert!(!headers.is_value("size_t"));
    }

    #[test]
    fn a_function_becomes_a_value_name() {
        let headers = collect_from("int printf(const char *, ...);");
        assert!(headers.is_value("printf"));
        assert!(!headers.is_type("printf"));
    }
}
