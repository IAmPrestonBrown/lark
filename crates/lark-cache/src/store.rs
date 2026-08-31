//! Where cached results live.
//!
//! An entry is two files named by the key: the output, and the witnesses that
//! the step read. A directory of entries needs no index, because the key is
//! the file name.
//!
//! Rule Y-3. A cache is a saving, never a source of truth. Every entry can be
//! deleted at any moment, and the build then does the work again.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::entry::Entry;
use crate::fingerprint::Key;

/// Why a cache operation failed.
#[derive(Debug)]
pub enum Error {
    /// A file could not be read or written.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A directory of cached results.
#[derive(Clone, Debug)]
pub struct Cache {
    root: PathBuf,
    enabled: bool,
}

impl Cache {
    /// Opens the cache in a build directory.
    #[must_use]
    pub fn open(build: &Path) -> Self {
        Self {
            root: build.join(".lark-cache"),
            enabled: true,
        }
    }

    /// Returns a cache that stores nothing and answers every lookup with a
    /// miss.
    ///
    /// `LARK_NO_CACHE=1` selects it, and a test of the slow path uses it.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            root: PathBuf::new(),
            enabled: false,
        }
    }

    /// Opens the cache that the environment asks for.
    #[must_use]
    pub fn for_build(build: &Path) -> Self {
        match std::env::var("LARK_NO_CACHE") {
            Ok(value) if value != "0" => Self::disabled(),
            _ => Self::open(build),
        }
    }

    /// Reports whether the cache stores anything.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the path of the output for one key, when the entry holds.
    ///
    /// Rule Y-2. An entry whose witnesses changed is a miss, so a header that
    /// a step read on its own still invalidates it.
    #[must_use]
    pub fn get(&self, key: &Key, extension: &str) -> Option<PathBuf> {
        if !self.enabled {
            return None;
        }
        let output = self.output_path(key, extension);
        if !output.exists() {
            return None;
        }
        let entry = std::fs::read_to_string(self.entry_path(key))
            .map_or_else(|_| Entry::default(), |text| Entry::parse(&text));
        if !entry.holds() {
            return None;
        }
        Some(output)
    }

    /// Stores an output under a key, and returns where it landed.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    pub fn put(
        &self,
        key: &Key,
        extension: &str,
        bytes: &[u8],
        entry: &Entry,
    ) -> Result<PathBuf, Error> {
        if !self.enabled {
            return Ok(PathBuf::new());
        }
        std::fs::create_dir_all(&self.root)?;
        let output = self.output_path(key, extension);
        // The write goes to a temporary name first, so a reader never meets a
        // half written entry. Two builds can run at once.
        let staging = output.with_extension(format!("{extension}.tmp{}", std::process::id()));
        std::fs::write(&staging, bytes)?;
        std::fs::rename(&staging, &output)?;
        std::fs::write(self.entry_path(key), entry.render())?;
        Ok(output)
    }

    /// Stores a file that already exists on disk.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or written.
    pub fn put_file(
        &self,
        key: &Key,
        extension: &str,
        source: &Path,
        entry: &Entry,
    ) -> Result<PathBuf, Error> {
        let bytes = std::fs::read(source)?;
        self.put(key, extension, &bytes, entry)
    }

    /// Returns the path of the output for a key.
    fn output_path(&self, key: &Key, extension: &str) -> PathBuf {
        self.root.join(format!("{key}.{extension}"))
    }

    /// Returns the path of the witness file for a key.
    fn entry_path(&self, key: &Key) -> PathBuf {
        self.root.join(format!("{key}.witness"))
    }
}

#[cfg(test)]
mod tests {
    use super::Cache;
    use crate::entry::Entry;
    use crate::fingerprint::Fingerprint;

    /// Opens a cache in a fresh scratch directory.
    fn scratch(name: &str) -> Cache {
        let path = std::env::temp_dir().join(format!("lark-cache-store-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        Cache::open(&path)
    }

    /// covers: Y-3
    #[test]
    fn a_stored_output_reads_back() {
        let cache = scratch("round");
        let key = Fingerprint::new().with("x", "one").finish();
        assert!(cache.get(&key, "o").is_none(), "an empty cache must miss");

        let Ok(path) = cache.put(&key, "o", b"object bytes", &Entry::default()) else {
            panic!("the write failed");
        };
        assert!(path.exists());

        let Some(found) = cache.get(&key, "o") else {
            panic!("the entry must be found");
        };
        assert_eq!(std::fs::read(found).ok(), Some(b"object bytes".to_vec()));
    }

    /// A different key is a different entry.
    #[test]
    fn two_keys_do_not_share_an_entry() {
        let cache = scratch("two");
        let first = Fingerprint::new().with("x", "one").finish();
        let second = Fingerprint::new().with("x", "two").finish();
        let _ = cache.put(&first, "o", b"first", &Entry::default());
        assert!(cache.get(&second, "o").is_none());
    }

    /// One key holds two outputs when the extension differs.
    #[test]
    fn one_key_holds_one_output_per_extension() {
        let cache = scratch("ext");
        let key = Fingerprint::new().with("x", "one").finish();
        let _ = cache.put(&key, "o", b"object", &Entry::default());
        let _ = cache.put(&key, "i", b"preprocessed", &Entry::default());
        assert!(cache.get(&key, "o").is_some());
        assert!(cache.get(&key, "i").is_some());
    }

    /// Rule Y-2. A witness that no longer holds makes the entry a miss.
    /// covers: Y-2
    #[test]
    fn a_changed_witness_makes_a_miss() {
        let cache = scratch("witness");
        let watched = std::env::temp_dir().join("lark-cache-store-watched.h");
        let _ = std::fs::write(&watched, "first");

        let key = Fingerprint::new().with("x", "one").finish();
        let entry = Entry::watching(std::slice::from_ref(&watched));
        let _ = cache.put(&key, "o", b"object", &entry);
        assert!(cache.get(&key, "o").is_some(), "an unchanged file must hit");

        let _ = std::fs::write(&watched, "second and longer");
        crate::forget_digests();
        assert!(
            cache.get(&key, "o").is_none(),
            "a changed file must make a miss"
        );
    }

    /// Rule Y-3. A cache that stores nothing still answers every call.
    /// covers: Y-3
    #[test]
    fn a_disabled_cache_always_misses() {
        let cache = Cache::disabled();
        assert!(!cache.is_enabled());
        let key = Fingerprint::new().with("x", "one").finish();
        assert!(cache.put(&key, "o", b"object", &Entry::default()).is_ok());
        assert!(cache.get(&key, "o").is_none());
    }
}
