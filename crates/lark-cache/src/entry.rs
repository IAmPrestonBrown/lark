//! What a cached step recorded, beside its output.
//!
//! A key names every input that the caller knew about. Some inputs are files
//! that a subprocess read on its own, and the caller learns their names only
//! after the run. The C preprocessor is the example: it reads a header set
//! that the include lines name indirectly.
//!
//! A witness records those files, so a later run checks them before it trusts
//! the entry. Rule Y-2.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// One file that a step read, with what it held at the time.
///
/// The record is a hash of the content, not a timestamp. A timestamp misses a
/// change that lands in the same second, and a length misses a change that
/// keeps the length. Both cases are ordinary: an edit from `#define VALUE 7`
/// to `#define VALUE 9` changes neither. Rule Y-1 resolves that doubt toward a
/// miss, so the record is the content itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Witness {
    /// The file.
    pub path: PathBuf,
    /// A hash of what it held.
    pub digest: String,
}

impl Witness {
    /// Records what a file holds now.
    #[must_use]
    pub fn of(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            digest: digest_of(path),
        }
    }

    /// Reports whether the file still holds the same content.
    #[must_use]
    pub fn holds(&self) -> bool {
        digest_of(&self.path) == self.digest
    }

    /// Returns the line that records this witness in an entry file.
    #[must_use]
    pub fn render(&self) -> String {
        format!("{} {}", self.digest, self.path.display())
    }

    /// Reads one line of an entry file.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let (digest, path) = line.split_once(' ')?;
        Some(Self {
            path: PathBuf::from(path),
            digest: digest.to_owned(),
        })
    }
}

/// Every hash that this process computed, by path.
static DIGESTS: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();

/// Returns a hash of what a file holds.
///
/// A file that cannot be read gives a value of its own, so a missing file
/// never matches an empty one.
///
/// The answer is remembered for the life of the process. One build reads the
/// same system header from every module that includes it, and hashing it once
/// per module costs more than the cache saves. A file that changes while a
/// build runs is a change that no build system reads either way.
fn digest_of(path: &Path) -> String {
    let memo = DIGESTS.get_or_init(|| Mutex::new(HashMap::new()));

    if let Ok(table) = memo.lock()
        && let Some(found) = table.get(path)
    {
        return found.clone();
    }

    let value = compute_digest(path);
    if let Ok(mut table) = memo.lock() {
        table.insert(path.to_path_buf(), value.clone());
    }
    value
}

/// Reads a file and hashes it.
fn compute_digest(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let Ok(bytes) = std::fs::read(path) else {
        return "absent".to_owned();
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let value = hasher.finalize();
    let mut text = String::with_capacity(value.len() * 2);
    for byte in value {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Forgets every remembered hash.
///
/// A test that writes a file and reads it back in one process needs this,
/// because the memo is for the life of a build.
pub fn forget_digests() {
    if let Some(memo) = DIGESTS.get()
        && let Ok(mut table) = memo.lock()
    {
        table.clear();
    }
}

/// One cached result: an output file and the files that the step read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    /// Every file that the step read, and that its key did not name.
    pub witnesses: Vec<Witness>,
}

impl Entry {
    /// Builds an entry that records a list of files.
    #[must_use]
    pub fn watching(paths: &[PathBuf]) -> Self {
        Self {
            witnesses: paths.iter().map(|path| Witness::of(path)).collect(),
        }
    }

    /// Reports whether every recorded file still looks the same.
    ///
    /// Rule Y-2. An entry with no witness holds, because its key named every
    /// input on its own.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.witnesses.iter().all(Witness::holds)
    }

    /// Returns the text of the entry file.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for witness in &self.witnesses {
            out.push_str(&witness.render());
            out.push('\n');
        }
        out
    }

    /// Reads an entry file.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        Self {
            witnesses: text.lines().filter_map(Witness::parse).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Witness};

    /// Writes a file in the scratch directory and returns its path.
    fn scratch(name: &str, text: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("lark-cache-entry-{name}"));
        let _ = std::fs::write(&path, text);
        path
    }

    /// covers: Y-2
    #[test]
    fn a_witness_holds_while_the_file_is_unchanged() {
        let path = scratch("hold", "one");
        let witness = Witness::of(&path);
        assert!(witness.holds());
    }

    /// covers: Y-2
    #[test]
    fn a_witness_fails_when_the_file_changes_length() {
        let path = scratch("length", "one");
        let witness = Witness::of(&path);
        let _ = std::fs::write(&path, "one and more");
        super::forget_digests();
        assert!(!witness.holds(), "a longer file must not hold");
    }

    /// A change that keeps the length and lands in the same second must still
    /// fail. A timestamp and a length both miss it.
    /// covers: Y-1, Y-2
    #[test]
    fn a_witness_fails_when_the_content_changes_but_the_length_does_not() {
        let path = scratch("same-length", "#define VALUE 7\n");
        let witness = Witness::of(&path);
        let _ = std::fs::write(&path, "#define VALUE 9\n");
        super::forget_digests();
        assert!(
            !witness.holds(),
            "a change of the same length must not hold"
        );
    }

    /// covers: Y-2
    #[test]
    fn a_witness_fails_when_the_file_goes() {
        let path = scratch("gone", "one");
        let witness = Witness::of(&path);
        let _ = std::fs::remove_file(&path);
        super::forget_digests();
        assert!(!witness.holds(), "a missing file must not hold");
    }

    /// An entry with no witness holds, because its key named every input.
    /// covers: Y-2
    #[test]
    fn an_entry_with_no_witness_holds() {
        assert!(Entry::default().holds());
    }

    #[test]
    fn an_entry_round_trips() {
        let path = scratch("round", "one");
        let entry = Entry::watching(&[path]);
        let text = entry.render();
        assert_eq!(Entry::parse(&text), entry);
    }

    /// A path with a space in it still reads back whole.
    #[test]
    fn a_path_with_a_space_reads_back() {
        let path = scratch("with space", "one");
        let entry = Entry::watching(std::slice::from_ref(&path));
        let read = Entry::parse(&entry.render());
        assert_eq!(read.witnesses[0].path, path);
    }
}
