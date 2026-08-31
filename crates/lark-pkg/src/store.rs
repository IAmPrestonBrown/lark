//! The package store, and the git commands that fill it.
//!
//! A fetched package lives read only under `LARK_HOME`, shared between
//! projects. A project holds no copy of its own.
//!
//! ```text
//! ~/.lark/
//!   index/<host>/<owner>/<repo>/     a clone of an index
//!   store/<host>/<owner>/<repo>/<commit>/   one version of one package
//! ```
//!
//! The manager shells out to `git`, as decision D005 shells out to `cc`. A git
//! implementation in Rust is a dependency far larger than the tool that needs
//! it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::index::Entry;
use crate::manifest::{GitRef, Manifest, Source};
use crate::resolve::{Reader, Resolved};

/// The environment variable that moves the store.
///
/// The test suite sets it, so no test touches a real home directory.
pub const HOME_VARIABLE: &str = "LARK_HOME";

/// Why a store operation failed.
#[derive(Debug)]
pub enum Error {
    /// `git` did not start.
    GitMissing(String),
    /// `git` ran and reported a problem.
    GitFailed {
        /// The command, rendered for a reader.
        command: String,
        /// What git wrote to its error stream.
        message: String,
    },
    /// A file could not be read or written.
    Io(String),
    /// The home directory cannot be found.
    NoHome,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitMissing(what) => write!(f, "cannot run `git`: {what}"),
            Self::GitFailed { command, message } => {
                write!(f, "{command} failed\n{}", message.trim())
            }
            Self::Io(what) => write!(f, "{what}"),
            Self::NoHome => write!(
                f,
                "cannot find a home directory\n  set {HOME_VARIABLE} to a directory to use"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// The directories that hold indexes and packages.
#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Opens the store that `LARK_HOME` names, or the one under the home
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns an error when neither is available.
    pub fn open() -> Result<Self, Error> {
        if let Ok(value) = std::env::var(HOME_VARIABLE)
            && !value.is_empty()
        {
            return Ok(Self::at(PathBuf::from(value)));
        }
        let home = std::env::var("HOME")
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or(Error::NoHome)?;
        Ok(Self::at(PathBuf::from(home).join(".lark")))
    }

    /// Opens a store at one directory.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the root of the store.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the directory that holds a clone of one index.
    #[must_use]
    pub fn index_path(&self, url: &str) -> PathBuf {
        self.root.join("index").join(slug(url))
    }

    /// Returns the directory that holds one version of one package.
    #[must_use]
    pub fn package_path(&self, url: &str, commit: &str) -> PathBuf {
        self.root.join("store").join(slug(url)).join(commit)
    }

    /// Clones or updates the clone of one index.
    ///
    /// # Errors
    ///
    /// Returns an error when `git` does not run or reports a problem.
    pub fn sync_index(&self, url: &str, branch: Option<&str>) -> Result<PathBuf, Error> {
        let path = self.index_path(url);
        if path.join(".git").exists() {
            run(&path, &["fetch", "--quiet", "origin"])?;
            let target =
                branch.map_or_else(|| "origin/HEAD".to_owned(), |name| format!("origin/{name}"));
            run(&path, &["reset", "--quiet", "--hard", &target])?;
            return Ok(path);
        }
        create_parent(&path)?;
        let mut arguments = vec!["clone", "--quiet"];
        if let Some(name) = branch {
            arguments.push("--branch");
            arguments.push(name);
        }
        let text = path.to_string_lossy().into_owned();
        arguments.push(url);
        arguments.push(&text);
        run(Path::new("."), &arguments)?;
        Ok(path)
    }

    /// Fetches one commit of one repository into the store.
    ///
    /// A directory that already holds the commit is left alone, so a second
    /// project with the same dependency fetches nothing.
    ///
    /// # Errors
    ///
    /// Returns an error when `git` does not run or reports a problem.
    pub fn fetch(&self, url: &str, commit: &str) -> Result<PathBuf, Error> {
        let path = self.package_path(url, commit);
        if path.join(".lark-fetched").exists() {
            return Ok(path);
        }
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|error| Error::Io(error.to_string()))?;
        }
        create_parent(&path)?;
        let text = path.to_string_lossy().into_owned();
        // A shallow clone of one commit is the smallest fetch that git offers.
        run(Path::new("."), &["init", "--quiet", &text])?;
        run(&path, &["remote", "add", "origin", url])?;
        run(
            &path,
            &["fetch", "--quiet", "--depth", "1", "origin", commit],
        )?;
        run(&path, &["checkout", "--quiet", "FETCH_HEAD"])?;
        std::fs::write(path.join(".lark-fetched"), commit)
            .map_err(|error| Error::Io(error.to_string()))?;
        Ok(path)
    }

    /// Returns the commit that a reference names, without fetching a tree.
    ///
    /// Rule K-5. A tag moves, so a build records what it resolved to.
    ///
    /// # Errors
    ///
    /// Returns an error when `git` does not run, or when the reference names
    /// nothing in the repository.
    pub fn resolve_reference(&self, url: &str, reference: &GitRef) -> Result<String, Error> {
        if let GitRef::Rev(commit) = reference {
            return Ok(commit.clone());
        }
        let output = run(Path::new("."), &["ls-remote", url, reference.text()])?;
        let commit = output
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        if commit.is_empty() {
            return Err(Error::GitFailed {
                command: format!("git ls-remote {url} {}", reference.text()),
                message: format!("`{}` names nothing in {url}", reference.text()),
            });
        }
        Ok(commit)
    }
}

