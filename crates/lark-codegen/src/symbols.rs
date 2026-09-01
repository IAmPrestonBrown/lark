//! The one place that decides the C name of a Lark name.
//!
//! A name reaches the emitted C through several paths. A function definition
//! and a header prototype come from the syntax tree. A record tag comes from
//! the `Managed` model. The element type of a `new` comes from the emitter.
//! None of those share a code path, so a change to the naming rule that
//! touched only one of them left the declaration and the use disagreeing, and
//! nothing linked.
//!
//! Every one of them goes through this module instead. `c_name` answers for a
//! name that a model holds, and `token` answers for a name that the tree
//! holds. A change to rule X-5 is then one edit here.
//!
//! Rule X-5 keeps the written name today, so both answers are the identity.
//! The tests that follow state that, so the switch is visible when it happens.
//!
//! A probe of the seam replaced `c_name` with a mangling one. Every path
//! agreed, and the one failure that remained named the rule that the switch
//! still needs: a declaration with no body binds to a C symbol that already
//! exists, so it keeps its name. `export int printf(const char*, ...);` is
//! that shape, and 80 of the 149 failures came from it alone.

use std::collections::BTreeSet;

use lark_syntax::SyntaxKind::IDENT;
use lark_syntax::SyntaxToken;

use crate::names;

/// What one token contributes to the emitted C.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Emit {
    /// Write this text.
    Text(String),
    /// Write nothing.
    Skip,
    /// Write nothing, and drop the space that follows.
    SkipSpace,
}

/// The naming rule for one module.
#[derive(Clone, Debug)]
pub struct Symbols {
    /// The module that the emitter is writing.
    module: String,
    /// Every name that this module declares at the top level.
    ///
    /// A token that carries one of these names refers to a declaration rather
    /// than to a local, a field, or a name that a C header gave. Rule X-5
    /// decides what such a name links as.
    declared: BTreeSet<String>,
}

impl Symbols {
    /// Builds the rule for one module.
    #[must_use]
    pub fn new(module: &str) -> Self {
        Self {
            module: module.to_owned(),
            declared: BTreeSet::new(),
        }
    }

    /// Builds the rule with the names that the module declares.
    #[must_use]
    pub fn with_declared(module: &str, declared: BTreeSet<String>) -> Self {
        Self {
            module: module.to_owned(),
            declared,
        }
    }

    /// Returns the module that this rule belongs to.
    #[must_use]
    pub fn module(&self) -> &str {
        &self.module
    }

    /// Returns the C name of a top level name that a module declares.
    ///
    /// The caller holds the name as text, from a model rather than from the
    /// tree. A record tag, a descriptor, and an element type all arrive here.
    #[must_use]
    pub fn c_name(&self, _module: &str, written: &str) -> String {
        // Rule X-5. The written name reaches C unchanged.
        written.to_owned()
    }

    /// Returns what one token contributes, for a token that carries a name.
    ///
    /// The caller handles anything that comes before a name: a substitution
    /// inside an instantiation, a `Self` inside an implementation, and the
    /// whitespace that a dropped marker leaves behind.
    #[must_use]
    pub fn token(&self, token: &SyntaxToken) -> Emit {
        self.token_with(token, &BTreeSet::new())
    }

    /// Returns what one token contributes, with the locals that shadow.
    ///
    /// A local and a parameter keep the name that the programmer wrote, so a
    /// local of the same name as a declaration hides it here, the way C hides
    /// it in the program.
    #[must_use]
    pub fn token_with(&self, token: &SyntaxToken, shadowed: &BTreeSet<String>) -> Emit {
        if names::is_dropped_marker(token) {
            return Emit::SkipSpace;
        }
        // Rule X-5. A qualified use names a declaration of another module.
        if let Some(name) = names::module_path_text(token) {
            let owner = names::module_path_owner(token).unwrap_or_default();
            return Emit::Text(self.c_name(&owner, &name));
        }
        if names::is_dropped_path_part(token) {
            return Emit::Skip;
        }
        // A name that this module declares, used or defined without a path.
        let text = token.text();
        if token.kind() == IDENT && self.declared.contains(text) && !shadowed.contains(text) {
            return Emit::Text(self.c_name(&self.module, text));
        }
        Emit::Text(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::Symbols;

    /// covers: X-5
    #[test]
    fn a_written_name_reaches_c_unchanged() {
        let symbols = Symbols::new("geometry");
        assert_eq!(symbols.c_name("geometry", "area"), "area");
        assert_eq!(symbols.c_name("std::collections", "Vector"), "Vector");
        assert_eq!(symbols.module(), "geometry");
    }
}
