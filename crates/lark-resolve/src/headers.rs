//! The names that a C header set contributes to a module.
//!
//! Rule N-12 gives every name from an `#include` to the module that wrote the
//! directive. Reading a header needs the platform preprocessor, and this crate
//! runs no process, so the caller supplies a reader. A tool that cannot run a
//! preprocessor passes [`NoHeaders`] and keeps an incomplete table.

use std::collections::BTreeSet;
use std::path::Path;

/// The names that one module gets from its headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderIndex {
    types: BTreeSet<String>,
    values: BTreeSet<String>,
    complete: bool,
}

impl HeaderIndex {
    /// Builds an index from a type set and a value set.
    pub fn new<T, V, S>(types: T, values: V) -> Self
    where
        T: IntoIterator<Item = S>,
        V: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            types: types.into_iter().map(Into::into).collect(),
            values: values.into_iter().map(Into::into).collect(),
            complete: false,
        }
    }

    /// Marks the index as covering every `#include` of the module.
    ///
    /// Rule L-15 turns a complete table into stricter checks, so a reader sets
    /// this only when it read every directive.
    #[must_use]
    pub fn complete(mut self, yes: bool) -> Self {
        self.complete = yes;
        self
    }

    /// Reports whether the index covers every `#include` of the module.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Reports whether a header declares the name as a type.
    #[must_use]
    pub fn is_type(&self, name: &str) -> bool {
        self.types.contains(name)
    }

    /// Reports whether a header declares the name as a value.
    #[must_use]
    pub fn is_value(&self, name: &str) -> bool {
        self.values.contains(name)
    }

    /// Reports whether a header declares the name at all.
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.is_type(name) || self.is_value(name)
    }

    /// Reports whether the index holds no name.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty() && self.values.is_empty()
    }

    /// Returns every type name, in order.
    pub fn types(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(String::as_str)
    }

    /// Returns every value name, in order.
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.values.iter().map(String::as_str)
    }
}

/// Reads the headers that a module includes.
pub trait HeaderReader {
    /// Returns the names that the `#include` directives of `source` declare.
    ///
    /// `path` fixes the directory that a quoted include searches first.
    fn read(&self, source: &str, path: &Path) -> HeaderIndex;
}

/// A reader that reads nothing.
///
/// Every module keeps an incomplete table, which is the behaviour of a tool
/// that cannot run a preprocessor.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHeaders;

impl HeaderReader for NoHeaders {
    fn read(&self, _source: &str, _path: &Path) -> HeaderIndex {
        HeaderIndex::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_index_is_not_complete() {
        let index = HeaderIndex::default();
        assert!(!index.is_complete());
        assert!(index.is_empty());
    }

    #[test]
    fn an_index_separates_types_from_values() {
        let index = HeaderIndex::new(["FILE", "size_t"], ["printf"]).complete(true);
        assert!(index.is_type("FILE"));
        assert!(!index.is_value("FILE"));
        assert!(index.is_value("printf"));
        assert!(index.has("size_t"));
        assert!(!index.has("nothing"));
        assert!(index.is_complete());
    }

    #[test]
    fn the_empty_reader_reads_nothing() {
        let index = NoHeaders.read("#include <stdio.h>", Path::new("m.lark"));
        assert!(index.is_empty());
        assert!(!index.is_complete());
    }
}
