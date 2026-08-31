//! The Lark build pipeline.
//!
//! One call runs every pass in order, as chapter 09 section 6 lists them.
//!
//! 1. Read `lark.toml`.
//! 2. Resolve the root module and everything it imports.
//! 3. Check the types.
//! 4. Emit C and headers.
//! 5. Call the C compiler and the linker.
//!
//! A pass that reports an error stops the build before the emit, so the C
//! compiler never sees output from a program that Lark rejected.

pub mod compile;
pub mod config;
pub mod generics;

use std::path::{Path, PathBuf};

use lark_codegen::{Emitted, Options};
use lark_diag::Diagnostics;
use lark_span::SourceMap;

pub use config::{Config, ConfigError};

/// What one module produced.
#[derive(Clone, Debug)]
pub struct ModuleOutput {
    /// The module name.
    pub name: String,
    /// The emitted C file.
    pub c_path: PathBuf,
    /// The emitted header file.
    pub header_path: PathBuf,
    /// The emitter result.
    pub emitted: Emitted,
}

/// What one build produced.
#[derive(Debug)]
pub struct Build {
    /// Every problem that the passes found.
    pub diagnostics: Diagnostics,
    /// The text of every module, for rendering a diagnostic.
    pub sources: SourceMap,
    /// One entry per module, in load order.
    pub outputs: Vec<ModuleOutput>,
    /// The binary, when the build reached the linker.
    pub binary: Option<PathBuf>,
    /// How many object files came from the cache. See rule Y-5.
    pub reused: usize,
    /// How many object files the compiler produced.
    pub compiled: usize,
}

impl Build {
    /// Reports whether any pass rejected the program.
    pub fn failed(&self) -> bool {
        self.diagnostics.has_errors()
    }

    /// Returns every diagnostic, rendered.
    pub fn report(&self) -> String {
        lark_diag::render_all(&self.diagnostics, &self.sources)
    }
}

