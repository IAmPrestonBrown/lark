//! Reads the C headers of a module through the platform preprocessor.
//!
//! The analysis crates run no process, so they take a reader. This module is
//! the reader that a build, the test harness, and the language server share.
//! It keeps a cache, because several modules usually include the same header.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use lark_resolve::{HeaderIndex, HeaderReader};

use crate::{Options, includes_of};

/// A reader that calls the platform preprocessor.
pub struct Reader {
    options: Options,
    /// The cache that survives between builds. Rule Y-4.
    cache: lark_cache::Cache,
    /// The include set of a module keys this, so two modules of one build that
    /// include the same headers preprocess once.
    memory: RefCell<HashMap<String, HeaderIndex>>,
    /// Every problem that a read reported, in order.
    errors: RefCell<Vec<String>>,
}

impl Reader {
    /// Builds a reader with the given options, and no cache on disk.
    #[must_use]
    pub fn new(options: Options) -> Self {
        Self::with_cache(options, lark_cache::Cache::disabled())
    }

    /// Builds a reader that keeps its answers between builds.
    ///
    /// Rule Y-4. The preprocessor is the slowest step of the front end, and
    /// its answer changes only when the include lines, the settings, or the
    /// headers themselves change.
    #[must_use]
    pub fn with_cache(options: Options, cache: lark_cache::Cache) -> Self {
        Self {
            options,
            cache,
            memory: RefCell::new(HashMap::new()),
            errors: RefCell::new(Vec::new()),
        }
    }

    /// Returns every problem that a read reported.
    #[must_use]
    pub fn errors(&self) -> Vec<String> {
        self.errors.borrow().clone()
    }

    /// Returns the search directories that the reader passes to the compiler.
    #[must_use]
    pub fn include_dirs(&self) -> &[PathBuf] {
        &self.options.include_dirs
    }
}

impl HeaderReader for Reader {
    fn read(&self, source: &str, path: &Path) -> HeaderIndex {
        let directives = includes_of(source);
        if directives.is_empty() {
            // No directive means nothing is missing, so the table is complete.
            return HeaderIndex::default().complete(true);
        }
        let key = directives
            .iter()
            .map(|include| include.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(hit) = self.memory.borrow().get(&key) {
            return hit.clone();
        }

        let index = match crate::read_cached(source, path, &self.options, &self.cache) {
            Ok(headers) => {
                // A macro is a value name to a program that writes it, and a
                // type name is never a macro here, so both sets merge.
                let values = headers
                    .values()
                    .chain(headers.macro_names())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let types = headers.types().map(str::to_owned).collect::<Vec<_>>();
                HeaderIndex::new(types, values).complete(true)
            }
            Err(error) => {
                // Rule C-6. A header that does not read leaves the table
                // incomplete, and the build reports the problem.
                self.errors.borrow_mut().push(error.to_string());
                HeaderIndex::default()
            }
        };
        self.memory.borrow_mut().insert(key, index.clone());
        index
    }
}

#[cfg(test)]
mod tests {
    // covers: C-1b, C-1d
    use super::*;

    #[test]
    fn a_module_with_no_include_has_a_complete_table() {
        let reader = Reader::new(Options::default());
        let index = reader.read("int main(void) { return 0; }", Path::new("m.lark"));
        assert!(index.is_complete());
        assert!(index.is_empty());
    }

    #[test]
    fn a_real_header_yields_its_names() {
        let reader = Reader::new(Options::default());
        let index = reader.read("#include <stdio.h>\n", Path::new("m.lark"));
        assert!(index.is_complete());
        assert!(index.is_value("printf"));
        assert!(index.is_type("FILE"));
        assert!(index.is_value("stdout"), "the macro `stdout` is missing");
    }

    #[test]
    fn a_second_read_of_the_same_set_uses_the_cache() {
        let reader = Reader::new(Options::default());
        let first = reader.read("#include <stdio.h>\n", Path::new("a.lark"));
        let second = reader.read("#include <stdio.h>\n", Path::new("b.lark"));
        assert_eq!(first, second);
    }

    #[test]
    fn a_missing_header_leaves_the_table_incomplete() {
        let reader = Reader::new(Options::default());
        let index = reader.read("#include <no_such_header_at_all.h>\n", Path::new("m.lark"));
        assert!(!index.is_complete());
        assert!(!reader.errors().is_empty());
    }
}
