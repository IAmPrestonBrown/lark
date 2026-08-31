//! The build configuration.
//!
//! Chapter 11 of the specification gives every field. The file is `lark.toml`
//! at the root of a package, and a command line flag wins over the file.

use std::path::{Path, PathBuf};

use std::collections::BTreeMap;

use serde::Deserialize;

/// The whole configuration.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The package that the file describes.
    pub package: Package,
    /// How the build runs.
    pub build: Build,
    /// Which collector the program links.
    pub gc: Gc,
    /// Where `@import` looks. See rule N-3.
    pub paths: Paths,
    /// Every index that the project reads. See rule K-1.
    pub registry: BTreeMap<String, lark_pkg::manifest::Registry>,
    /// Every dependency, by package name. See rule K-2.
    pub dependencies: BTreeMap<String, lark_pkg::manifest::Dependency>,
}

impl Config {
    /// Returns the dependency sections, in the form `lark-pkg` reads.
    #[must_use]
    pub fn manifest(&self) -> lark_pkg::manifest::Manifest {
        lark_pkg::manifest::Manifest {
            registry: self.registry.clone(),
            dependencies: self.dependencies.clone(),
        }
    }
}

/// The package section.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Package {
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: String,
}

impl Default for Package {
    fn default() -> Self {
        Self {
            name: "main".to_owned(),
            version: "0.0.0".to_owned(),
        }
    }
}

/// The build section.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Build {
    /// The C compiler that the build calls.
    ///
    /// Rule F-7. The default is `clang` on every platform. One compiler means
    /// one flag dialect, so a build behaves the same everywhere and the
    /// Windows port needs no second set of flags.
    pub cc: String,
    /// The C standard that the output targets.
    pub std: String,
    /// The directory that holds the emitted C and the binary.
    pub out: PathBuf,
    /// Whether the build keeps the generated C.
    pub emit_c: bool,
    /// Whether the C compiler emits debug information.
    ///
    /// Rule Z-5. A debugger needs it to name a local, and rule X-3 already
    /// maps every line back to the Lark source. The default is on, because a
    /// program that cannot be debugged is harder to trust than one that is
    /// larger.
    pub debug: bool,
    /// The directory that holds the runtime, or empty to find it.
    ///
    /// A program that uses managed memory links the runtime. The search order
    /// is this field, the `LARK_RUNTIME` environment variable, and then
    /// `runtime` beside the project.
    pub runtime: PathBuf,
    /// The optimization level that the C compiler uses.
    ///
    /// Rule F-5. The value becomes `-O<level>`, so `cc` decides what each one
    /// means. The default matches `debug`, because a build that a debugger
    /// cannot follow surprises a reader more than a slow one does.
    ///
    /// A level reads naturally as a number, so `opt = 2` and `opt = "2"` both
    /// work. A level like `s` or `fast` needs the quoted form.
    #[serde(deserialize_with = "level")]
    pub opt: String,
}

impl Default for Build {
    fn default() -> Self {
        Self {
            cc: "clang".to_owned(),
            std: "c11".to_owned(),
            out: PathBuf::from("build/"),
            emit_c: true,
            debug: true,
            runtime: PathBuf::new(),
            opt: "0".to_owned(),
        }
    }
}

/// Reads an optimization level written as a string or as a number.
///
/// Rule F-5. `--build.opt=2` on the command line gives an integer, and so does
/// `opt = 2` in the file. Both mean `-O2`.
fn level<'de, D>(reader: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Level {
        Text(String),
        Number(i64),
    }
    match Level::deserialize(reader)? {
        Level::Text(text) => Ok(text),
        Level::Number(value) => Ok(value.to_string()),
    }
}

/// The collector section.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Gc {
    /// The collector that the program links.
    pub strategy: String,
    /// The stack root mechanism.
    pub roots: String,
    /// Whether a cast that adds `gc` carries a runtime check. See rule T-7.
    pub checks: bool,
    /// Whether every safepoint runs a full collection. See rule F-3.
    pub torture: bool,
}

impl Default for Gc {
    fn default() -> Self {
        Self {
            strategy: "precise-marksweep".to_owned(),
            roots: "shadow-stack".to_owned(),
            checks: true,
            torture: false,
        }
    }
}

/// The paths section.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Paths {
    /// Directories that `@import` searches after the importing file's own.
    pub search: Vec<PathBuf>,
}

