//! Compiling and linking, one object file per module.
//!
//! A build used to pass every emitted `.c` to one `cc` run that produced the
//! binary. Nothing was reusable: a change to one module recompiled all of
//! them. Now each module compiles to its own object file, and the object goes
//! in the cache under a key that names every input to that compile.
//!
//! Rule Y-5. A cached object is used only when its key matches and every file
//! that the compile read still looks the same. A doubt resolves toward a miss,
//! because a wrong object produces a program that builds and misbehaves.

use std::path::{Path, PathBuf};
use std::process::Command;

use lark_cache::{Cache, Entry, Fingerprint, Key};

use crate::config::Config;
use crate::{BuildError, ModuleOutput};

/// What one compile and link produced.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// How many objects came from the cache.
    pub reused: usize,
    /// How many objects the compiler produced.
    pub compiled: usize,
}

/// Compiles every module to an object file and links the binary.
///
/// # Errors
///
/// Returns [`BuildError::CompileFailed`] when the C compiler rejects a file,
/// and [`BuildError::Io`] when a file operation fails.
pub fn compile_and_link(
    config: &Config,
    outputs: &[ModuleOutput],
    runtime: Option<&Path>,
    binary: &Path,
    sources: &[PathBuf],
    cache: &Cache,
) -> Result<Report, BuildError> {
    let identity = compiler_identity(&config.build.cc);
    let flags = compile_flags(config, runtime, sources);
    let generated = generated_headers(config);

    // Every unit that this build compiles. A module first, then the runtime.
    // The runtime changes only when the collector or the settings change, so
    // it caches the same way a module does.
    let mut units: Vec<PathBuf> = outputs.iter().map(|output| output.c_path.clone()).collect();
    if let Some(path) = runtime {
        units.extend(runtime_sources(config, path)?);
    }

    // Rule Y-6. One unit does not read the output of another, so the compiles
    // run at the same time. The link reads every object, so it waits.
    let plan: Vec<(PathBuf, Key)> = units
        .into_iter()
        .map(|source| {
            let key = object_key(&source, &identity, &flags, &generated);
            (source, key)
        })
        .collect();

    let mut report = Report::default();
    let mut pending = Vec::new();
    let mut objects: Vec<Option<PathBuf>> = Vec::with_capacity(plan.len());

    for (index, (source, key)) in plan.iter().enumerate() {
        if let Some(path) = cache.get(key, "o") {
            objects.push(Some(path));
            report.reused += 1;
        } else {
            objects.push(None);
            pending.push((index, source.clone(), key.clone()));
        }
    }

    let compiled = compile_all(config, &pending, &flags, cache)?;
    for (index, object) in compiled {
        objects[index] = Some(object);
        report.compiled += 1;
    }

    let objects: Vec<PathBuf> = objects.into_iter().flatten().collect();
    link(config, &objects, binary)?;
    Ok(report)
}

/// Returns the key that names one object compile.
///
/// The key holds the source, every generated header of the build, and the
/// settings. A generated header goes in the key rather than in a witness,
/// because the build writes it and a second write inside one second would
/// otherwise look unchanged.
fn object_key(
    source: &Path,
    identity: &str,
    flags: &[String],
    generated: &[(String, String)],
) -> Key {
    let mut print = Fingerprint::new()
        .with("step", "compile")
        .with("cc", identity)
        .with_file("source", source)
        .with_all("flags", flags);
    for (name, text) in generated {
        print = print.with(&format!("header:{name}"), text);
    }
    print.finish()
}

/// Compiles every unit that missed the cache, several at a time.
///
/// Rule Y-6. One unit reads no output of another, so the order does not
/// matter and the runs overlap. The number of threads is the number of
/// processors, because each run is a subprocess that uses one.
fn compile_all(
    config: &Config,
    pending: &[(usize, PathBuf, Key)],
    flags: &[String],
    cache: &Cache,
) -> Result<Vec<(usize, PathBuf)>, BuildError> {
    if pending.len() < 2 {
        let mut out = Vec::new();
        for (index, source, key) in pending {
            out.push((*index, compile_file(config, source, flags, key, cache)?));
        }
        return Ok(out);
    }

    let width = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
    let mut results: Vec<(usize, PathBuf)> = Vec::with_capacity(pending.len());
    let mut failure: Option<BuildError> = None;

    for batch in pending.chunks(width) {
        let outcomes: Vec<Result<(usize, PathBuf), BuildError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|(index, source, key)| {
                    scope.spawn(move || {
                        compile_file(config, source, flags, key, cache)
                            .map(|object| (*index, object))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        Err(BuildError::CompileFailed {
                            command: "cc".to_owned(),
                            output: "a compile thread stopped".to_owned(),
                        })
                    })
                })
                .collect()
        });
        for outcome in outcomes {
            match outcome {
                Ok(value) => results.push(value),
                // Every unit of the batch finishes, so one failure does not
                // leave a half written object behind. The first one reports.
                Err(error) if failure.is_none() => failure = Some(error),
                Err(_) => {}
            }
        }
        if failure.is_some() {
            break;
        }
    }

    match failure {
        Some(error) => Err(error),
        None => Ok(results),
    }
}