/// A failure that is not a diagnostic.
#[derive(Debug)]
pub enum BuildError {
    /// The configuration cannot be read.
    Config(ConfigError),
    /// A file cannot be read or written.
    Io(std::io::Error),
    /// The C compiler rejected the emitted C, or could not run.
    CompileFailed {
        /// The command that ran.
        command: String,
        /// What the compiler printed.
        output: String,
    },
    /// The program uses managed memory, and the runtime is not on disk.
    RuntimeMissing,
    /// `gc.strategy` names no collector that the runtime ships.
    UnknownCollector {
        /// The name that the configuration gave.
        name: String,
        /// Every name that the runtime answers to.
        known: &'static str,
    },
    /// A dependency did not resolve or did not fetch.
    Dependencies(String),
    /// The collector does not accept the configured root mechanism.
    RootsRefused {
        /// The collector that `gc.strategy` names.
        collector: String,
        /// The mechanism that `gc.roots` names.
        roots: String,
    },
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::UnknownCollector { name, known } => write!(
                f,
                "`gc.strategy = \"{name}\"` names no collector\n  \
                 the runtime ships: {known}"
            ),
            Self::Dependencies(message) => write!(f, "{message}"),
            Self::RootsRefused { collector, roots } => write!(
                f,
                "`gc.strategy = \"{collector}\"` does not accept \
                 `gc.roots = \"{roots}\"`\n  \
                 a collector that moves an object must write a new address into \
                 every root,\n  and a conservative scan cannot say which words \
                 are roots\n  set `gc.roots = \"shadow-stack\"`"
            ),
            Self::CompileFailed { command, output } => {
                write!(
                    f,
                    "the C compiler rejected the emitted C\n  {command}\n{output}"
                )
            }
            Self::RuntimeMissing => write!(
                f,
                "this program uses managed memory and needs the runtime\n  \
                 set `build.runtime` in lark.toml, or the LARK_RUNTIME variable"
            ),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<std::io::Error> for BuildError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ConfigError> for BuildError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

/// Runs every pass over a root file, and stops before the emit on an error.
///
/// # Errors
///
/// Returns an error when a file cannot be read or written. A problem in the
/// program itself arrives as a diagnostic in the result, not as an error.
pub fn check(root: &Path, config: &Config) -> Result<Build, BuildError> {
    // A relative search path is relative to the package, not to the file.
    let project = Config::root_for(root.parent().unwrap_or(Path::new(".")));
    let mut search = config.search_paths(&project);
    // Rule K-8. A dependency is a module search root, so `@import json` finds
    // `json.lark` inside the package. Rule N-3 already searches a path.
    let synced = fetch_dependencies(&project, config)?;
    search.extend(synced.search_paths());
    // Rule C-1. A build reads every `#include` through the platform
    // preprocessor, so rule L-15 sees a complete name table.
    // Rule Y-4. The header read survives between builds, keyed on the include
    // lines and validated against the headers themselves.
    let cache = lark_cache::Cache::for_build(&config.build.out);
    let reader = lark_cpp::Reader::with_cache(
        lark_cpp::Options {
            cc: config.build.cc.clone(),
            std: config.build.std.clone(),
            include_dirs: search.clone(),
            defines: Vec::new(),
        },
        cache.clone(),
    );
    let resolution = lark_resolve::resolve_path_with(root, &search, &reader)?;

    // Rule R-1. The checks read the capabilities of the collector that
    // `gc.strategy` names.
    let Some(capabilities) = lark_types::caps::Capabilities::of(&config.gc.strategy) else {
        return Err(BuildError::UnknownCollector {
            name: config.gc.strategy.clone(),
            known: config::Gc::COLLECTORS,
        });
    };
    // Rule R-5. A moving collector accepts rule M-10 shadow stack roots alone.
    if !capabilities.accepts_roots(&config.gc.roots) {
        return Err(BuildError::RootsRefused {
            collector: config.gc.strategy.clone(),
            roots: config.gc.roots.clone(),
        });
    }
    let mut diagnostics = resolution.diagnostics.clone();

    // The instantiations come first, because rule DQ-2 reports the
    // instantiation beside an error that the substituted body caused. A type
    // error inside a generic body needs that answer before it is reported.
    let mut mono_diagnostics = Diagnostics::new();
    let program = lark_mono::collect(&resolution.graph, &mut mono_diagnostics);

    let mut type_diagnostics = lark_types::check_resolution_with(&resolution, capabilities);
    // Rule I-1 needs a whole program, and a build makes one.
    type_diagnostics.extend(lark_types::check_program(&resolution));
    generics::attribute(&mut type_diagnostics, &program);

    diagnostics.extend(type_diagnostics);
    diagnostics.extend(mono_diagnostics);
    diagnostics.sort_by_position();
    if diagnostics.has_errors() {
        return Ok(Build {
            diagnostics,
            sources: resolution.sources,
            outputs: Vec::new(),
            binary: None,
            reused: 0,
            compiled: 0,
        });
    }

    let options = Options {
        roots: lark_codegen::Roots::parse(&config.gc.roots),
        torture: config.gc.torture,
        // Rule R-2. Only a collector that walks part of the heap needs a call
        // for a store of a managed pointer.
        write_barrier: capabilities.write_barrier,
        ..Options::default()
    };
    let out_dir = config.build.out.clone();
    let mut outputs = Vec::new();
    for module in resolution.graph.modules() {
        let Some(emitted) = lark_codegen::emit(&resolution.graph, module.id, &options, &program)
        else {
            continue;
        };
        outputs.push(ModuleOutput {
            name: module.name.clone(),
            c_path: out_dir.join(format!("{}.c", module.name)),
            header_path: out_dir.join(lark_codegen::names::header_file(&module.name)),
            emitted,
        });
    }

    Ok(Build {
        diagnostics,
        sources: resolution.sources,
        outputs,
        binary: None,
        reused: 0,
        compiled: 0,
    })
}

/// Copies the debugger scripts into the output directory.
///
/// Rule Z-5. The scripts read the object header that rule M-4 puts before
/// every payload, so they need no metadata that the compiler does not already
/// emit. A failure to copy one is not a build failure: a program still runs
/// without a debugger.
fn write_debugger_scripts(config: &Config, runtime: &Path) {
    for name in ["lark_lldb.py", "lark_gdb.py"] {
        let source = runtime.join("tools").join(name);
        let Ok(text) = std::fs::read_to_string(&source) else {
            continue;
        };
        let _ = std::fs::write(config.build.out.join(name), text);
    }
}

/// Fetches every dependency of a project.
///
/// A project with no dependency does nothing and touches no store.
///
/// # Errors
///
/// Returns the first failure: a manifest that does not read, a resolution that
/// cannot settle, or a git command that failed.
fn fetch_dependencies(
    project: &Path,
    config: &Config,
) -> Result<lark_pkg::sync::Synced, BuildError> {
    let manifest = config.manifest();
    if manifest.dependencies.is_empty() {
        return Ok(lark_pkg::sync::Synced::default());
    }
    let store = lark_pkg::store::Store::open()
        .map_err(|error| BuildError::Dependencies(error.to_string()))?;
    lark_pkg::sync::sync(project, &manifest, &store)
        .map_err(|error| BuildError::Dependencies(error.to_string()))
}

/// Runs every pass, writes the C, and calls the C compiler.
///
/// # Errors
///
/// Returns an error when a file cannot be written, or when the C compiler
/// rejects the emitted C.
pub fn build(root: &Path, config: &Config) -> Result<Build, BuildError> {
    let cache = lark_cache::Cache::for_build(&config.build.out);
    build_with(root, config, &cache)
}

/// Runs every pass and links, with one cache.
///
/// Rule Y-3. A cache is a saving, never a source of truth.
/// `lark_cache::Cache::disabled` gives a build that does every step.
///
/// # Errors
///
/// Returns an error when a file cannot be written, or when the C compiler
/// rejects the emitted C.
pub fn build_with(
    root: &Path,
    config: &Config,
    cache: &lark_cache::Cache,
) -> Result<Build, BuildError> {
    let mut result = check(root, config)?;
    if result.failed() || result.outputs.is_empty() {
        return Ok(result);
    }

    std::fs::create_dir_all(&config.build.out)?;
    // Rule F-2. The settings of the build sit beside the output, so a reader
    // knows what produced it and a second build reproduces it.
    std::fs::write(config.build.out.join(config::RECORD_NAME), config.record())?;
    for output in &result.outputs {
        std::fs::write(&output.header_path, &output.emitted.header)?;
        std::fs::write(&output.c_path, &output.emitted.c)?;
    }

    let stem = root.file_stem().map_or_else(
        || "program".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let binary = config.build.out.join(stem);

    // A program that uses managed memory links the runtime.
    let needs_runtime = result
        .outputs
        .iter()
        .any(|output| output.emitted.uses_runtime);
    let runtime = if needs_runtime {
        let project = Config::root_for(root.parent().unwrap_or(Path::new(".")));
        match config.runtime_path(&project) {
            Some(path) => Some(path),
            None => return Err(BuildError::RuntimeMissing),
        }
    } else {
        None
    };

    // Every directory that holds a source module, so a `#include \"local.h\"`
    // beside a module finds the header the programmer wrote.
    let mut sources: Vec<PathBuf> = result
        .sources
        .files()
        .iter()
        .map(|file| {
            // A relative path with one component has an empty parent, and an
            // empty argument to `-iquote` names nothing. The directory of the
            // file is the working directory in that case.
            match file.path().parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
                _ => PathBuf::from("."),
            }
        })
        .collect();
    sources.sort();
    sources.dedup();

    // Rule Z-5. A program that links the runtime gets the debugger scripts
    // beside it, so a reader loads one by a path that is already open.
    if let Some(path) = runtime.as_deref() {
        write_debugger_scripts(config, path);
    }

    // Rule Y-5. Each module compiles to its own object, and the object comes
    // from the cache when nothing that it reads changed.
    let report = compile::compile_and_link(
        config,
        &result.outputs,
        runtime.as_deref(),
        &binary,
        &sources,
        cache,
    )?;
    result.reused = report.reused;
    result.compiled = report.compiled;
    result.binary = Some(binary);
    Ok(result)
}