/// Reads index entries from a set of cloned indexes.
pub struct IndexReader {
    /// Where each index lives on disk, by the name the project gave it.
    paths: BTreeMap<String, PathBuf>,
}

impl IndexReader {
    /// Builds a reader over the indexes that a project already cloned.
    #[must_use]
    pub fn new(paths: BTreeMap<String, PathBuf>) -> Self {
        Self { paths }
    }
}

impl Reader for IndexReader {
    fn entry(&self, registry: &str, name: &str) -> Result<Option<Entry>, String> {
        let Some(root) = self.paths.get(registry) else {
            return Err(format!("the index `{registry}` is not cloned"));
        };
        let path = root.join(Entry::path_for(name));
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        Entry::parse(&text, &path)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    fn dependencies_of(&self, resolved: &Resolved) -> Result<BTreeMap<String, Source>, String> {
        // A package states its own dependencies in its `lark.toml`. The caller
        // fetches the package first, so the manifest sits beside it.
        let Some(directory) = &resolved.directory else {
            return Ok(BTreeMap::new());
        };
        let path = directory.join("lark.toml");
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        let manifest: Manifest = toml::from_str(&text).map_err(|error| error.to_string())?;
        manifest.sources().map_err(|error| error.to_string())
    }
}

/// Runs one git command in a directory and returns its output.
fn run(directory: &Path, arguments: &[&str]) -> Result<String, Error> {
    let mut command = Command::new("git");
    command.args(arguments);
    if directory != Path::new(".") {
        command.current_dir(directory);
    }
    let rendered = format!("git {}", arguments.join(" "));
    let output = command
        .output()
        .map_err(|error| Error::GitMissing(error.to_string()))?;
    if !output.status.success() {
        return Err(Error::GitFailed {
            command: rendered,
            message: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Creates the parent directory of a path.
fn create_parent(path: &Path) -> Result<(), Error> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent).map_err(|error| Error::Io(error.to_string()))
}

/// Turns a repository url into a directory path.
///
/// `https://github.com/preston/lark-json` becomes
/// `github.com/preston/lark-json`. A character that a file name cannot hold
/// becomes an underscore, so a url of any shape yields a path.
#[must_use]
pub fn slug(url: &str) -> PathBuf {
    let trimmed = url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("git://")
        .trim_start_matches("file://")
        .replace("git@", "")
        .replace(':', "/");
    let mut path = PathBuf::new();
    for part in trimmed.split('/').filter(|part| !part.is_empty()) {
        let safe: String = part
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        path.push(safe);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::{Store, slug};

    #[test]
    fn a_url_becomes_a_path() {
        assert_eq!(
            slug("https://github.com/preston/lark-json"),
            std::path::Path::new("github.com/preston/lark-json")
        );
        // A `.git` suffix and a trailing slash name the same repository.
        assert_eq!(
            slug("https://github.com/preston/lark-json.git/"),
            slug("https://github.com/preston/lark-json")
        );
        // An ssh url gives the same shape.
        assert_eq!(
            slug("git@github.com:preston/lark-json"),
            std::path::Path::new("github.com/preston/lark-json")
        );
    }

    #[test]
    fn a_store_lays_out_its_directories() {
        let store = Store::at(std::path::PathBuf::from("/tmp/lark-home"));
        let index = store.index_path("https://example.com/index");
        assert!(index.ends_with("example.com/index"));
        assert!(index.starts_with("/tmp/lark-home/index"));

        let package = store.package_path("https://example.com/json", "abc123");
        assert!(package.ends_with("example.com/json/abc123"));
        assert!(package.starts_with("/tmp/lark-home/store"));
    }
}