/// A problem in the configuration.
#[derive(Debug)]
pub enum ConfigError {
    /// The file cannot be read.
    Io(std::io::Error),
    /// The file is not valid TOML, or holds an unknown field.
    Parse(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Parse(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Reads `lark.toml` from a directory.
    ///
    /// A directory with no such file yields the default configuration, so a
    /// single file builds with no setup.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but does not parse.
    pub fn load(directory: &Path) -> Result<Self, ConfigError> {
        Self::load_with(directory, &[])
    }

    /// Reads the configuration for a directory, with command line overrides.
    ///
    /// Rule F-1. An override wins over the file, and it applies even when the
    /// directory holds no `lark.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error when the file does not parse, or when an override
    /// names an unknown field.
    pub fn load_with(directory: &Path, overrides: &[String]) -> Result<Self, ConfigError> {
        let text = match Self::find(directory) {
            Some(path) => match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(error) => return Err(ConfigError::Io(error)),
            },
            None => String::new(),
        };
        Self::parse_with(&text, overrides)
    }

    /// Returns the directory that holds the `lark.toml` for a file.
    ///
    /// The search walks up from the file, so a package works from any
    /// subdirectory.
    #[must_use]
    pub fn root_for(directory: &Path) -> PathBuf {
        match Self::find(directory) {
            Some(path) => path.parent().unwrap_or(directory).to_path_buf(),
            None => directory.to_path_buf(),
        }
    }

    /// Returns the path of the nearest `lark.toml`, from here upward.
    fn find(directory: &Path) -> Option<PathBuf> {
        let mut current = Some(directory);
        while let Some(here) = current {
            let candidate = here.join("lark.toml");
            if candidate.is_file() {
                return Some(candidate);
            }
            current = here.parent();
        }
        None
    }

    /// Reads a configuration from text.
    ///
    /// # Errors
    ///
    /// Returns an error when the text does not parse, or names an unknown
    /// field. Rule F-1 makes an unknown field a problem, not a silent default.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|error| ConfigError::Parse(error.to_string()))
    }

    /// Reads a configuration from text, with command line overrides applied.
    ///
    /// Rule F-1. An override names the same dotted path that the file uses,
    /// and it wins over the file. The merged document goes through the same
    /// deserialization, so an unknown path is the same error as an unknown
    /// field in the file.
    ///
    /// # Errors
    ///
    /// Returns an error when the text does not parse, when an override is not
    /// `name.field=value`, or when the result names an unknown field.
    pub fn parse_with(text: &str, overrides: &[String]) -> Result<Self, ConfigError> {
        if overrides.is_empty() {
            return Self::parse(text);
        }
        let mut document: toml::Table =
            toml::from_str(text).map_err(|error| ConfigError::Parse(error.to_string()))?;
        for entry in overrides {
            apply_override(&mut document, entry)?;
        }
        let text =
            toml::to_string(&document).map_err(|error| ConfigError::Parse(error.to_string()))?;
        Self::parse(&text)
    }

    /// Returns the directory that holds the runtime.
    ///
    /// A build that needs no managed memory never calls this.
    #[must_use]
    pub fn runtime_path(&self, root: &Path) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if !self.build.runtime.as_os_str().is_empty() {
            candidates.push(self.build.runtime.clone());
        }
        if let Ok(value) = std::env::var("LARK_RUNTIME") {
            candidates.push(PathBuf::from(value));
        }
        candidates.push(root.join("runtime"));
        candidates.push(root.join("../runtime"));
        candidates.extend(installed_runtimes());
        candidates
            .into_iter()
            .find(|path| path.join("include/lark_rt.h").is_file())
    }

    /// Returns the search path that rule N-3 uses, relative to a root.
    #[must_use]
    pub fn search_paths(&self, root: &Path) -> Vec<PathBuf> {
        self.paths
            .search
            .iter()
            .map(|entry| {
                if entry.is_absolute() {
                    entry.clone()
                } else {
                    root.join(entry)
                }
            })
            .collect()
    }
}

/// Writes one `section.field=value` override into a document. See rule F-1.
///
/// The value goes through the TOML value parser, so `true` is a boolean and
/// `2` is an integer. A value that parses as nothing becomes a string, which
/// is what a bare word like `semispace` needs.
fn apply_override(document: &mut toml::Table, entry: &str) -> Result<(), ConfigError> {
    let Some((path, text)) = entry.split_once('=') else {
        return Err(ConfigError::Parse(format!(
            "`{entry}` is not `section.field=value`"
        )));
    };
    let mut parts = path.split('.').filter(|part| !part.is_empty()).peekable();

    let mut table = document;
    let mut field = None;
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            field = Some(part.to_owned());
            break;
        }
        let next = table
            .entry(part.to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        let toml::Value::Table(inner) = next else {
            return Err(ConfigError::Parse(format!(
                "`{path}` names a field inside a value that is not a table"
            )));
        };
        table = inner;
    }
    let Some(field) = field else {
        return Err(ConfigError::Parse(format!(
            "`{entry}` names no field to set"
        )));
    };

    // A bare word is a string. Anything the TOML value parser reads keeps its
    // own type, so `debug=false` is a boolean rather than the text "false".
    let value = text
        .parse::<toml::Value>()
        .unwrap_or_else(|_| toml::Value::String(text.to_owned()));
    table.insert(field, value);
    Ok(())
}

