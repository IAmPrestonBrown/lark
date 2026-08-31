//! The oracle that the parser asks during the second pass.
//!
//! Rule L-6 needs to know whether a name binds to a type. Rule L-15 needs to
//! know whether the table is complete.

use std::collections::BTreeSet;

use lark_syntax::{Binding, NameOracle};

use crate::headers::HeaderIndex;
use crate::symbol::SymbolTable;

/// The names that one module can see without a prefix.
///
/// Rule N-2 requires the `name::` prefix for every imported symbol, so an
/// imported name never enters this set.
#[derive(Clone, Debug, Default)]
pub struct ModuleNames {
    types: BTreeSet<String>,
    values: BTreeSet<String>,
    complete: bool,
}

impl ModuleNames {
    /// Builds the oracle from the table that pass one produced.
    ///
    /// `complete` follows rule L-15. A module with an `#include` that the front
    /// end cannot read has an incomplete table.
    #[must_use]
    pub fn from_table(table: &SymbolTable, complete: bool) -> Self {
        let mut names = Self {
            complete,
            ..Self::default()
        };
        for symbol in table.iter() {
            if symbol.kind.is_type() {
                names.types.insert(symbol.name.clone());
            } else {
                names.values.insert(symbol.name.clone());
            }
        }
        names
    }

    /// Adds the names that the headers of the module declare.
    ///
    /// Rule N-12 puts a header name in the global namespace, so a program
    /// calls `printf` without a prefix. A module name wins a clash, because
    /// the module is the nearer declaration.
    #[must_use]
    pub fn with_headers(mut self, headers: &HeaderIndex) -> Self {
        for name in headers.types() {
            if !self.values.contains(name) {
                self.types.insert(name.to_owned());
            }
        }
        for name in headers.values() {
            if !self.types.contains(name) {
                self.values.insert(name.to_owned());
            }
        }
        self
    }

    /// Adds the macro names that the module itself defines.
    ///
    /// Rule C-2a. A macro is a value name, because a program writes it where a
    /// value goes. Without it rule L-15 reads `a < LIMIT` as a generic
    /// argument list.
    #[must_use]
    pub fn with_macros<'a, I>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        for name in names {
            if !self.types.contains(name) {
                self.values.insert(name.to_owned());
            }
        }
        self
    }

    /// Returns the number of names that bind to a type.
    #[must_use]
    pub fn type_count(&self) -> usize {
        self.types.len()
    }
}

impl NameOracle for ModuleNames {
    fn binding(&self, name: &str) -> Binding {
        if self.types.contains(name) {
            return Binding::Type;
        }
        if self.values.contains(name) {
            return Binding::Value;
        }
        Binding::Unbound
    }

    fn is_complete(&self) -> bool {
        self.complete
    }
}

#[cfg(test)]
mod tests {
    use lark_span::Span;
    use lark_syntax::{Binding, NameOracle};

    use super::ModuleNames;
    use crate::symbol::{Symbol, SymbolKind, SymbolTable, Visibility};

    fn table() -> SymbolTable {
        let mut table = SymbolTable::new();
        for (name, kind) in [
            ("Person", SymbolKind::Type),
            ("Greet", SymbolKind::Iface),
            ("main", SymbolKind::Function),
        ] {
            table.insert(Symbol {
                name: name.to_owned(),
                kind,
                visibility: Visibility::Private,
                span: Span::at(0),
                generic: false,
            });
        }
        table
    }

    #[test]
    fn a_type_and_an_interface_bind_to_a_type() {
        let names = ModuleNames::from_table(&table(), true);
        assert_eq!(names.binding("Person"), Binding::Type);
        assert_eq!(names.binding("Greet"), Binding::Type);
        assert_eq!(names.binding("main"), Binding::Value);
        assert_eq!(names.binding("missing"), Binding::Unbound);
        assert_eq!(names.type_count(), 2);
    }

    /// covers: L-15
    #[test]
    fn an_unread_include_makes_the_table_incomplete() {
        assert!(ModuleNames::from_table(&table(), true).is_complete());
        assert!(!ModuleNames::from_table(&table(), false).is_complete());
    }
}
