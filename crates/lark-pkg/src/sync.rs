//! One call that turns a manifest into a set of directories on disk.
//!
//! The steps are the same every time.
//!
//! 1. Read `lark.lock`. When it exists, fetch every commit it names and stop.
//!    Rule K-7 makes that path read no index at all.
//! 2. Otherwise clone every index that the manifest names.
//! 3. Resolve. Rule K-4 picks one version per package.
//! 4. Fetch every resolved commit into the store.
//! 5. Write `lark.lock`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::lock::{Lock, Locked};
use crate::manifest::{GitRef, Manifest, Source};
use crate::resolve::{self, Resolved};
use crate::store::{self, IndexReader, Store};

/// What a sync produced.
#[derive(Clone, Debug, Default)]
pub struct Synced {
    /// Every package, with the directory that holds its source.
    pub packages: Vec<Package>,
    /// Whether the run read the lock file rather than an index.
    pub from_lock: bool,
    /// Every reference that can name a different commit later. Rule K-5.
    pub moving: Vec<String>,
}

impl Synced {
    /// Returns every directory that a module search path must hold.
    ///
    /// Rule N-3 searches a path, and a dependency adds one entry to it.
    #[must_use]
    pub fn search_paths(&self) -> Vec<PathBuf> {
        self.packages
            .iter()
            .map(|package| package.directory.clone())
            .collect()
    }
}

/// One package, fetched.
#[derive(Clone, Debug)]
pub struct Package {
    /// The package name.
    pub name: String,
    /// The version, when an index gave one.
    pub version: Option<semver::Version>,
    /// Where the source sits on disk.
    pub directory: PathBuf,
}

/// Why a sync did not finish.
#[derive(Debug)]
pub enum Error {
    /// The manifest names a dependency that does not read.
    Manifest(String),
    /// The resolution failed.
    Resolve(resolve::Error),
    /// A git operation failed.
    Store(store::Error),
    /// The lock file did not read or write.
    Lock(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(message) | Self::Lock(message) => f.write_str(message),
            Self::Resolve(error) => write!(f, "{error}"),
            Self::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

/// Fetches every dependency of a project, and writes the lock file.
///
/// # Errors
///
/// Returns the first failure: a manifest that does not read, a resolution that
/// cannot settle, or a git command that failed.
pub fn sync(project: &Path, manifest: &Manifest, store: &Store) -> Result<Synced, Error> {
    if manifest.dependencies.is_empty() {
        return Ok(Synced::default());
    }

    // Rule K-7. A lock file names every commit, so nothing else is read.
    if let Some(lock) = Lock::read(project).map_err(|error| Error::Lock(error.to_string()))? {
        return from_lock(&lock, store);
    }

    let mut paths = BTreeMap::new();
    for (name, registry) in &manifest.registry {
        let path = store
            .sync_index(&registry.git, registry.branch.as_deref())
            .map_err(Error::Store)?;
        paths.insert(name.clone(), path);
    }

    let roots = manifest
        .sources()
        .map_err(|error| Error::Manifest(error.to_string()))?;
    let registries: BTreeSet<String> = manifest.registry.keys().cloned().collect();
    let reader = FetchingReader {
        inner: IndexReader::new(paths),
        store: store.clone(),
    };
    let resolution = resolve::resolve(&roots, &registries, &reader).map_err(Error::Resolve)?;

    let mut packages = Vec::new();
    let mut locked = Vec::new();
    for resolved in &resolution.packages {
        let (directory, commit) = place(resolved, store, project)?;
        packages.push(Package {
            name: resolved.name.clone(),
            version: resolved.version.clone(),
            directory,
        });
        locked.push(record(resolved, commit));
    }

    Lock::new(locked)
        .write(project)
        .map_err(|error| Error::Lock(error.to_string()))?;

    Ok(Synced {
        packages,
        from_lock: false,
        moving: resolution.moving,
    })
}

/// Fetches every commit that a lock file names.
fn from_lock(lock: &Lock, store: &Store) -> Result<Synced, Error> {
    let mut packages = Vec::new();
    for entry in &lock.packages {
        let directory = match (&entry.path, &entry.repository, &entry.commit) {
            (Some(path), _, _) => path.clone(),
            (_, Some(url), Some(commit)) => store.fetch(url, commit).map_err(Error::Store)?,
            _ => {
                return Err(Error::Lock(format!(
                    "the lock file entry for `{}` names no source",
                    entry.name
                )));
            }
        };
        packages.push(Package {
            name: entry.name.clone(),
            version: entry.version.clone(),
            directory,
        });
    }
    Ok(Synced {
        packages,
        from_lock: true,
        moving: Vec::new(),
    })
}

/// Puts one resolved package on disk, and returns where and at what commit.
fn place(
    resolved: &Resolved,
    store: &Store,
    project: &Path,
) -> Result<(PathBuf, Option<String>), Error> {
    match &resolved.source {
        Source::Path(path) => {
            let full = if path.is_absolute() {
                path.clone()
            } else {
                project.join(path)
            };
            Ok((full, None))
        }
        Source::Git { url, reference } => {
            let commit = match reference {
                GitRef::Rev(value) => value.clone(),
                other => store.resolve_reference(url, other).map_err(Error::Store)?,
            };
            let directory = store.fetch(url, &commit).map_err(Error::Store)?;
            Ok((directory, Some(commit)))
        }
        Source::Registry { .. } => {
            let (Some(url), Some(commit)) = (&resolved.repository, &resolved.commit) else {
                return Err(Error::Lock(format!(
                    "`{}` resolved without a commit",
                    resolved.name
                )));
            };
            let directory = store.fetch(url, commit).map_err(Error::Store)?;
            Ok((directory, Some(commit.clone())))
        }
    }
}

/// Builds the lock entry for one resolved package.
fn record(resolved: &Resolved, commit: Option<String>) -> Locked {
    let source = match &resolved.source {
        Source::Registry { registry, .. } => format!("registry+{registry}"),
        Source::Git { url, .. } => format!("git+{url}"),
        Source::Path(path) => format!("path+{}", path.display()),
    };
    Locked {
        name: resolved.name.clone(),
        version: resolved.version.clone(),
        source,
        repository: resolved.repository.clone(),
        commit,
        path: match &resolved.source {
            Source::Path(path) => Some(path.clone()),
            _ => None,
        },
        dependencies: Vec::new(),
    }
}

/// An index reader that fetches a package before it reads its manifest.
///
/// Rule K-4 needs the dependencies of a package to resolve the graph, and a
/// package states them in its own `lark.toml`. So the resolver has to see the
/// source before it can finish.
struct FetchingReader {
    inner: IndexReader,
    store: Store,
}

impl resolve::Reader for FetchingReader {
    fn entry(&self, registry: &str, name: &str) -> Result<Option<crate::index::Entry>, String> {
        self.inner.entry(registry, name)
    }

    fn dependencies_of(&self, resolved: &Resolved) -> Result<BTreeMap<String, Source>, String> {
        let mut with_directory = resolved.clone();
        if with_directory.directory.is_none()
            && let (Some(url), Some(commit)) = (&resolved.repository, &resolved.commit)
        {
            let path = self
                .store
                .fetch(url, commit)
                .map_err(|error| error.to_string())?;
            with_directory.directory = Some(path);
        }
        self.inner.dependencies_of(&with_directory)
    }
}
