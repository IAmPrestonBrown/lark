//! The dependency section of `lark.toml`.
//!
//! A project names a dependency in one of three ways. Rule K-2 allows all
//! three in one project.
//!
//! ```toml
//! [registry]
//! main = { git = "https://github.com/preston/lark-index" }
//!
//! [dependencies]
//! json = "1.2.0"                                            # through an index
//! http = { version = "^0.4", registry = "main" }             # through an index
//! zlib = { git = "https://example.com/zlib", tag = "v2.1" }  # direct
//! local = { path = "../lark-http" }                          # on disk
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use semver::VersionReq;
use serde::{Deserialize, Serialize};

/// The name that a project uses when it names no registry.
pub const DEFAULT_REGISTRY: &str = "main";

/// One index that a project reads.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Registry {
    /// The git repository that holds the index.
    pub git: String,
    /// The branch to read. The default is whatever the remote points at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// How a project names one dependency.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Dependency {
    /// A version requirement alone, read through the default registry.
    Version(VersionReq),
    /// Every other form.
    Detailed(Detailed),
}

/// A dependency with more than a version requirement.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Detailed {
    /// The version requirement, for an index dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionReq>,
    /// The index to read. The default is [`DEFAULT_REGISTRY`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    /// A git repository, for a direct dependency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    /// The tag to read from that repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// The branch to read from that repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The commit to read from that repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// A directory on disk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

/// What a dependency resolves through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// An index gives the versions. Rule K-4 resolves the range against it.
    Registry {
        /// The name of the index in `[registry]`.
        registry: String,
        /// What the project asked for.
        requirement: VersionReq,
    },
    /// A git repository, with no index between.
    Git {
        /// The repository.
        url: String,
        /// What to read from it.
        reference: GitRef,
    },
    /// A directory on disk. It is never fetched and never locked.
    Path(PathBuf),
}

/// What a direct dependency reads from its repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitRef {
    /// A tag. Rule K-5 warns, because a tag moves.
    Tag(String),
    /// A branch. Rule K-5 warns, because a branch moves.
    Branch(String),
    /// A commit. It never moves.
    Rev(String),
    /// Whatever the remote points at. Rule K-5 warns.
    Default,
}

impl GitRef {
    /// Reports whether the reference can name a different commit later.
    ///
    /// Rule K-5. A build that depends on one warns once, because it is not
    /// reproducible without the lock file.
    #[must_use]
    pub const fn moves(&self) -> bool {
        !matches!(self, Self::Rev(_))
    }

    /// Returns the text that `git` reads.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Tag(name) | Self::Branch(name) | Self::Rev(name) => name,
            Self::Default => "HEAD",
        }
    }
}

