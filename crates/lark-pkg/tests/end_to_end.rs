//! The whole path, against git repositories that the test builds.
//!
//! Every repository here is a local directory. The test writes it, commits it,
//! and fetches from it, so nothing reaches the network. `LARK_HOME` points at
//! a scratch directory, so no test touches a real home.
//!
//! covers: K-1, K-2, K-3, K-7, K-8, K-9

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

use lark_pkg::lock::Lock;
use lark_pkg::manifest::Manifest;
use lark_pkg::store::Store;
use lark_pkg::sync::sync;

/// Runs one git command and returns its output, or panics with what it said.
fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output();
    let Ok(output) = output else {
        panic!("git did not run");
    };
    assert!(
        output.status.success(),
        "git {} failed in {}:\n{}",
        arguments.join(" "),
        directory.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Makes a directory a git repository with one commit, and returns the commit.
fn commit_all(directory: &Path, message: &str) -> String {
    if !directory.join(".git").exists() {
        git(directory, &["init", "--quiet", "-b", "main"]);
        git(directory, &["config", "user.email", "test@example.com"]);
        git(directory, &["config", "user.name", "Test"]);
        // A local fetch of one commit needs this on the source side.
        git(
            directory,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
    }
    git(directory, &["add", "-A"]);
    git(directory, &["commit", "--quiet", "-m", message]);
    git(directory, &["rev-parse", "HEAD"])
}

/// A scratch directory that every part of one test shares.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("lark-pkg-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        let Ok(()) = std::fs::create_dir_all(&path) else {
            panic!("cannot make a scratch directory");
        };
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        let Ok(()) = std::fs::create_dir_all(&path) else {
            panic!("cannot make {}", path.display());
        };
        path
    }
}

/// Writes a file, making its parent directory first.
fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let Ok(()) = std::fs::create_dir_all(parent) else {
            panic!("cannot make {}", parent.display());
        };
    }
    let Ok(()) = std::fs::write(path, text) else {
        panic!("cannot write {}", path.display());
    };
}

/// Builds a package repository holding one Lark module, and returns its commit.
fn make_package(scratch: &Scratch, name: &str, body: &str) -> (PathBuf, String) {
    let directory = scratch.join(name);
    write(&directory.join(format!("{name}.lark")), body);
    write(
        &directory.join("lark.toml"),
        &format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"),
    );
    let commit = commit_all(&directory, "the package");
    (directory, commit)
}

/// Builds an index repository holding one entry.
fn make_index(scratch: &Scratch, name: &str, repository: &Path, commit: &str) -> PathBuf {
    let directory = scratch.join("index");
    let entry = format!(
        "name = \"{name}\"\nrepository = \"{}\"\n\n\
         [[version]]\nversion = \"1.2.0\"\ncommit = \"{commit}\"\n",
        repository.display()
    );
    write(
        &directory.join(lark_pkg::index::Entry::path_for(name)),
        &entry,
    );
    commit_all(&directory, "the index");
    directory
}

/// Reads a manifest from text.
fn manifest(text: &str) -> Manifest {
    match toml::from_str(text) {
        Ok(found) => found,
        Err(error) => panic!("the manifest did not read: {error}"),
    }
}

/// A package resolves through an index, and the lock file records the commit.
#[test]
fn a_package_resolves_through_an_index() {
    let scratch = Scratch::new("index-path");
    let (repository, commit) = make_package(
        &scratch,
        "json",
        "export int parse(const char* text) { return 1; }\n",
    );
    let index = make_index(&scratch, "json", &repository, &commit);

    let project = scratch.join("project");
    let store = Store::at(scratch.join("home"));
    let found = manifest(&format!(
        "[registry]\nmain = {{ git = \"{}\" }}\n\n[dependencies]\njson = \"1.2\"\n",
        index.display()
    ));

    let Ok(synced) = sync(&project, &found, &store) else {
        panic!("the sync failed");
    };
    assert_eq!(synced.packages.len(), 1);
    assert!(!synced.from_lock, "the first run reads the index");

    let package = &synced.packages[0];
    assert_eq!(package.name, "json");
    assert_eq!(
        package.version.as_ref().map(ToString::to_string),
        Some("1.2.0".to_owned())
    );
    // The source is on disk, under the store, and holds the module.
    assert!(package.directory.join("json.lark").exists());
    assert!(package.directory.starts_with(store.root()));

    // Rule K-7. The lock file names the commit that the index pinned.
    let Ok(Some(lock)) = Lock::read(&project) else {
        panic!("the lock file was not written");
    };
    let Some(entry) = lock.get("json") else {
        panic!("the lock file has no entry for json");
    };
    assert_eq!(entry.commit.as_deref(), Some(commit.as_str()));
}

