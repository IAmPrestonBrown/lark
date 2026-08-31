//! The package commands.
//!
//! Every one reads the `lark.toml` of the current directory. None of them
//! takes a file, because a dependency belongs to a project rather than to one
//! module.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lark_driver::Config;
use lark_pkg::index::{Entry, is_commit};
use lark_pkg::lock::Lock;
use lark_pkg::manifest::{Dependency, Detailed};
use lark_pkg::store::Store;
use lark_pkg::sync::sync;

/// Runs one package command.
pub fn run(command: &str, arguments: &[String]) -> ExitCode {
    let project = PathBuf::from(".");
    let config = match Config::load(&project) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("cannot read lark.toml: {error}");
            return ExitCode::FAILURE;
        }
    };

    match command {
        "tree" => tree(&project, &config),
        "update" => update(&project, &config, arguments.first().map(String::as_str)),
        "add" => add(&project, arguments),
        "vendor" => vendor(&project, &config),
        "publish" => publish(&project, &config, arguments),
        _ => ExitCode::FAILURE,
    }
}

/// Fetches every dependency and returns what it found.
fn fetch(project: &Path, config: &Config) -> Option<lark_pkg::sync::Synced> {
    let manifest = config.manifest();
    if manifest.dependencies.is_empty() {
        println!("this project has no dependency");
        return Some(lark_pkg::sync::Synced::default());
    }
    let store = match Store::open() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{error}");
            return None;
        }
    };
    match sync(project, &manifest, &store) {
        Ok(found) => Some(found),
        Err(error) => {
            eprintln!("{error}");
            None
        }
    }
}

/// Prints the dependency graph.
fn tree(project: &Path, config: &Config) -> ExitCode {
    let Some(found) = fetch(project, config) else {
        return ExitCode::FAILURE;
    };
    println!("{} v{}", config.package.name, config.package.version);
    for package in &found.packages {
        let version = package
            .version
            .as_ref()
            .map_or_else(String::new, |value| format!(" v{value}"));
        println!("|-- {}{version}", package.name);
    }
    warn_about_moving(&found);
    ExitCode::SUCCESS
}

