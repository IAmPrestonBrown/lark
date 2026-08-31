//! Version resolution. One version of one package per build.
//!
//! Rule K-4. The graph is flat. Every requirement for one package must hold at
//! the same time, and the resolver picks the highest version that satisfies
//! them all. If none does, the error names every requirement and the path that
//! asked for it, because that is the only way a reader can act on it.
//!
//! The resolver reads an index through a trait, so its tests need no git and
//! no network. `lark_pkg::store` supplies the real reader.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use semver::{Version, VersionReq};

use crate::index::Entry;
use crate::manifest::{GitRef, Source};

/// Where a requirement came from.
///
/// The root is the project itself. Every other path names the chain of
/// packages that asked, so an error says who wants what.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Origin(Vec<String>);

impl Origin {
    /// Returns the origin of a requirement that the project itself states.
    #[must_use]
    pub fn root() -> Self {
        Self(Vec::new())
    }

    /// Returns the origin of a requirement that one package states.
    #[must_use]
    pub fn through(&self, name: &str) -> Self {
        let mut path = self.0.clone();
        path.push(name.to_owned());
        Self(path)
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("this project");
        }
        f.write_str(&self.0.join(" -> "))
    }
}

/// One requirement, with the path that stated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Demand {
    /// What the requirement asks for.
    pub requirement: VersionReq,
    /// Who asked.
    pub origin: Origin,
}

/// One package, resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// The package name.
    pub name: String,
    /// The version, when an index gave one.
    pub version: Option<Version>,
    /// Where the resolution came from.
    pub source: Source,
    /// The repository, for a fetched package.
    pub repository: Option<String>,
    /// The commit, for a fetched package.
    pub commit: Option<String>,
    /// Where the source sits on disk, once the caller fetched it.
    ///
    /// The resolver leaves this empty. A reader that fetches fills it in, so
    /// that `dependencies_of` can read the manifest of the package.
    pub directory: Option<PathBuf>,
}

