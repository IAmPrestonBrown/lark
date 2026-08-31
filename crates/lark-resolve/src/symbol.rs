//! Symbols and symbol tables.
//!
//! One module holds one table. The table records every top level name that the
//! module declares, with what the name binds to and whether the module exports
//! it.

use std::collections::BTreeMap;

use lark_span::Span;

/// What a symbol names.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SymbolKind {
    /// A struct, a union, an enum, or a name that `typedef` introduces.
    Type,
    /// An interface. Chapter 02 rule T-12 makes an interface name a type.
    Iface,
    /// A function.
    Function,
    /// A variable at file scope, including one from a `@global` block.
    Global,
}

impl SymbolKind {
    /// Reports whether a name of this kind can appear in a type position.
    #[must_use]
    pub const fn is_type(self) -> bool {
        matches!(self, Self::Type | Self::Iface)
    }

    /// Returns the word that a diagnostic uses for this kind.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Iface => "interface",
            Self::Function => "function",
            Self::Global => "global",
        }
    }
}

/// Whether a module exports a symbol. See rules N-5 and N-6.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Visibility {
    /// Only the module that declares the symbol can use it.
    Private,
    /// A module that imports this module can use it.
    Exported,
}

impl Visibility {
    /// Reports whether a module that imports this one can use the symbol.
    #[must_use]
    pub const fn is_exported(self) -> bool {
        matches!(self, Self::Exported)
    }
}

/// One top level name that a module declares.
#[derive(Clone, Debug)]
pub struct Symbol {
    /// The name itself.
    pub name: String,
    /// What the name binds to.
    pub kind: SymbolKind,
    /// Whether a module that imports this one can use the name.
    pub visibility: Visibility,
    /// Where the name is written.
    pub span: Span,
    /// Whether the declaration takes generic parameters.
    pub generic: bool,
}

/// Every top level name that one module declares.
#[derive(Clone, Debug, Default)]
pub struct SymbolTable {
    symbols: BTreeMap<String, Symbol>,
}

impl SymbolTable {
    /// Builds an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a symbol.
    ///
    /// A later declaration of the same name replaces an earlier one only when
    /// the earlier one is not exported. A C program declares a function before
    /// it defines it, and both carry the same name.
    pub fn insert(&mut self, symbol: Symbol) {
        match self.symbols.get_mut(&symbol.name) {
            Some(existing) => {
                if symbol.visibility.is_exported() {
                    existing.visibility = Visibility::Exported;
                }
                if symbol.generic {
                    existing.generic = true;
                }
                if existing.kind == SymbolKind::Global && symbol.kind != SymbolKind::Global {
                    existing.kind = symbol.kind;
                }
            }
            None => {
                self.symbols.insert(symbol.name.clone(), symbol);
            }
        }
    }

    /// Returns the symbol for a name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Symbol> {
        self.symbols.get(name)
    }

    /// Reports whether the table holds a name that binds to a type.
    #[must_use]
    pub fn is_type(&self, name: &str) -> bool {
        self.get(name).is_some_and(|symbol| symbol.kind.is_type())
    }

    /// Returns every symbol, in name order.
    pub fn iter(&self) -> impl Iterator<Item = &Symbol> {
        self.symbols.values()
    }

    /// Returns the number of symbols.
    #[must_use]
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// Reports whether the table holds no symbol.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use lark_span::Span;

    use super::{Symbol, SymbolKind, SymbolTable, Visibility};

    fn symbol(name: &str, kind: SymbolKind, visibility: Visibility) -> Symbol {
        Symbol {
            name: name.to_owned(),
            kind,
            visibility,
            span: Span::at(0),
            generic: false,
        }
    }

    #[test]
    fn a_type_and_an_interface_both_count_as_types() {
        assert!(SymbolKind::Type.is_type());
        assert!(SymbolKind::Iface.is_type());
        assert!(!SymbolKind::Function.is_type());
        assert!(!SymbolKind::Global.is_type());
    }

    #[test]
    fn a_table_finds_a_name_it_holds() {
        let mut table = SymbolTable::new();
        table.insert(symbol("Person", SymbolKind::Type, Visibility::Exported));
        assert!(table.is_type("Person"));
        assert!(!table.is_type("Missing"));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn a_second_declaration_keeps_the_export() {
        let mut table = SymbolTable::new();
        table.insert(symbol("f", SymbolKind::Function, Visibility::Exported));
        table.insert(symbol("f", SymbolKind::Function, Visibility::Private));
        let found = table.get("f").map(|symbol| symbol.visibility);
        assert_eq!(found, Some(Visibility::Exported));
    }

    #[test]
    fn an_export_on_a_later_declaration_reaches_the_first() {
        let mut table = SymbolTable::new();
        table.insert(symbol("f", SymbolKind::Function, Visibility::Private));
        table.insert(symbol("f", SymbolKind::Function, Visibility::Exported));
        let found = table.get("f").map(|symbol| symbol.visibility);
        assert_eq!(found, Some(Visibility::Exported));
    }
}