/// Compiles one C file to an object, and stores it under a key.
fn compile_file(
    config: &Config,
    source: &Path,
    flags: &[String],
    key: &Key,
    cache: &Cache,
) -> Result<PathBuf, BuildError> {
    // The name carries the key, so two units never write one file. Rule Y-6
    // compiles several at a time, and two modules of different projects can
    // share a stem.
    let object = config
        .build
        .out
        .join(format!("{}-{}.o", stem_of(source), key.short()));
    let depends = object.with_extension("d");

    let mut command = Command::new(&config.build.cc);
    command.args(flags);
    command
        .arg("-c")
        .arg(source)
        .arg("-o")
        .arg(&object)
        .arg("-MD")
        .arg("-MF")
        .arg(&depends);

    let printed = format!(
        "{} -c {} -o {}",
        config.build.cc,
        source.display(),
        object.display()
    );
    let result = command.output()?;
    if !result.status.success() {
        let _ = std::fs::remove_file(&depends);
        return Err(BuildError::CompileFailed {
            command: printed,
            output: String::from_utf8_lossy(&result.stderr).into_owned(),
        });
    }

    // Rule Y-5. Every file that the compile read joins the witness list, so a
    // system header that changes makes the entry a miss.
    //
    // A generated file is left out. The build rewrites it every time, so its
    // timestamp always differs, and its content is already in the key. A
    // witness on it would turn every entry into a miss.
    let watched: Vec<PathBuf> = std::fs::read_to_string(&depends)
        .map_or_else(|_| Vec::new(), |text| dependency_paths(&text))
        .into_iter()
        .filter(|path| !is_generated(path, &config.build.out))
        .collect();
    let _ = std::fs::remove_file(&depends);

    let stored = cache.put_file(key, "o", &object, &Entry::watching(&watched));
    match stored {
        Ok(path) if cache.is_enabled() => Ok(path),
        _ => Ok(object),
    }
}

/// Links every object into the binary.
fn link(config: &Config, objects: &[PathBuf], binary: &Path) -> Result<(), BuildError> {
    let mut command = Command::new(&config.build.cc);
    command.arg("-o").arg(binary);
    for object in objects {
        command.arg(object);
    }
    if config.build.runtime.as_os_str().is_empty() {
        // Nothing extra to link.
    }
    command.arg("-pthread");

    let printed = format!("{} -o {} <objects>", config.build.cc, binary.display());
    let result = command.output()?;
    if result.status.success() {
        return Ok(());
    }
    Err(BuildError::CompileFailed {
        command: printed,
        output: String::from_utf8_lossy(&result.stderr).into_owned(),
    })
}

/// Returns the flags that every compile in this build uses.
fn compile_flags(config: &Config, runtime: Option<&Path>, sources: &[PathBuf]) -> Vec<String> {
    let mut flags = vec![
        format!("-std={}", config.build.std),
        "-Wall".to_owned(),
        "-Wextra".to_owned(),
        "-iquote".to_owned(),
        config.build.out.display().to_string(),
    ];
    // Rule Z-5. A debugger needs the information to name a local, and rule
    // X-3 already maps every line back to the Lark source.
    if config.build.debug {
        flags.push("-g".to_owned());
    }
    // Rule F-5. The level reaches `cc` unchanged, so a project picks whatever
    // its compiler accepts.
    flags.push(format!("-O{}", config.build.opt));
    // Rule X-4b. The source directory comes after the build directory, so a
    // header that the programmer wrote keeps its own name.
    for directory in sources {
        flags.push("-iquote".to_owned());
        flags.push(directory.display().to_string());
    }
    if let Some(path) = runtime {
        flags.push("-I".to_owned());
        flags.push(path.join("include").display().to_string());
        flags.push("-I".to_owned());
        flags.push(path.join("core").display().to_string());
        flags.push("-pthread".to_owned());
    }
    flags
}

/// Returns the runtime sources that a program links.
fn runtime_sources(config: &Config, runtime: &Path) -> Result<Vec<PathBuf>, BuildError> {
    // Rule R-3. A program links exactly one collector, and `gc.strategy` names
    // it. Chapter 10 section 4 lists them.
    let collector = config
        .gc
        .collector_source()
        .map_err(|known| BuildError::UnknownCollector {
            name: config.gc.strategy.clone(),
            known,
        })?;
    Ok(vec![
        runtime.join("core/lark_core.c"),
        runtime.join(collector),
    ])
}

/// Returns every generated header of the build, with its text.
///
/// The list is sorted, so the key does not change with the order of a
/// directory listing.
fn generated_headers(config: &Config) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(&config.build.out) else {
        return Vec::new();
    };
    let mut found: Vec<(String, String)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.to_string_lossy().ends_with(".lark.h"))
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            Some((name, text))
        })
        .collect();
    found.sort();
    found
}

/// Returns a name for the object file of a source.
fn stem_of(source: &Path) -> String {
    source.file_stem().map_or_else(
        || "unit".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Returns the identity of the C compiler, so an upgrade invalidates the cache.
fn compiler_identity(cc: &str) -> String {
    Command::new(cc)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || cc.to_owned(),
            |output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .take(2)
                    .collect()
            },
        )
}

/// Reports whether a path names a file that the build itself wrote.
///
/// The comparison is on the text of the path, because a generated file always
/// sits under the output directory and the compiler reports it the same way
/// the build passed it in.
fn is_generated(path: &Path, out: &Path) -> bool {
    if path.starts_with(out) {
        return true;
    }
    // The compiler can report a relative path where the build gave one.
    let out_text = out.display().to_string();
    let path_text = path.display().to_string();
    !out_text.is_empty() && path_text.starts_with(&out_text)
}

/// Reads the header list that `-MD` wrote.
fn dependency_paths(text: &str) -> Vec<PathBuf> {
    let Some((_, list)) = text.split_once(':') else {
        return Vec::new();
    };
    list.replace("\\\n", " ")
        .split_whitespace()
        .filter(|item| *item != "\\")
        .map(PathBuf::from)
        .collect()
}
