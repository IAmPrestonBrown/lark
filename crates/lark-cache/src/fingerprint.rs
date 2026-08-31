//! The key that names every input of one step.
//!
//! Rule Y-1. A key that misses an input produces a wrong result silently, so
//! the builder takes each input explicitly and the caller states what it fed
//! in. A key is a hex string, so it is also a file name.

use std::fmt;
use std::fmt::Write as _;
use std::path::Path;

use sha2::{Digest, Sha256};

/// The version of the key format.
///
/// A change to what a key covers changes this, so every old entry stops
/// matching at once. That is cheaper and safer than a migration.
const FORMAT: &str = "lark-cache-1";

/// A key that names one set of inputs.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Key(String);

impl Key {
    /// Returns the key as text, which is also its file name.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0
    }

    /// Returns the first eight characters, for a message a person reads.
    #[must_use]
    pub fn short(&self) -> &str {
        &self.0[..8.min(self.0.len())]
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Builds a key from a list of inputs.
///
/// Every input carries a label, so two inputs of the same bytes under
/// different names give different keys. Without the label, swapping two
/// arguments would leave the key unchanged.
#[derive(Debug)]
pub struct Fingerprint {
    hasher: Sha256,
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self::new()
    }
}

impl Fingerprint {
    /// Starts a key.
    #[must_use]
    pub fn new() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(FORMAT.as_bytes());
        Self { hasher }
    }

    /// Adds one labelled value.
    #[must_use]
    pub fn with(mut self, label: &str, value: &str) -> Self {
        self.feed(label, value.as_bytes());
        self
    }

    /// Adds one labelled block of bytes.
    #[must_use]
    pub fn with_bytes(mut self, label: &str, value: &[u8]) -> Self {
        self.feed(label, value);
        self
    }

    /// Adds the content of a file, or a marker when it cannot be read.
    ///
    /// A file that cannot be read gives a distinct key rather than an error,
    /// so a missing input never quietly matches a present one.
    #[must_use]
    pub fn with_file(mut self, label: &str, path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => self.feed(label, &bytes),
            Err(_) => self.feed(label, b"<unreadable>"),
        }
        self
    }

    /// Adds every value of a list, in the order given.
    #[must_use]
    pub fn with_all<I, S>(mut self, label: &str, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut count = 0usize;
        for value in values {
            self.feed(&format!("{label}[{count}]"), value.as_ref().as_bytes());
            count += 1;
        }
        self.feed(&format!("{label}.len"), count.to_string().as_bytes());
        self
    }

    /// Finishes the key.
    #[must_use]
    pub fn finish(self) -> Key {
        let bytes = self.hasher.finalize();
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            let _ = write!(text, "{byte:02x}");
        }
        Key(text)
    }

    /// Feeds one labelled value, with its length, so no two inputs run
    /// together into a third that hashes the same.
    fn feed(&mut self, label: &str, value: &[u8]) {
        self.hasher.update((label.len() as u64).to_le_bytes());
        self.hasher.update(label.as_bytes());
        self.hasher.update((value.len() as u64).to_le_bytes());
        self.hasher.update(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{Fingerprint, Key};

    fn key(build: impl FnOnce(Fingerprint) -> Fingerprint) -> Key {
        build(Fingerprint::new()).finish()
    }

    /// covers: Y-1
    #[test]
    fn the_same_inputs_give_the_same_key() {
        let first = key(|f| f.with("source", "int main(void) { return 0; }"));
        let second = key(|f| f.with("source", "int main(void) { return 0; }"));
        assert_eq!(first, second);
    }

    /// covers: Y-1
    #[test]
    fn a_changed_input_gives_a_different_key() {
        let first = key(|f| f.with("source", "return 0;"));
        let second = key(|f| f.with("source", "return 1;"));
        assert_ne!(first, second);
    }

    /// A label separates two inputs, so swapping them changes the key.
    /// covers: Y-1
    #[test]
    fn a_label_separates_two_inputs() {
        let first = key(|f| f.with("a", "one").with("b", "two"));
        let swapped = key(|f| f.with("a", "two").with("b", "one"));
        assert_ne!(first, swapped, "the labels must separate the values");
    }

    /// Two inputs must not run together into one. `ab` plus `c` is not the
    /// same as `a` plus `bc`.
    /// covers: Y-1
    #[test]
    fn two_inputs_never_run_together() {
        let first = key(|f| f.with("x", "ab").with("x", "c"));
        let second = key(|f| f.with("x", "a").with("x", "bc"));
        assert_ne!(first, second);
    }

    /// A list of a different length gives a different key, even when the
    /// values run together the same way.
    #[test]
    fn a_list_carries_its_length() {
        let first = key(|f| f.with_all("flags", ["-O2", "-g"]));
        let second = key(|f| f.with_all("flags", ["-O2", "-g", ""]));
        assert_ne!(first, second);
    }

    /// The order of a list matters, because it is the order of the arguments.
    #[test]
    fn a_list_keeps_its_order() {
        let first = key(|f| f.with_all("flags", ["-O2", "-g"]));
        let second = key(|f| f.with_all("flags", ["-g", "-O2"]));
        assert_ne!(first, second);
    }

    /// A file that cannot be read gives a key of its own, never the key of an
    /// empty file.
    #[test]
    fn an_unreadable_file_gives_its_own_key() {
        let missing = key(|f| f.with_file("h", std::path::Path::new("/no/such/file")));
        let empty = key(|f| f.with_bytes("h", b""));
        assert_ne!(missing, empty);
    }

    #[test]
    fn a_key_is_a_hex_string_of_a_fixed_length() {
        let value = key(|f| f.with("x", "y"));
        assert_eq!(value.text().len(), 64);
        assert!(value.text().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(value.short().len(), 8);
    }
}
