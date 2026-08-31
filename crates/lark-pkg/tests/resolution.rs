//! Version resolution, against an index that the test writes.
//!
//! The resolver reads an index through a trait, so these tests need no git and
//! no network. Every version and every requirement is in the test itself.
//!
//! covers: K-4, K-5, K-6

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use lark_pkg::index::Entry;
use lark_pkg::manifest::{Manifest, Source};
use lark_pkg::resolve::{Error, Reader, Resolution, Resolved, resolve};

/// A commit hash. The value does not matter, only its shape.
fn commit(seed: u8) -> String {
    format!("{seed:02x}").repeat(20)
}

/// An index that the test builds in memory.
#[derive(Default)]
struct FakeIndex {
    entries: BTreeMap<String, Entry>,
    /// What each package depends on, for a transitive test.
    graph: BTreeMap<String, Vec<(String, &'static str)>>,
}

impl FakeIndex {
    /// Adds a package with a list of versions.
    fn package(mut self, name: &str, versions: &[(&str, bool)]) -> Self {
        let mut text = format!("name = \"{name}\"\nrepository = \"https://example.com/{name}\"\n");
        for (index, (version, yanked)) in versions.iter().enumerate() {
            let hash = commit(u8::try_from(index).unwrap_or(0));
            let _ = write!(
                text,
                "\n[[version]]\nversion = \"{version}\"\ncommit = \"{hash}\"\nyanked = {yanked}\n"
            );
        }
        let Ok(entry) = Entry::parse(&text, std::path::Path::new("x.toml")) else {
            panic!("the test wrote an invalid entry:\n{text}");
        };
        self.entries.insert(name.to_owned(), entry);
        self
    }

    /// States that one package depends on another.
    fn depends(mut self, from: &str, on: &str, requirement: &'static str) -> Self {
        self.graph
            .entry(from.to_owned())
            .or_default()
            .push((on.to_owned(), requirement));
        self
    }
}

impl Reader for FakeIndex {
    fn entry(&self, _registry: &str, name: &str) -> Result<Option<Entry>, String> {
        Ok(self.entries.get(name).cloned())
    }