/// Returns the runtime directories that sit beside an installed binary.
///
/// A release archive holds `bin/lark` and `runtime/`, so a program that lives
/// outside the source tree still finds the runtime. Without this, a user who
/// installs the archive has to set `LARK_RUNTIME` by hand.
///
/// The list is empty when the path of the running program is not available.
fn installed_runtimes() -> Vec<PathBuf> {
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let Some(directory) = exe.parent() else {
        return Vec::new();
    };
    vec![
        // bin/lark, with runtime/ beside bin/
        directory.join("../runtime"),
        // A prefix install, as in /usr/local/bin and /usr/local/share.
        directory.join("../share/lark/runtime"),
        // The binary and the runtime in one directory.
        directory.join("runtime"),
    ]
}

/// The name of the file that records the settings of a build. See rule F-2.
pub const RECORD_NAME: &str = "lark-build.toml";

impl Config {
    /// Returns the settings of the build, as the file records them.
    ///
    /// Rule F-2. A build writes this beside the emitted C, so a reader knows
    /// which compiler, which standard, which collector, and which root
    /// mechanism produced the output. A build with the same record produces
    /// the same output.
    #[must_use]
    pub fn record(&self) -> String {
        format!(
            "# The settings of the build that produced this directory.\n\
             # Rule F-2. Do not edit. Change `lark.toml` and build again.\n\
             \n\
             [package]\n\
             name = \"{}\"\n\
             version = \"{}\"\n\
             \n\
             [build]\n\
             cc = \"{}\"\n\
             std = \"{}\"\n\
             emit_c = {}\n\
             debug = {}\n\
             opt = \"{}\"\n\
             \n\
             [gc]\n\
             strategy = \"{}\"\n\
             roots = \"{}\"\n\
             checks = {}\n\
             torture = {}\n",
            self.package.name,
            self.package.version,
            self.build.cc,
            self.build.std,
            self.build.emit_c,
            self.build.debug,
            self.build.opt,
            self.gc.strategy,
            self.gc.roots,
            self.gc.checks,
            self.gc.torture,
        )
    }
}

impl Gc {
    /// Returns the source file of the collector, relative to the runtime.
    ///
    /// Chapter 10 section 4 names each collector. A name that no collector
    /// answers to is an error, not a silent fall back to the default.
    ///
    /// # Errors
    ///
    /// Returns the list of known names when the setting names no collector.
    pub fn collector_source(&self) -> Result<&'static str, &'static str> {
        match self.strategy.as_str() {
            "precise-marksweep" => Ok("gc-marksweep/lark_marksweep.c"),
            "arena" => Ok("gc-arena/lark_arena.c"),
            "semispace" => Ok("gc-semispace/lark_semispace.c"),
            "generational" => Ok("gc-generational/lark_generational.c"),
            _ => Err(Self::COLLECTORS),
        }
    }

    /// Every collector name that `gc.strategy` accepts.
    pub const COLLECTORS: &'static str = "precise-marksweep, arena, semispace, generational";
}

#[cfg(test)]
mod tests {
    use super::{Config, Gc};

    /// Rule Z-6. A build emits debug information by default, because a
    /// debugger needs it to name a local.
    /// covers: Z-6
    #[test]
    fn a_build_emits_debug_information_by_default() {
        let config = Config::default();
        assert!(config.build.debug, "debug information must be on");

        // The record names the setting, so a reader sees what produced a
        // directory. Rule F-2.
        assert!(
            config.record().contains("debug = true"),
            "{}",
            config.record()
        );
    }

    /// The record names every setting that changes the output, so a reader
    /// knows what produced a directory and a second build reproduces it.
    /// covers: F-2
    #[test]
    fn the_record_names_every_setting_that_changes_the_output() {
        let mut config = Config::default();
        config.package.name = "demo".to_owned();
        config.gc.strategy = "semispace".to_owned();
        config.gc.torture = true;

        let text = config.record();
        for expected in [
            "name = \"demo\"",
            "cc = \"clang\"",
            "std = \"c11\"",
            "strategy = \"semispace\"",
            "roots = \"shadow-stack\"",
            "torture = true",
        ] {
            assert!(
                text.contains(expected),
                "the record is missing {expected}\n{text}"
            );
        }
        // The record parses as the configuration it describes.
        let parsed: Result<Config, _> = toml::from_str(&text);
        assert!(parsed.is_ok(), "the record does not parse\n{text}");
    }

