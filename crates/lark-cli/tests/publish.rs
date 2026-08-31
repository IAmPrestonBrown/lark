//! Rule K-10. Publishing writes an entry and pushes nothing.
//!
//! The command reads the tag of the local repository, resolves it to a commit,
//! and prints the entry to submit as a pull request against the index.
//!
//! covers: K-10

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns the path of the `lark` binary that the test run built.
fn binary() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.join("target").join("debug").join("lark")
}

/// Runs one git command, and panics with what it said on a failure.
fn git(directory: &Path, arguments: &[&str]) {
    let Ok(output) = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .output()
    else {
        panic!("git did not run");
    };
    assert!(
        output.status.success(),
        "git {} failed:\n{}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn publish_prints_an_entry_and_pushes_nothing() {
    let binary = binary();
    if !binary.exists() {
        // A `cargo test -p lark-cli` with no prior build has no binary. The
        // gate builds every crate first, so this skip never fires there.
        return;
    }

    let directory = std::env::temp_dir().join("lark-cli-publish-probe");
    let _ = std::fs::remove_dir_all(&directory);
    let Ok(()) = std::fs::create_dir_all(&directory) else {
        panic!("cannot make a scratch directory");
    };

    let Ok(()) = std::fs::write(
        directory.join("lark.toml"),
        "[package]\nname = \"json\"\nversion = \"1.2.0\"\n",
    ) else {
        panic!("cannot write lark.toml");
    };
    let Ok(()) = std::fs::write(
        directory.join("json.lark"),
        "export int one(void) { return 1; }\n",
    ) else {
        panic!("cannot write the module");
    };

    git(&directory, &["init", "--quiet", "-b", "main"]);
    git(&directory, &["config", "user.email", "test@example.com"]);
    git(&directory, &["config", "user.name", "Test"]);
    git(
        &directory,
        &["remote", "add", "origin", "https://example.com/lark-json"],
    );
    git(&directory, &["add", "-A"]);
    git(&directory, &["commit", "--quiet", "-m", "one"]);

    let Ok(output) = Command::new(&binary)
        .arg("publish")
        .current_dir(&directory)
        .output()
    else {
        panic!("the command did not run");
    };
    assert!(output.status.success(), "publish failed");
    let text = String::from_utf8_lossy(&output.stdout);

    // The entry names the file to edit, the repository, and the version.
    assert!(text.contains("js/on/json.toml"), "{text}");
    assert!(text.contains("name = \"json\""), "{text}");
    assert!(
        text.contains("repository = \"https://example.com/lark-json\""),
        "{text}"
    );
    assert!(text.contains("version = \"1.2.0\""), "{text}");

    // Rule K-3. The entry pins a full commit hash, not a tag.
    let Some(line) = text.lines().find(|line| line.starts_with("commit = ")) else {
        panic!("the entry names no commit:\n{text}");
    };
    let commit = line.trim_start_matches("commit = ").trim_matches('"');
    assert!(
        lark_pkg::index::is_commit(commit),
        "`{commit}` is not a commit"
    );

    // Rule K-10. The command pushed nothing, so the repository has one commit
    // and no new remote branch.
    let Ok(log) = Command::new("git")
        .args(["log", "--oneline"])
        .current_dir(&directory)
        .output()
    else {
        panic!("git did not run");
    };
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).lines().count(),
        1,
        "publish must not commit"
    );
}