    fn dependencies_of(&self, resolved: &Resolved) -> Result<BTreeMap<String, Source>, String> {
        let mut out = BTreeMap::new();
        for (name, requirement) in self.graph.get(&resolved.name).into_iter().flatten() {
            let Ok(parsed) = requirement.parse() else {
                return Err(format!("`{requirement}` is not a requirement"));
            };
            out.insert(
                name.clone(),
                Source::Registry {
                    registry: "main".to_owned(),
                    requirement: parsed,
                },
            );
        }
        Ok(out)
    }
}

/// Reads a dependency table and resolves it against one index.
fn run(dependencies: &str, index: &FakeIndex) -> Result<Resolution, Error> {
    let text = format!(
        "[registry]\nmain = {{ git = \"https://example.com/index\" }}\n\n\
         [dependencies]\n{dependencies}"
    );
    let Ok(manifest) = toml::from_str::<Manifest>(&text) else {
        panic!("the test wrote an invalid manifest:\n{text}");
    };
    let Ok(roots) = manifest.sources() else {
        panic!("the sources did not resolve");
    };
    let registries: BTreeSet<String> = manifest.registry.keys().cloned().collect();
    resolve(&roots, &registries, index)
}

/// Returns the version of one package, as text.
fn version_of(found: &Resolution, name: &str) -> Option<String> {
    found
        .packages
        .iter()
        .find(|package| package.name == name)
        .and_then(|package| package.version.as_ref())
        .map(ToString::to_string)
}

/// Rule K-4. The highest version that the requirement accepts.
#[test]
fn a_range_picks_the_highest_version_that_matches() {
    let index = FakeIndex::default().package(
        "json",
        &[
            ("1.0.0", false),
            ("1.2.0", false),
            ("1.4.3", false),
            ("2.0.0", false),
        ],
    );
    let Ok(found) = run("json = \"1.0\"\n", &index) else {
        panic!("the resolution failed");
    };
    assert_eq!(found.packages.len(), 1);
    assert_eq!(version_of(&found, "json"), Some("1.4.3".to_owned()));
    // The commit comes from the index, not from a tag.
    assert!(found.packages[0].commit.is_some());
    assert_eq!(
        found.packages[0].repository.as_deref(),
        Some("https://example.com/json")
    );
}

/// An exact requirement takes that version and no other.
#[test]
fn an_exact_requirement_pins_one_version() {
    let index = FakeIndex::default().package(
        "json",
        &[("1.0.0", false), ("1.2.0", false), ("1.4.0", false)],
    );
    let Ok(found) = run("json = \"=1.2.0\"\n", &index) else {
        panic!("the resolution failed");
    };
    assert_eq!(version_of(&found, "json"), Some("1.2.0".to_owned()));
}

/// Rule K-6. A yanked version never resolves fresh.
#[test]
fn a_yanked_version_is_not_chosen() {
    let index = FakeIndex::default().package("json", &[("1.0.0", false), ("1.2.0", true)]);
    let Ok(found) = run("json = \"1\"\n", &index) else {
        panic!("the resolution failed");
    };
    assert_eq!(
        version_of(&found, "json"),
        Some("1.0.0".to_owned()),
        "the yanked version must not be chosen"
    );
}

/// Rule K-4. Two requirements for one package must both hold.
#[test]
fn two_requirements_both_hold() {
    let index = FakeIndex::default()
        .package(
            "json",
            &[("1.0.0", false), ("1.2.0", false), ("1.9.0", false)],
        )
        .package("http", &[("0.4.0", false)])
        .depends("http", "json", "<=1.2.0");

    let Ok(found) = run("json = \"1\"\nhttp = \"0.4\"\n", &index) else {
        panic!("the resolution failed");
    };
    assert_eq!(
        version_of(&found, "json"),
        Some("1.2.0".to_owned()),
        "the second requirement caps the choice: {:?}",
        found.packages
    );
}

/// Rule K-4. When nothing satisfies every requirement, the error names each
/// one and the path that asked.
#[test]
fn an_impossible_pair_names_every_requirement() {
    let index = FakeIndex::default()
        .package("json", &[("1.0.0", false), ("2.0.0", false)])
        .package("http", &[("0.4.0", false)])
        .depends("http", "json", "^2");

    let Err(error) = run("json = \"=1.0.0\"\nhttp = \"0.4\"\n", &index) else {
        panic!("two incompatible requirements must not resolve");
    };
    let Error::NoVersion { name, demands, .. } = &error else {
        panic!("expected a version conflict, got {error}");
    };
    assert_eq!(name, "json");
    assert_eq!(demands.len(), 2, "both requirements must appear");

    let text = error.to_string();
    assert!(text.contains("this project"), "{text}");
    assert!(text.contains("http"), "{text}");
    assert!(text.contains("=1.0.0"), "{text}");
}

/// A transitive dependency joins the graph.
#[test]
fn a_transitive_dependency_is_resolved() {
    let index = FakeIndex::default()
        .package("http", &[("0.4.0", false)])
        .package("json", &[("1.0.0", false)])
        .package("bytes", &[("2.0.0", false)])
        .depends("http", "json", "1")
        .depends("json", "bytes", "2");

    let Ok(found) = run("http = \"0.4\"\n", &index) else {
        panic!("the resolution failed");
    };
    let names: Vec<&str> = found
        .packages
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, vec!["bytes", "http", "json"]);
}

/// Rule K-5. A tag can name a different commit later, so the build warns.
#[test]
fn a_moving_reference_is_reported() {
    let index = FakeIndex::default();
    let Ok(found) = run(
        "zlib = { git = \"https://example.com/zlib\", tag = \"v2.1\" }\n\
         png = { git = \"https://example.com/png\", rev = \"9c1f2ab\" }\n",
        &index,
    ) else {
        panic!("the resolution failed");
    };
    assert_eq!(
        found.moving,
        vec!["zlib".to_owned()],
        "a tag moves, a commit does not"
    );
}

/// A package that no index lists names the index that was read.
#[test]
fn a_package_the_index_does_not_list_is_an_error() {
    let index = FakeIndex::default();
    let Err(error) = run("json = \"1\"\n", &index) else {
        panic!("a missing package must not resolve");
    };
    assert!(matches!(error, Error::NotInIndex { .. }));
    assert!(error.to_string().contains("main"), "{error}");
}

/// A dependency that names an undeclared index is an error, not a guess.
#[test]
fn an_undeclared_registry_is_an_error() {
    let index = FakeIndex::default().package("json", &[("1.0.0", false)]);
    let Err(error) = run("json = { version = \"1\", registry = \"other\" }\n", &index) else {
        panic!("an undeclared index must not resolve");
    };
    assert!(matches!(error, Error::UnknownRegistry { .. }));
}

/// Rule K-4. One package reads from one source.
#[test]
fn one_package_from_two_sources_is_an_error() {
    let index = FakeIndex::default()
        .package("http", &[("0.4.0", false)])
        .package("json", &[("1.0.0", false)])
        .depends("http", "json", "1");

    // The project takes `json` from git, and `http` asks for it from the index.
    let Err(error) = run(
        "http = \"0.4\"\njson = { git = \"https://example.com/json\", tag = \"v1\" }\n",
        &index,
    ) else {
        panic!("two sources for one package must not resolve");
    };
    assert!(matches!(error, Error::ConflictingSources { .. }), "{error}");
    assert!(error.to_string().contains("K-4"), "{error}");
}
