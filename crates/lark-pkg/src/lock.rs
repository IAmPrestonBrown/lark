//! The lock file. What a build actually used.
//!
//! `lark.lock` records the commit that every package resolved to, direct and
//! transitive. A build with a lock file fetches by hash and reads no index.
//! That is what makes a build reproducible, and rule F-2 asks for the same
//! property of the build settings.
//!
//! ```toml
//! version = 1
//!
//! [[package]]
//! name = "json"
//! version = "1.2.0"
//! source = "registry+https://github.com/preston/lark-index"
//! repository = "https://github.com/preston/lark-json"
//! commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"
//! ```

use std::fmt;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

/// The format version of the file. A reader refuses a number it does not know.
pub const FORMAT: u32 = 1;

/// The name of the file, in the project directory.
pub const FILE_NAME: &str = "lark.lock";

/// One resolved package.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Locked {
    /// The package name.
    pub name: String,
    /// The version, when an index gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<Version>,
    /// Where the resolution came from, as text a reader can compare.
    pub source: String,
    /// The git repository that holds the source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// The commit that the build used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// The directory, for a path dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Every package that this one depends on, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

/// The whole lock file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Lock {
    /// The format version.
    #[serde(default)]
    pub version: u32,
    /// Every resolved package, sorted by name.
    #[serde(default, rename = "package")]
    pub packages: Vec<Locked>,
}

/// Why a lock file is not usable.
#[derive(Debug)]
pub enum Error {
    /// The file is not valid TOML, or a field is missing.
    Malformed(String),
    /// The file states a format that this version does not read.
    UnknownFormat(u32),
    /// The file cannot be read or written.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(message) => write!(f, "{FILE_NAME} is not valid: {message}"),
            Self::UnknownFormat(found) => write!(
                f,
                "{FILE_NAME} states format {found}, and this version reads {FORMAT}\n  \
                 delete it and build again to write a new one"
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl Lock {
    /// Builds a lock file from a set of resolved packages.
    #[must_use]
    pub fn new(mut packages: Vec<Locked>) -> Self {
        packages.sort_by(|left, right| left.name.cmp(&right.name));
        Self {
            version: FORMAT,
            packages,
        }
    }

    /// Reads a lock file from text.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not valid, or when it states a format
    /// that this version does not read.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let lock: Self =
            toml::from_str(text).map_err(|error| Error::Malformed(error.to_string()))?;
        if lock.version != FORMAT {
            return Err(Error::UnknownFormat(lock.version));
        }
        Ok(lock)
    }

    /// Reads the lock file of a project, when it has one.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists and does not read.
    pub fn read(project: &Path) -> Result<Option<Self>, Error> {
        let path = project.join(FILE_NAME);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        Self::parse(&text).map(Some)
    }

    /// Writes the lock file of a project.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    pub fn write(&self, project: &Path) -> Result<(), Error> {
        std::fs::write(project.join(FILE_NAME), self.render())?;
        Ok(())
    }

    /// Returns the text of the file.
    ///
    /// The header says what the file is for, because a reader meets it in a
    /// diff more often than in an editor.
    #[must_use]
    pub fn render(&self) -> String {
        let body = toml::to_string_pretty(self).unwrap_or_default();
        format!(
            "# What this build used. Do not edit.\n\
             # A build with this file fetches by commit and reads no index.\n\
             \n\
             {body}"
        )
    }

    /// Returns the entry for one package.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Locked> {
        self.packages.iter().find(|entry| entry.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::{FORMAT, Lock, Locked};

    fn entry(name: &str, version: &str) -> Locked {
        Locked {
            name: name.to_owned(),
            version: version.parse().ok(),
            source: "registry+https://example.com/index".to_owned(),
            repository: Some(format!("https://example.com/{name}")),
            commit: Some("9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99".to_owned()),
            path: None,
            dependencies: Vec::new(),
        }
    }

    /// covers: K-7
    #[test]
    fn a_lock_file_round_trips() {
        let lock = Lock::new(vec![entry("json", "1.2.0"), entry("http", "0.4.1")]);
        let text = lock.render();
        let Ok(read_back) = Lock::parse(&text) else {
            panic!("the file did not read back:\n{text}");
        };
        assert_eq!(read_back, lock);
        // The entries are sorted, so a diff shows only what changed.
        assert_eq!(read_back.packages[0].name, "http");
        assert_eq!(read_back.packages[1].name, "json");
    }

    /// covers: K-7
    #[test]
    fn a_lock_file_names_its_format() {
        let lock = Lock::new(Vec::new());
        assert_eq!(lock.version, FORMAT);
        let text = lock.render().replace("version = 1", "version = 99");
        let Err(error) = Lock::parse(&text) else {
            panic!("an unknown format must not read");
        };
        assert!(error.to_string().contains("99"));
    }

    #[test]
    fn a_lookup_finds_one_package() {
        let lock = Lock::new(vec![entry("json", "1.2.0")]);
        assert!(lock.get("json").is_some());
        assert!(lock.get("missing").is_none());
    }

    #[test]
    fn a_project_with_no_lock_file_yields_nothing() {
        let directory = std::env::temp_dir().join("lark-pkg-no-lock-probe");
        let _ = std::fs::create_dir_all(&directory);
        let _ = std::fs::remove_file(directory.join(super::FILE_NAME));
        assert!(matches!(Lock::read(&directory), Ok(None)));
    }
}