/// Rule K-7. A build with a lock file reads no index at all.
#[test]
fn a_build_with_a_lock_file_reads_no_index() {
    let scratch = Scratch::new("lock-path");
    let (repository, commit) =
        make_package(&scratch, "json", "export int one(void) { return 1; }\n");
    let index = make_index(&scratch, "json", &repository, &commit);

    let project = scratch.join("project");
    let store = Store::at(scratch.join("home"));
    let found = manifest(&format!(
        "[registry]\nmain = {{ git = \"{}\" }}\n\n[dependencies]\njson = \"1.2\"\n",
        index.display()
    ));

    let Ok(first) = sync(&project, &found, &store) else {
        panic!("the first sync failed");
    };
    assert!(!first.from_lock);

    // Remove the index. A build with a lock file must not need it.
    let Ok(()) = std::fs::remove_dir_all(&index) else {
        panic!("cannot remove the index");
    };

    let Ok(second) = sync(&project, &found, &store) else {
        panic!("the second sync failed without the index");
    };
    assert!(second.from_lock, "the second run must read the lock file");
    assert_eq!(second.packages.len(), 1);
    assert!(second.packages[0].directory.join("json.lark").exists());
}

/// Rule K-2. A direct dependency needs no index.
#[test]
fn a_direct_dependency_needs_no_index() {
    let scratch = Scratch::new("direct-path");
    let (repository, commit) =
        make_package(&scratch, "zlib", "export int deflate(void) { return 0; }\n");
    git(&repository, &["tag", "v1.0.0"]);

    let project = scratch.join("project");
    let store = Store::at(scratch.join("home"));
    let found = manifest(&format!(
        "[dependencies]\nzlib = {{ git = \"{}\", tag = \"v1.0.0\" }}\n",
        repository.display()
    ));

    let Ok(synced) = sync(&project, &found, &store) else {
        panic!("the sync failed");
    };
    assert_eq!(synced.packages.len(), 1);
    assert!(synced.packages[0].directory.join("zlib.lark").exists());
    // Rule K-5. A tag moves, so the build says so.
    assert_eq!(synced.moving, vec!["zlib".to_owned()]);

    // The lock file records what the tag pointed at when the build ran.
    let Ok(Some(lock)) = Lock::read(&project) else {
        panic!("the lock file was not written");
    };
    assert_eq!(
        lock.get("zlib").and_then(|entry| entry.commit.as_deref()),
        Some(commit.as_str())
    );
}

/// A path dependency is read where it is, and never fetched.
#[test]
fn a_path_dependency_is_read_in_place() {
    let scratch = Scratch::new("path-path");
    let directory = scratch.join("http");
    write(
        &directory.join("http.lark"),
        "export int get(void) { return 200; }\n",
    );

    let project = scratch.join("project");
    let store = Store::at(scratch.join("home"));
    let found = manifest(&format!(
        "[dependencies]\nhttp = {{ path = \"{}\" }}\n",
        directory.display()
    ));

    let Ok(synced) = sync(&project, &found, &store) else {
        panic!("the sync failed");
    };
    assert_eq!(synced.packages[0].directory, directory);
    // Nothing reached the store.
    assert!(!store.root().join("store").exists());
}