    /// A program links exactly one collector, and the setting names it.
    /// covers: R-3
    #[test]
    fn every_collector_name_maps_to_a_source_file() {
        for name in ["precise-marksweep", "arena", "semispace", "generational"] {
            let gc = Gc {
                strategy: name.to_owned(),
                ..Gc::default()
            };
            let source = gc.collector_source();
            assert!(source.is_ok(), "{name} names no collector");
            assert!(source.unwrap_or_default().contains("lark_"));
        }
    }

    /// A name that no collector answers to is an error, not a default.
    /// covers: R-3
    #[test]
    fn an_unknown_collector_name_is_an_error() {
        let gc = Gc {
            strategy: "nonsense".to_owned(),
            ..Gc::default()
        };
        let error = gc.collector_source();
        assert!(error.is_err());
        assert!(error.unwrap_err().contains("precise-marksweep"));
    }

    #[test]
    fn a_missing_file_yields_the_defaults() {
        let config = Config::default();
        assert_eq!(config.build.cc, "clang");
        assert_eq!(config.build.std, "c11");
        assert_eq!(config.gc.strategy, "precise-marksweep");
        assert_eq!(config.gc.roots, "shadow-stack");
        assert!(!config.gc.torture);
    }

    #[test]
    fn a_field_in_the_file_wins_over_the_default() {
        let text = "[build]\ncc = \"clang\"\n\n[gc]\nroots = \"conservative\"\ntorture = true\n";
        let Ok(config) = Config::parse(text) else {
            panic!("the fixture must parse");
        };
        assert_eq!(config.build.cc, "clang");
        assert_eq!(config.gc.roots, "conservative");
        assert!(config.gc.torture);
        // A field that the file leaves out keeps its default.
        assert_eq!(config.build.std, "c11");
    }

    /// covers: F-7
    #[test]
    fn the_default_compiler_is_clang() {
        let Ok(config) = Config::parse("") else {
            panic!("an empty file must parse");
        };
        assert_eq!(config.build.cc, "clang");
        // Rule F-2 records it, so a reader knows what produced the output.
        assert!(config.record().contains("cc = \"clang\""));

        // A project that needs another compiler names it.
        let Ok(config) = Config::parse("[build]\ncc = \"gcc\"\n") else {
            panic!("the fixture must parse");
        };
        assert_eq!(config.build.cc, "gcc");
    }

    /// covers: F-5
    #[test]
    fn the_optimization_level_defaults_to_zero_and_reaches_the_record() {
        let Ok(config) = Config::parse("") else {
            panic!("an empty file must parse");
        };
        assert_eq!(config.build.opt, "0");

        let Ok(config) = Config::parse("[build]\nopt = \"2\"\n") else {
            panic!("the fixture must parse");
        };
        assert_eq!(config.build.opt, "2");
        // Rule F-2 records the level, so a reader knows what produced the
        // output and rule Y-2 keys the object file on it.
        assert!(config.record().contains("opt = \"2\""));
    }

    /// covers: F-1
    #[test]
    fn a_command_line_override_wins_over_the_file() {
        let Ok(config) = Config::parse_with(
            "[gc]\nstrategy = \"arena\"\n",
            &["gc.strategy=semispace".to_owned(), "build.opt=2".to_owned()],
        ) else {
            panic!("the fixture must parse");
        };
        assert_eq!(config.gc.strategy, "semispace");
        assert_eq!(config.build.opt, "2");
        // A value that TOML reads keeps its own type.
        let Ok(config) = Config::parse_with("", &["gc.torture=true".to_owned()]) else {
            panic!("the fixture must parse");
        };
        assert!(config.gc.torture);
        // An override that names an unknown field is the same error as an
        // unknown field in the file.
        assert!(Config::parse_with("", &["gc.nonsense=1".to_owned()]).is_err());
        assert!(Config::parse_with("", &["nonsense".to_owned()]).is_err());
    }

    /// covers: F-1
    #[test]
    fn an_unknown_field_is_a_problem() {
        assert!(Config::parse("[build]\nnot_a_field = 1\n").is_err());
        assert!(Config::parse("[not_a_section]\nx = 1\n").is_err());
    }

    /// covers: N-3
    #[test]
    fn a_relative_search_path_joins_the_root() {
        let Ok(config) = Config::parse("[paths]\nsearch = [\"lib\", \"/abs\"]\n") else {
            panic!("the fixture must parse");
        };
        let paths = config.search_paths(std::path::Path::new("/project"));
        assert_eq!(paths[0], std::path::Path::new("/project/lib"));
        assert_eq!(paths[1], std::path::Path::new("/abs"));
    }
}