/// Why a resolution did not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// No version satisfies every requirement.
    NoVersion {
        /// The package.
        name: String,
        /// Every requirement, with who asked.
        demands: Vec<Demand>,
        /// Every version that the index lists and no requirement matched.
        available: Vec<Version>,
    },
    /// The index has no entry for the package.
    NotInIndex {
        /// The package.
        name: String,
        /// The index that was read.
        registry: String,
    },
    /// A project names an index that `[registry]` does not declare.
    UnknownRegistry {
        /// The package that named it.
        name: String,
        /// The index name that no entry declares.
        registry: String,
    },
    /// The resolution did not settle.
    ///
    /// Every requirement narrows the choice, so a graph settles in at most one
    /// step per requirement. A graph that does not is a defect in the
    /// resolver, and the bound turns a hang into a message.
    DidNotSettle {
        /// How many steps ran.
        steps: usize,
    },
    /// Two entries give the same package two different sources.
    ConflictingSources {
        /// The package.
        name: String,
        /// The first source, rendered.
        first: String,
        /// The second source, rendered.
        second: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVersion {
                name,
                demands,
                available,
            } => {
                writeln!(f, "no version of `{name}` satisfies every requirement")?;
                for demand in demands {
                    writeln!(f, "  {} asks for {}", demand.origin, demand.requirement)?;
                }
                if available.is_empty() {
                    write!(f, "  the index lists no version that a build can use")
                } else {
                    let list: Vec<String> = available.iter().map(ToString::to_string).collect();
                    write!(f, "  the index lists {}", list.join(", "))
                }
            }
            Self::DidNotSettle { steps } => write!(
                f,
                "the resolution did not settle after {steps} steps\n  \
                 this is a defect in the resolver, not in the project"
            ),
            Self::NotInIndex { name, registry } => {
                write!(f, "the index `{registry}` has no package named `{name}`")
            }
            Self::UnknownRegistry { name, registry } => write!(
                f,
                "`{name}` reads the index `{registry}`, which `[registry]` does not declare"
            ),
            Self::ConflictingSources {
                name,
                first,
                second,
            } => write!(
                f,
                "`{name}` comes from two sources at once\n  {first}\n  {second}\n  \
                 rule K-4 allows one version of one package per build"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Reads an index. The resolver asks for an entry and gets one, or nothing.
pub trait Reader {
    /// Returns the entry for a package in one index.
    ///
    /// # Errors
    ///
    /// Returns a message when the index cannot be read at all. A package that
    /// the index does not list is `Ok(None)`, not an error.
    fn entry(&self, registry: &str, name: &str) -> Result<Option<Entry>, String>;

    /// Returns the dependencies that one resolved package states.
    ///
    /// A package with no manifest of its own has none. The default returns
    /// none, so a test that resolves a flat graph needs no answer.
    ///
    /// # Errors
    ///
    /// Returns a message when the package cannot be read.
    fn dependencies_of(&self, _resolved: &Resolved) -> Result<BTreeMap<String, Source>, String> {
        Ok(BTreeMap::new())
    }
}

/// What a resolution produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    /// Every package, sorted by name.
    pub packages: Vec<Resolved>,
    /// Every reference that can name a different commit later. Rule K-5.
    pub moving: Vec<String>,
}

/// Resolves a dependency graph, starting from the roots of one project.
///
/// # Errors
///
/// Returns the first package whose requirements cannot all hold, or whose
/// index cannot answer.
pub fn resolve(
    roots: &BTreeMap<String, Source>,
    registries: &BTreeSet<String>,
    reader: &dyn Reader,
) -> Result<Resolution, Error> {
    let mut demands: BTreeMap<String, Vec<Demand>> = BTreeMap::new();
    let mut sources: BTreeMap<String, (Source, Origin)> = BTreeMap::new();
    let mut pending: Vec<(String, Source, Origin)> = Vec::new();
    let mut done: BTreeMap<String, Resolved> = BTreeMap::new();
    let mut moving = Vec::new();

    for (name, source) in roots {
        pending.push((name.clone(), source.clone(), Origin::root()));
    }

    // Every requirement narrows the choice, so the loop settles. The bound
    // turns a defect into a message rather than a hang.
    let limit = 64 * (roots.len() + 1) * (roots.len() + 1) + 1024;
    let mut steps = 0usize;

    while let Some((name, source, origin)) = pending.pop() {
        steps += 1;
        if steps > limit {
            return Err(Error::DidNotSettle { steps });
        }
        // Rule K-4. One package reads from one source.
        if let Some((first, _)) = sources.get(&name) {
            if !same_source(first, &source) {
                return Err(Error::ConflictingSources {
                    name,
                    first: render(first),
                    second: render(&source),
                });
            }
        } else {
            sources.insert(name.clone(), (source.clone(), origin.clone()));
        }

        if let Source::Registry {
            registry,
            requirement,
        } = &source
        {
            if !registries.contains(registry) {
                return Err(Error::UnknownRegistry {
                    name,
                    registry: registry.clone(),
                });
            }
            demands.entry(name.clone()).or_default().push(Demand {
                requirement: requirement.clone(),
                origin: origin.clone(),
            });
        }
        if let Source::Git { reference, .. } = &source
            && reference.moves()
            && !moving.contains(&name)
        {
            moving.push(name.clone());
        }

        let resolved = pick(&name, &source, demands.get(&name), registries, reader)?;
        let dependencies = reader
            .dependencies_of(&resolved)
            .unwrap_or_else(|_| BTreeMap::new());
        let changed = done
            .insert(name.clone(), resolved)
            .is_none_or(|previous| previous.version != done[&name].version);

        // A package whose choice changed makes its dependants ask again, and a
        // fresh package brings its own requirements in.
        if changed {
            let through = origin.through(&name);
            for (child, child_source) in dependencies {
                pending.push((child, child_source, through.clone()));
            }
        }
    }

    let mut packages: Vec<Resolved> = done.into_values().collect();
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    moving.sort();
    Ok(Resolution { packages, moving })
}

/// Chooses one version for one package.
fn pick(
    name: &str,
    source: &Source,
    demands: Option<&Vec<Demand>>,
    registries: &BTreeSet<String>,
    reader: &dyn Reader,
) -> Result<Resolved, Error> {
    let Source::Registry { registry, .. } = source else {
        // A git or a path dependency states its own answer.
        let (repository, commit) = match source {
            Source::Git { url, reference } => (
                Some(url.clone()),
                match reference {
                    GitRef::Rev(commit) => Some(commit.clone()),
                    _ => None,
                },
            ),
            _ => (None, None),
        };
        let directory = match source {
            Source::Path(path) => Some(path.clone()),
            _ => None,
        };
        return Ok(Resolved {
            name: name.to_owned(),
            version: None,
            source: source.clone(),
            repository,
            commit,
            directory,
        });
    };
    let _ = registries;

    let entry = reader
        .entry(registry, name)
        .map_err(|_| Error::NotInIndex {
            name: name.to_owned(),
            registry: registry.clone(),
        })?
        .ok_or_else(|| Error::NotInIndex {
            name: name.to_owned(),
            registry: registry.clone(),
        })?;

    let empty = Vec::new();
    let wanted = demands.unwrap_or(&empty);
    // Rule K-4. The highest version that every requirement accepts.
    let best = entry
        .available()
        .filter(|release| {
            wanted
                .iter()
                .all(|demand| demand.requirement.matches(&release.version))
        })
        .max_by(|left, right| left.version.cmp(&right.version));

    let Some(release) = best else {
        return Err(Error::NoVersion {
            name: name.to_owned(),
            demands: wanted.clone(),
            available: entry.available().map(|item| item.version.clone()).collect(),
        });
    };

    Ok(Resolved {
        name: name.to_owned(),
        version: Some(release.version.clone()),
        source: source.clone(),
        repository: Some(entry.repository.clone()),
        commit: Some(release.commit.clone()),
        directory: None,
    })
}

/// Reports whether two sources name the same place.
///
/// Two registry entries agree whenever they read the same index, because the
/// requirements then combine. Every other pair must match exactly.
fn same_source(left: &Source, right: &Source) -> bool {
    match (left, right) {
        (
            Source::Registry {
                registry: first, ..
            },
            Source::Registry {
                registry: second, ..
            },
        ) => first == second,
        _ => left == right,
    }
}

/// Renders a source for a message.
fn render(source: &Source) -> String {
    match source {
        Source::Registry { registry, .. } => format!("the index `{registry}`"),
        Source::Git { url, reference } => format!("the repository {url} at {}", reference.text()),
        Source::Path(path) => format!("the directory {}", path.display()),
    }
}