/// Refetches every dependency and rewrites the lock file.
fn update(project: &Path, config: &Config, only: Option<&str>) -> ExitCode {
    if let Some(name) = only {
        // A single package updates by dropping its entry, so the next
        // resolution reads the index for that one alone.
        match Lock::read(project) {
            Ok(Some(mut lock)) => {
                lock.packages.retain(|entry| entry.name != name);
                if lock.write(project).is_err() {
                    eprintln!("cannot write {}", lark_pkg::lock::FILE_NAME);
                    return ExitCode::FAILURE;
                }
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
        // A partial lock file would fetch only what it names, so the whole
        // file goes and every package resolves again. Rule K-7 keeps the file
        // whole or absent.
        let _ = std::fs::remove_file(project.join(lark_pkg::lock::FILE_NAME));
    } else {
        let _ = std::fs::remove_file(project.join(lark_pkg::lock::FILE_NAME));
    }

    let Some(found) = fetch(project, config) else {
        return ExitCode::FAILURE;
    };
    for package in &found.packages {
        let version = package
            .version
            .as_ref()
            .map_or_else(String::new, |value| format!(" v{value}"));
        println!("fetched {}{version}", package.name);
    }
    warn_about_moving(&found);
    ExitCode::SUCCESS
}

/// Adds a dependency to `lark.toml`.
///
/// The command edits the file as text, because a rewrite through the parser
/// would drop every comment that the file holds.
fn add(project: &Path, arguments: &[String]) -> ExitCode {
    let Some(first) = arguments.first() else {
        eprintln!("usage: lark add <name>@<version> | lark add <git-url> [--tag <tag>]");
        return ExitCode::FAILURE;
    };

    let entry = if first.contains("://") || first.starts_with("git@") {
        let name = repository_name(first);
        let mut detail = Detailed {
            git: Some(first.clone()),
            ..Detailed::default()
        };
        let mut rest = arguments.iter().skip(1);
        while let Some(flag) = rest.next() {
            match flag.as_str() {
                "--tag" => detail.tag = rest.next().cloned(),
                "--branch" => detail.branch = rest.next().cloned(),
                "--rev" => detail.rev = rest.next().cloned(),
                other => {
                    eprintln!("unknown option `{other}`");
                    return ExitCode::FAILURE;
                }
            }
        }
        (name, Dependency::Detailed(detail))
    } else {
        let (name, version) = match first.split_once('@') {
            Some((name, version)) => (name.to_owned(), version.to_owned()),
            None => (first.clone(), "*".to_owned()),
        };
        let Ok(requirement) = version.parse() else {
            eprintln!("`{version}` is not a version requirement");
            return ExitCode::FAILURE;
        };
        (name, Dependency::Version(requirement))
    };

    let (name, dependency) = entry;
    let Some(line) = render_entry(&name, &dependency) else {
        eprintln!("cannot render the entry for `{name}`");
        return ExitCode::FAILURE;
    };

    let path = project.join("lark.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("cannot read {}", path.display());
        return ExitCode::FAILURE;
    };
    let updated = if text.contains("[dependencies]") {
        text.replacen("[dependencies]", &format!("[dependencies]\n{line}"), 1)
    } else {
        format!("{}\n\n[dependencies]\n{line}\n", text.trim_end())
    };
    if std::fs::write(&path, updated).is_err() {
        eprintln!("cannot write {}", path.display());
        return ExitCode::FAILURE;
    }
    println!("added {name}");
    // The lock file no longer describes the manifest, so it goes.
    let _ = std::fs::remove_file(project.join(lark_pkg::lock::FILE_NAME));
    ExitCode::SUCCESS
}

/// Copies every dependency into `./vendor`, so a build needs no network.
fn vendor(project: &Path, config: &Config) -> ExitCode {
    let Some(found) = fetch(project, config) else {
        return ExitCode::FAILURE;
    };
    let root = project.join("vendor");
    let _ = std::fs::remove_dir_all(&root);
    for package in &found.packages {
        let target = root.join(&package.name);
        if let Err(error) = copy_tree(&package.directory, &target) {
            eprintln!("cannot copy {}: {error}", package.name);
            return ExitCode::FAILURE;
        }
        println!("vendored {}", package.name);
    }
    println!("add `vendor` to `paths.search` in lark.toml");
    ExitCode::SUCCESS
}

/// Prints the index entry that publishes the current version.
///
/// The command pushes nothing. Publishing is a pull request against the index
/// repository, and this writes the entry to put in it.
fn publish(project: &Path, config: &Config, arguments: &[String]) -> ExitCode {
    let name = &config.package.name;
    let version = &config.package.version;

    let commit = match arguments.iter().position(|item| item == "--commit") {
        Some(index) => arguments.get(index + 1).cloned().unwrap_or_default(),
        None => head_commit(project),
    };
    if !is_commit(&commit) {
        eprintln!(
            "`{commit}` is not a commit\n  \
             rule K-3. an index entry pins a full commit hash, because a tag moves"
        );
        return ExitCode::FAILURE;
    }

    let repository = remote_url(project);
    println!(
        "# add this to {} in your index",
        Entry::path_for(name).display()
    );
    println!();
    println!("name = \"{name}\"");
    println!("repository = \"{repository}\"");
    println!();
    println!("[[version]]");
    println!("version = \"{version}\"");
    println!("commit = \"{commit}\"");
    ExitCode::SUCCESS
}

/// Warns once about every reference that can move. Rule K-5.
fn warn_about_moving(found: &lark_pkg::sync::Synced) {
    if found.moving.is_empty() {
        return;
    }
    eprintln!(
        "warning: {} names a tag or a branch, which moves",
        found.moving.join(", ")
    );
    eprintln!("  the lock file records what it pointed at, so this build repeats");
}

/// Returns the TOML line for one dependency.
///
/// The line is written by hand rather than through the serializer, because a
/// bare version is a string and the serializer needs a table at the top level.
/// The inline form also matches what a person writes.
fn render_entry(name: &str, dependency: &Dependency) -> Option<String> {
    match dependency {
        Dependency::Version(requirement) => Some(format!("{name} = \"{requirement}\"")),
        Dependency::Detailed(detail) => {
            let mut parts = Vec::new();
            if let Some(version) = &detail.version {
                parts.push(format!("version = \"{version}\""));
            }
            if let Some(registry) = &detail.registry {
                parts.push(format!("registry = \"{registry}\""));
            }
            if let Some(url) = &detail.git {
                parts.push(format!("git = \"{url}\""));
            }
            if let Some(tag) = &detail.tag {
                parts.push(format!("tag = \"{tag}\""));
            }
            if let Some(branch) = &detail.branch {
                parts.push(format!("branch = \"{branch}\""));
            }
            if let Some(rev) = &detail.rev {
                parts.push(format!("rev = \"{rev}\""));
            }
            if let Some(path) = &detail.path {
                parts.push(format!("path = \"{}\"", path.display()));
            }
            if parts.is_empty() {
                return None;
            }
            Some(format!("{name} = {{ {} }}", parts.join(", ")))
        }
    }
}

/// Returns the last part of a repository url, without a `.git` suffix.
fn repository_name(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("package")
        .to_owned()
}

/// Returns the commit that `HEAD` names, or an empty string.
fn head_commit(project: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// Returns the url of the `origin` remote, or a placeholder.
fn remote_url(project: &Path) -> String {
    std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(project)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "https://example.com/CHANGE-ME".to_owned())
}

/// Copies a directory, leaving the git metadata behind.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Reports whether a command belongs to this module.
#[must_use]
pub fn is_package_command(command: &str) -> bool {
    matches!(command, "tree" | "update" | "add" | "vendor" | "publish")
}