/// Why a dependency entry is not usable.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The entry names two sources at once.
    Conflicting {
        /// The package.
        name: String,
        /// The two fields that disagree.
        fields: String,
    },
    /// The entry names no source at all.
    Empty {
        /// The package.
        name: String,
    },
    /// A git entry names more than one reference.
    TooManyReferences {
        /// The package.
        name: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflicting { name, fields } => write!(
                f,
                "`{name}` names {fields} at once\n  a dependency reads from one \
                 source: an index, a git repository, or a path"
            ),
            Self::Empty { name } => write!(
                f,
                "`{name}` names no source\n  give a version, a `git` url, or a `path`"
            ),
            Self::TooManyReferences { name } => write!(
                f,
                "`{name}` names more than one of `tag`, `branch`, and `rev`"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl Dependency {
    /// Returns what the dependency reads from.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry names two sources, names none, or names
    /// more than one git reference.
    pub fn source(&self, name: &str) -> Result<Source, Error> {
        let detail = match self {
            Self::Version(requirement) => {
                return Ok(Source::Registry {
                    registry: DEFAULT_REGISTRY.to_owned(),
                    requirement: requirement.clone(),
                });
            }
            Self::Detailed(detail) => detail,
        };

        let has_git = detail.git.is_some();
        let has_path = detail.path.is_some();
        let has_version = detail.version.is_some() || detail.registry.is_some();

        if has_git && has_path {
            return Err(Error::Conflicting {
                name: name.to_owned(),
                fields: "`git` and `path`".to_owned(),
            });
        }
        if has_path && has_version {
            return Err(Error::Conflicting {
                name: name.to_owned(),
                fields: "`path` and a version".to_owned(),
            });
        }
        if has_git && detail.version.is_some() {
            return Err(Error::Conflicting {
                name: name.to_owned(),
                fields: "`git` and `version`".to_owned(),
            });
        }

        if let Some(path) = &detail.path {
            return Ok(Source::Path(path.clone()));
        }
        if let Some(url) = &detail.git {
            let named = usize::from(detail.tag.is_some())
                + usize::from(detail.branch.is_some())
                + usize::from(detail.rev.is_some());
            if named > 1 {
                return Err(Error::TooManyReferences {
                    name: name.to_owned(),
                });
            }
            let reference = if let Some(tag) = &detail.tag {
                GitRef::Tag(tag.clone())
            } else if let Some(branch) = &detail.branch {
                GitRef::Branch(branch.clone())
            } else if let Some(rev) = &detail.rev {
                GitRef::Rev(rev.clone())
            } else {
                GitRef::Default
            };
            return Ok(Source::Git {
                url: url.clone(),
                reference,
            });
        }
        if let Some(requirement) = &detail.version {
            return Ok(Source::Registry {
                registry: detail
                    .registry
                    .clone()
                    .unwrap_or_else(|| DEFAULT_REGISTRY.to_owned()),
                requirement: requirement.clone(),
            });
        }
        Err(Error::Empty {
            name: name.to_owned(),
        })
    }
}

/// The dependency sections of one `lark.toml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Manifest {
    /// Every index that the project reads.
    #[serde(default)]
    pub registry: BTreeMap<String, Registry>,
    /// Every dependency, by package name.
    #[serde(default)]
    pub dependencies: BTreeMap<String, Dependency>,
}

impl Manifest {
    /// Returns every dependency with the source it reads from.
    ///
    /// # Errors
    ///
    /// Returns the first entry that names no source or two sources.
    pub fn sources(&self) -> Result<BTreeMap<String, Source>, Error> {
        let mut out = BTreeMap::new();
        for (name, dependency) in &self.dependencies {
            out.insert(name.clone(), dependency.source(name)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{Dependency, Error, GitRef, Manifest, Source};

    fn manifest(text: &str) -> Manifest {
        match toml::from_str(text) {
            Ok(found) => found,
            Err(error) => panic!("the manifest did not read: {error}"),
        }
    }

    /// covers: K-2
    #[test]
    fn a_bare_version_reads_through_the_default_registry() {
        let found = manifest("[dependencies]\njson = \"1.2.0\"\n");
        let Ok(sources) = found.sources() else {
            panic!("the sources did not resolve");
        };
        let Some(Source::Registry {
            registry,
            requirement,
        }) = sources.get("json")
        else {
            panic!("expected a registry source");
        };
        assert_eq!(registry, "main");
        assert!(requirement.matches(&semver::Version::new(1, 2, 3)));
        assert!(!requirement.matches(&semver::Version::new(2, 0, 0)));
    }

    /// covers: K-2
    #[test]
    fn a_git_entry_reads_directly() {
        let found = manifest(
            "[dependencies]\n\
             zlib = { git = \"https://example.com/zlib\", tag = \"v2.1.0\" }\n",
        );
        let Ok(sources) = found.sources() else {
            panic!("the sources did not resolve");
        };
        let Some(Source::Git { url, reference }) = sources.get("zlib") else {
            panic!("expected a git source");
        };
        assert_eq!(url, "https://example.com/zlib");
        assert_eq!(reference, &GitRef::Tag("v2.1.0".to_owned()));
        // Rule K-5. A tag moves, so a build without a lock file warns.
        assert!(reference.moves());
    }

    /// Rule K-5. A commit never moves, so it needs no warning.
    /// covers: K-5
    #[test]
    fn a_commit_reference_does_not_move() {
        assert!(!GitRef::Rev("9c1f2ab".to_owned()).moves());
        assert!(GitRef::Tag("v1".to_owned()).moves());
        assert!(GitRef::Branch("main".to_owned()).moves());
        assert!(GitRef::Default.moves());
    }

    /// covers: K-2
    #[test]
    fn a_path_entry_reads_from_disk() {
        let found = manifest("[dependencies]\nlocal = { path = \"../lark-http\" }\n");
        let Ok(sources) = found.sources() else {
            panic!("the sources did not resolve");
        };
        assert!(matches!(sources.get("local"), Some(Source::Path(_))));
    }

    /// A project reads two indexes, and an entry names which one.
    /// covers: K-1, K-2
    #[test]
    fn an_entry_names_its_registry() {
        let found = manifest(
            "[registry]\n\
             main = { git = \"https://example.com/index\" }\n\
             other = { git = \"https://example.com/other\" }\n\
             \n\
             [dependencies]\n\
             http = { version = \"^0.4\", registry = \"other\" }\n",
        );
        assert_eq!(found.registry.len(), 2);
        let Ok(sources) = found.sources() else {
            panic!("the sources did not resolve");
        };
        let Some(Source::Registry { registry, .. }) = sources.get("http") else {
            panic!("expected a registry source");
        };
        assert_eq!(registry, "other");
    }

    #[test]
    fn an_entry_names_one_source_at_a_time() {
        let both = manifest(
            "[dependencies]\n\
             bad = { git = \"https://example.com/x\", path = \"../x\" }\n",
        );
        assert!(matches!(both.sources(), Err(Error::Conflicting { .. })));

        let empty = Dependency::Detailed(super::Detailed::default());
        assert!(matches!(empty.source("bad"), Err(Error::Empty { .. })));
    }

    #[test]
    fn a_git_entry_names_one_reference() {
        let found = manifest(
            "[dependencies]\n\
             bad = { git = \"https://example.com/x\", tag = \"v1\", rev = \"abc\" }\n",
        );
        assert!(matches!(
            found.sources(),
            Err(Error::TooManyReferences { .. })
        ));
    }
}
