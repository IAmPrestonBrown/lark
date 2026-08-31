//! The index format. One file per package, in a git repository.
//!
//! An index is the source of truth for what versions a package has. It is a
//! git repository holding one TOML file per package, and each file names the
//! source repository and every published version.
//!
//! ```toml
//! name = "json"
//! repository = "https://github.com/preston/lark-json"
//!
//! [[version]]
//! version = "1.2.0"
//! commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"
//! ```
//!
//! Rule K-3 makes the commit mandatory. A tag moves and a branch moves, so
//! neither is a version. That rule is what makes an index worth having: a
//! direct dependency trusts whoever controls the tag, and an index dependency
//! trusts a hash, which cannot change under it.

use std::fmt;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};

/// The length of a full git object name, in hexadecimal characters.
const COMMIT_LENGTH: usize = 40;

/// One package, as the index records it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Entry {
    /// The package name. It matches the file name.
    pub name: String,
    /// Where the source lives.
    pub repository: String,
    /// Every published version, in the order the file lists them.
    #[serde(default, rename = "version")]
    pub versions: Vec<Release>,
}

/// One published version of a package.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Release {
    /// The semantic version.
    pub version: Version,
    /// The commit that the version names. Rule K-3 makes it mandatory.
    pub commit: String,
    /// Whether the version is withdrawn. Rule K-6.
    #[serde(default)]
    pub yanked: bool,
    /// Why it is withdrawn, for the message that names it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Why an index file is not usable.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The file is not valid TOML, or a field is missing.
    Malformed {
        /// The file that failed.
        path: PathBuf,
        /// What the parser said.
        message: String,
    },
    /// A version names something other than a commit. Rule K-3.
    NotACommit {
        /// The package.
        name: String,
        /// The version whose entry is wrong.
        version: Version,
        /// The text that the entry gave.
        found: String,
    },
    /// Two entries name the same version.
    DuplicateVersion {
        /// The package.
        name: String,
        /// The version that appears twice.
        version: Version,
    },
    /// The file name and the `name` field disagree.
    NameMismatch {
        /// The name inside the file.
        declared: String,
        /// The name that the path gives.
        expected: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { path, message } => {
                write!(
                    f,
                    "{} is not a valid index entry: {message}",
                    path.display()
                )
            }
            Self::NotACommit {
                name,
                version,
                found,
            } => write!(
                f,
                "`{name}` version {version} names `{found}`, which is not a commit\n  \
                 rule K-3. a tag moves and a branch moves, so an index entry \
                 pins a full commit hash"
            ),
            Self::DuplicateVersion { name, version } => {
                write!(f, "`{name}` lists version {version} twice")
            }
            Self::NameMismatch { declared, expected } => write!(
                f,
                "the entry names `{declared}`, and its path names `{expected}`"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl Entry {
    /// Reads one index entry from text.
    ///
    /// # Errors
    ///
    /// Returns an error when the text is not a valid entry, or when a version
    /// names anything other than a full commit hash.
    pub fn parse(text: &str, path: &Path) -> Result<Self, Error> {
        let entry: Self = toml::from_str(text).map_err(|error| Error::Malformed {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        entry.validate()?;
        Ok(entry)
    }

    /// Checks every rule that the format states.
    fn validate(&self) -> Result<(), Error> {
        let mut seen: Vec<&Version> = Vec::new();
        for release in &self.versions {
            if !is_commit(&release.commit) {
                return Err(Error::NotACommit {
                    name: self.name.clone(),
                    version: release.version.clone(),
                    found: release.commit.clone(),
                });
            }
            if seen.contains(&&release.version) {
                return Err(Error::DuplicateVersion {
                    name: self.name.clone(),
                    version: release.version.clone(),
                });
            }
            seen.push(&release.version);
        }
        Ok(())
    }

    /// Returns the release for one version, yanked or not.
    #[must_use]
    pub fn release(&self, version: &Version) -> Option<&Release> {
        self.versions
            .iter()
            .find(|release| &release.version == version)
    }

    /// Returns every version that a build can still resolve to.
    ///
    /// Rule K-6. A yanked version never resolves fresh. A lock file that
    /// already names one keeps working, and the caller looks it up directly.
    pub fn available(&self) -> impl Iterator<Item = &Release> {
        self.versions.iter().filter(|release| !release.yanked)
    }

    /// Returns the path of the entry inside an index, relative to its root.
    ///
    /// The name is spread over two directory levels, so a large index holds no
    /// directory with thousands of files in it.
    ///
    /// | Name | Path |
    /// |---|---|
    /// | `a` | `1/a.toml` |
    /// | `at` | `2/at.toml` |
    /// | `net` | `3/n/net.toml` |
    /// | `json` | `js/on/json.toml` |
    #[must_use]
    pub fn path_for(name: &str) -> PathBuf {
        let lower = name.to_lowercase();
        let file = format!("{lower}.toml");
        match lower.len() {
            0 => PathBuf::from(file),
            1 => PathBuf::from("1").join(file),
            2 => PathBuf::from("2").join(file),
            3 => PathBuf::from("3").join(&lower[..1]).join(file),
            _ => PathBuf::from(&lower[..2]).join(&lower[2..4]).join(file),
        }
    }
}

/// Reports whether text is a full git object name.
///
/// Rule K-3. A short hash is ambiguous, and a tag or a branch moves, so an
/// entry carries the whole thing.
#[must_use]
pub fn is_commit(text: &str) -> bool {
    text.len() == COMMIT_LENGTH && text.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{Entry, Error, is_commit};

    const COMMIT: &str = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99";

    fn parse(text: &str) -> Result<Entry, Error> {
        Entry::parse(text, std::path::Path::new("json.toml"))
    }

    /// covers: K-1, K-3
    #[test]
    fn an_entry_reads_its_versions() {
        let text = format!(
            "name = \"json\"\n\
             repository = \"https://github.com/preston/lark-json\"\n\
             \n\
             [[version]]\n\
             version = \"1.2.0\"\n\
             commit = \"{COMMIT}\"\n"
        );
        let Ok(entry) = parse(&text) else {
            panic!("a valid entry did not read");
        };
        assert_eq!(entry.name, "json");
        assert_eq!(entry.versions.len(), 1);
        assert_eq!(entry.versions[0].commit, COMMIT);
        assert!(!entry.versions[0].yanked);
    }

    /// Rule K-3. A tag is not a version.
    /// covers: K-3
    #[test]
    fn a_version_that_names_a_tag_is_an_error() {
        let text = "name = \"json\"\n\
                    repository = \"https://example.com/json\"\n\
                    \n\
                    [[version]]\n\
                    version = \"1.0.0\"\n\
                    commit = \"v1.0.0\"\n";
        let Err(error) = parse(text) else {
            panic!("a tag must not read as a commit");
        };
        assert!(matches!(error, Error::NotACommit { .. }));
        assert!(error.to_string().contains("K-3"));
    }

    /// A short hash is ambiguous, so it is not a commit either.
    /// covers: K-3
    #[test]
    fn a_short_hash_is_not_a_commit() {
        assert!(is_commit(COMMIT));
        assert!(!is_commit("9c1f2ab"));
        assert!(!is_commit(""));
        assert!(!is_commit("z9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a9"));
    }

    /// covers: K-6
    #[test]
    fn a_yanked_version_is_not_available() {
        let text = format!(
            "name = \"json\"\n\
             repository = \"https://example.com/json\"\n\
             \n\
             [[version]]\n\
             version = \"1.0.0\"\n\
             commit = \"{COMMIT}\"\n\
             yanked = true\n\
             reason = \"the parser accepted a trailing comma\"\n\
             \n\
             [[version]]\n\
             version = \"1.1.0\"\n\
             commit = \"{COMMIT}\"\n"
        );
        let Ok(entry) = parse(&text) else {
            panic!("a valid entry did not read");
        };
        let available: Vec<_> = entry.available().collect();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].version.to_string(), "1.1.0");
        // The yanked one is still there, so a lock file that names it works.
        assert_eq!(entry.versions.len(), 2);
        assert!(entry.versions[0].reason.is_some());
    }

    #[test]
    fn one_version_cannot_appear_twice() {
        let text = format!(
            "name = \"json\"\n\
             repository = \"https://example.com/json\"\n\
             \n\
             [[version]]\n\
             version = \"1.0.0\"\n\
             commit = \"{COMMIT}\"\n\
             \n\
             [[version]]\n\
             version = \"1.0.0\"\n\
             commit = \"{COMMIT}\"\n"
        );
        assert!(matches!(parse(&text), Err(Error::DuplicateVersion { .. })));
    }

    /// The path spreads a name over two levels, so no directory grows huge.
    #[test]
    fn a_name_maps_to_a_path() {
        assert_eq!(Entry::path_for("a"), std::path::Path::new("1/a.toml"));
        assert_eq!(Entry::path_for("at"), std::path::Path::new("2/at.toml"));
        assert_eq!(Entry::path_for("net"), std::path::Path::new("3/n/net.toml"));
        assert_eq!(
            Entry::path_for("json"),
            std::path::Path::new("js/on/json.toml")
        );
        // The lookup ignores case, so two names cannot differ by case alone.
        assert_eq!(Entry::path_for("JSON"), Entry::path_for("json"));
    }
}