/// A transitive dependency joins the graph, read from the package manifest.
#[test]
fn a_transitive_dependency_is_fetched() {
    let scratch = Scratch::new("transitive-path");

    // `bytes` has no dependency of its own.
    let (bytes_repo, bytes_commit) =
        make_package(&scratch, "bytes", "export int len(void) { return 4; }\n");

    // `json` depends on `bytes`, through the same index.
    let json_dir = scratch.join("json");
    write(
        &json_dir.join("json.lark"),
        "export int parse(void) { return 1; }\n",
    );
    write(
        &json_dir.join("lark.toml"),
        "[package]\nname = \"json\"\nversion = \"1.0.0\"\n\n\
         [registry]\nmain = { git = \"unused\" }\n\n\
         [dependencies]\nbytes = \"1\"\n",
    );
    let json_commit = commit_all(&json_dir, "json");

    // One index holding both packages.
    let index = scratch.join("index");
    for (name, repository, commit) in [
        ("json", &json_dir, &json_commit),
        ("bytes", &bytes_repo, &bytes_commit),
    ] {
        write(
            &index.join(lark_pkg::index::Entry::path_for(name)),
            &format!(
                "name = \"{name}\"\nrepository = \"{}\"\n\n\
                 [[version]]\nversion = \"1.0.0\"\ncommit = \"{commit}\"\n",
                repository.display()
            ),
        );
    }
    commit_all(&index, "the index");

    let project = scratch.join("project");
    let store = Store::at(scratch.join("home"));
    let found = manifest(&format!(
        "[registry]\nmain = {{ git = \"{}\" }}\n\n[dependencies]\njson = \"1\"\n",
        index.display()
    ));

    let Ok(synced) = sync(&project, &found, &store) else {
        panic!("the sync failed");
    };
    let mut names: Vec<&str> = synced
        .packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["bytes", "json"],
        "the transitive package is missing"
    );

    // Both are on disk, and the search path names both.
    assert_eq!(synced.search_paths().len(), 2);
    for path in synced.search_paths() {
        assert!(path.exists(), "{} does not exist", path.display());
    }
}

/// Rule K-9. A package lives under the store, and a project holds no copy.
/// Rule K-8. Its directory joins the module search path.
/// covers: K-8, K-9
#[test]
fn a_package_joins_the_search_path_from_the_store() {
    let scratch = Scratch::new("search-path");
    let (repository, commit) =
        make_package(&scratch, "json", "export int one(void) { return 1; }\n");
    let index = make_index(&scratch, "json", &repository, &commit);

    let project = scratch.join("project");
    let home = scratch.join("home");
    let store = Store::at(home.clone());
    let found = manifest(&format!(
        "[registry]\nmain = {{ git = \"{}\" }}\n\n[dependencies]\njson = \"1.2\"\n",
        index.display()
    ));

    let Ok(synced) = sync(&project, &found, &store) else {
        panic!("the sync failed");
    };

    // Rule K-8. The search path names the directory, so `@import json` finds
    // `json.lark` inside it.
    let paths = synced.search_paths();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].join("json.lark").exists());

    // Rule K-9. The source lives under the store, and the project holds none.
    assert!(paths[0].starts_with(home.join("store")));
    assert!(!project.join("json").exists());
    assert!(!project.join("vendor").exists());
}

/// A second project with the same dependency fetches nothing new.
#[test]
fn the_store_is_shared_between_projects() {
    let scratch = Scratch::new("shared-path");
    let (repository, commit) =
        make_package(&scratch, "json", "export int one(void) { return 1; }\n");
    let index = make_index(&scratch, "json", &repository, &commit);

    let store = Store::at(scratch.join("home"));
    let found = manifest(&format!(
        "[registry]\nmain = {{ git = \"{}\" }}\n\n[dependencies]\njson = \"1.2\"\n",
        index.display()
    ));

    let first = scratch.join("first");
    let second = scratch.join("second");
    let Ok(one) = sync(&first, &found, &store) else {
        panic!("the first sync failed");
    };
    let Ok(two) = sync(&second, &found, &store) else {
        panic!("the second sync failed");
    };

    // One directory holds the source, and both projects name it.
    assert_eq!(one.packages[0].directory, two.packages[0].directory);
    // Neither project holds a copy.
    assert!(!first.join("json").exists());
    assert!(!second.join("json").exists());
}
