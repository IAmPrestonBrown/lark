//! The name oracle.
//!
//! Rule L-6 resolves an identifier before `<` to decide between a generic
//! argument list and a comparison. The declaration grammar needs the same
//! answer for other identifiers.
//!
//! The parser must not depend on the name resolver, because a language server
//! needs a tree from a file whose names do not resolve. The parser therefore
//! asks an oracle, and `lark-resolve` supplies the real one.

/// What a name binds to in the innermost enclosing scope.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Binding {
    /// The name binds to a type.
    Type,
    /// The name binds to a value.
    Value,
    /// The name binds to nothing that the oracle knows.
    Unbound,
}

/// Answers what a name binds to.
pub trait NameOracle {
    /// Returns the binding of a name in the innermost enclosing scope.
    fn binding(&self, name: &str) -> Binding;

    /// Reports whether the oracle knows every name that the file can see.
    ///
    /// Rule L-15. With a complete table, an unbound name before `<` opens a
    /// generic argument list. With an incomplete table, it opens a comparison,
    /// because a name from an unread header is almost always a value.
    fn is_complete(&self) -> bool {
        false
    }

    /// Reports whether the name binds to a type.
    fn is_type_name(&self, name: &str) -> bool {
        self.binding(name) == Binding::Type
    }
}

/// An oracle that knows no name.
///
/// Every name is unbound and the table is not complete, so the parser falls
/// back to the syntactic rules. The lexer and the parser use it on their own.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoNames;

impl NameOracle for NoNames {
    fn binding(&self, _name: &str) -> Binding {
        Binding::Unbound
    }
}

/// An oracle built from a fixed list of type names. Tests use it.
#[derive(Clone, Debug, Default)]
pub struct KnownNames {
    types: Vec<String>,
    values: Vec<String>,
    complete: bool,
}

impl KnownNames {
    /// Builds an oracle that treats every given name as a type.
    ///
    /// Every other name is unbound, and the table is not complete.
    pub fn new<I, S>(types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            types: types.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    /// Adds names that bind to values.
    #[must_use]
    pub fn with_values<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.values = values.into_iter().map(Into::into).collect();
        self
    }

    /// Marks the table as complete. See rule L-15.
    #[must_use]
    pub fn complete(mut self) -> Self {
        self.complete = true;
        self
    }
}

impl NameOracle for KnownNames {
    fn binding(&self, name: &str) -> Binding {
        if self.types.iter().any(|known| known == name) {
            return Binding::Type;
        }
        if self.values.iter().any(|known| known == name) {
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
    use super::{Binding, KnownNames, NameOracle, NoNames};

    #[test]
    fn an_empty_oracle_binds_nothing_and_is_not_complete() {
        assert_eq!(NoNames.binding("Person"), Binding::Unbound);
        assert!(!NoNames.is_complete());
        assert!(!NoNames.is_type_name("Person"));
    }

    #[test]
    fn a_known_name_binds_to_a_type_or_a_value() {
        let oracle = KnownNames::new(["Person"]).with_values(["count"]);
        assert_eq!(oracle.binding("Person"), Binding::Type);
        assert_eq!(oracle.binding("count"), Binding::Value);
        assert_eq!(oracle.binding("other"), Binding::Unbound);
        assert!(oracle.is_type_name("Person"));
    }

    #[test]
    fn completeness_is_off_until_it_is_set() {
        assert!(!KnownNames::new(["Person"]).is_complete());
        assert!(KnownNames::new(["Person"]).complete().is_complete());
    }
}
