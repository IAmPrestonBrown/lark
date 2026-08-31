//! Collects the names that a preprocessed translation unit declares.
//!
//! The walk stays at the top level. A name inside a function body or a struct
//! body belongs to that body, not to the unit, so rule N-12 does not carry it.

use std::collections::{BTreeMap, BTreeSet};

use lark_syntax::{SyntaxKind, SyntaxNode};

use SyntaxKind::{
    DECL_SPECIFIERS, DECLARATION, DECLARATOR, ENUM_BODY, ENUM_DEF, ENUMERATOR, FN_DEF, IDENT,
    INIT_DECLARATOR, NAME, PARAM_LIST, STRUCT_DEF, TYPEDEF_KW, UNION_DEF,
};

/// One name that a header declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decl {
    /// The declared name.
    pub name: String,
    /// The text of the declaration that introduced the name.
    pub text: String,
    /// True when the declarator ends in a parameter list.
    pub is_function: bool,
}

/// Every name that a header set declares.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    types: BTreeSet<String>,
    tags: BTreeSet<String>,
    values: BTreeMap<String, Decl>,
    macro_names: BTreeSet<String>,
}

impl Headers {
    /// Reports whether the set holds no name at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
            && self.tags.is_empty()
            && self.values.is_empty()
            && self.macro_names.is_empty()
    }

    /// Reports whether a header defines the name as a macro.
    #[must_use]
    pub fn is_macro(&self, name: &str) -> bool {
        self.macro_names.contains(name)
    }

    /// Returns every macro name, in order.
    pub fn macro_names(&self) -> impl Iterator<Item = &str> {
        self.macro_names.iter().map(String::as_str)
    }

    /// Reports whether a name binds to a type. Rule L-6 needs this answer.
    #[must_use]
    pub fn is_type(&self, name: &str) -> bool {
        self.types.contains(name)
    }

    /// Reports whether a name binds to a value.
    #[must_use]
    pub fn is_value(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Reports whether a name is a struct, a union, or an enum tag.
    #[must_use]
    pub fn is_tag(&self, name: &str) -> bool {
        self.tags.contains(name)
    }

    /// Returns every type name, in order.
    pub fn types(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(String::as_str)
    }

    /// Returns every tag name, in order.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.iter().map(String::as_str)
    }

    /// Returns every value name, in order.
    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    /// Returns the declaration that introduced a value name.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&Decl> {
        self.values.get(name)
    }

    /// Counts every name in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.types.len() + self.tags.len() + self.values.len() + self.macro_names.len()
    }

    /// Adds every name of another set to this one.
    pub fn merge(&mut self, other: &Self) {
        self.types.extend(other.types.iter().cloned());
        self.tags.extend(other.tags.iter().cloned());
        self.macro_names.extend(other.macro_names.iter().cloned());
        for (name, decl) in &other.values {
            self.values
                .entry(name.clone())
                .or_insert_with(|| decl.clone());
        }
    }
}

/// Records every macro name that a `#define` line introduces.
///
/// The compiler runs with `-dD`, so the preprocessed text keeps the lines. A
/// macro is a name that a program can write, and rule L-15 counts it, so the
/// table stays complete.
pub fn macros(preprocessed: &str, out: &mut Headers) {
    for line in preprocessed.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("define") else {
            continue;
        };
        let rest = rest.trim_start();
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() || name.chars().next().is_some_and(char::is_numeric) {
            continue;
        }
        // A macro whose replacement is a type name is a type. `<stdbool.h>`
        // defines `bool` as `_Bool`, and a program writes `bool ready = 1;`.
        // Rule C-1e. Without this the name binds to a value and the
        // declaration reads as an expression.
        let replacement = &rest[name.len()..];
        if is_type_replacement(replacement, out) {
            out.types.insert(name);
        } else {
            out.macro_names.insert(name);
        }
    }
}

/// Reports whether the replacement list of a macro names a type.
///
/// The test is narrow on purpose. The text must hold only type keywords, known
/// type names, and pointer stars, so `#define LIMIT 8` never reads as a type.
fn is_type_replacement(replacement: &str, known: &Headers) -> bool {
    const KEYWORDS: &[&str] = &[
        "void",
        "char",
        "short",
        "int",
        "long",
        "float",
        "double",
        "signed",
        "unsigned",
        "_Bool",
        "_Complex",
        "_Imaginary",
        "const",
        "volatile",
        "struct",
        "union",
        "enum",
    ];
    // A function like macro takes parameters, so the name is followed by `(`.
    if replacement.starts_with('(') {
        return false;
    }
    let text = replacement.trim();
    if text.is_empty() {
        return false;
    }
    let mut saw_name = false;
    for word in text.split_whitespace() {
        let word = word.trim_end_matches('*');
        if word.is_empty() {
            continue;
        }
        if KEYWORDS.contains(&word) || known.types.contains(word) {
            saw_name = true;
            continue;
        }
        return false;
    }
    saw_name
}

/// Walks a tree and records every top level declaration.
pub fn walk(root: &SyntaxNode, out: &mut Headers) {
    for child in root.children() {
        match child.kind() {
            DECLARATION => declaration(&child, out),
            FN_DEF => function(&child, out),
            _ => {}
        }
    }
}

/// Records the names of one declaration.
fn declaration(node: &SyntaxNode, out: &mut Headers) {
    let specifiers = node.children().find(|c| c.kind() == DECL_SPECIFIERS);
    let is_typedef = specifiers
        .as_ref()
        .is_some_and(|s| s.children_with_tokens().any(|e| e.kind() == TYPEDEF_KW));
    if let Some(specifiers) = &specifiers {
        record_tags(specifiers, out);
    }

    let text = node.text().to_string();
    for declarator in node.children().filter(|c| c.kind() == INIT_DECLARATOR) {
        let Some(inner) = declarator.children().find(|c| c.kind() == DECLARATOR) else {
            continue;
        };
        let Some(name) = declared_name(&inner) else {
            continue;
        };
        if is_typedef {
            out.types.insert(name);
        } else {
            let is_function = has_param_list(&inner);
            out.values.entry(name.clone()).or_insert(Decl {
                name,
                text: text.clone(),
                is_function,
            });
        }
    }
}

/// Records the name of a function definition.
fn function(node: &SyntaxNode, out: &mut Headers) {
    if let Some(specifiers) = node.children().find(|c| c.kind() == DECL_SPECIFIERS) {
        record_tags(&specifiers, out);
    }
    let Some(declarator) = node.children().find(|c| c.kind() == DECLARATOR) else {
        return;
    };
    let Some(name) = declared_name(&declarator) else {
        return;
    };
    out.values.entry(name.clone()).or_insert(Decl {
        name,
        text: node.text().to_string(),
        is_function: true,
    });
}

/// Records every tag and enum constant that a specifier list introduces.
fn record_tags(specifiers: &SyntaxNode, out: &mut Headers) {
    for def in specifiers
        .children()
        .filter(|c| matches!(c.kind(), STRUCT_DEF | UNION_DEF | ENUM_DEF))
    {
        if let Some(name) = child_name(&def) {
            out.tags.insert(name);
        }
        // An enum constant is a value in the ordinary namespace, so a program
        // can name it without the enum.
        for body in def.children().filter(|c| c.kind() == ENUM_BODY) {
            for enumerator in body.children().filter(|c| c.kind() == ENUMERATOR) {
                if let Some(name) = child_name(&enumerator) {
                    out.values.entry(name.clone()).or_insert(Decl {
                        name,
                        text: enumerator.text().to_string(),
                        is_function: false,
                    });
                }
            }
        }
    }
}

/// Returns the name that a declarator declares, however deeply it nests.
///
/// A declarator such as `(*fp)(void)` puts the name inside a second declarator,
/// so the search follows the chain.
fn declared_name(declarator: &SyntaxNode) -> Option<String> {
    if let Some(name) = child_name(declarator) {
        return Some(name);
    }
    declarator
        .children()
        .filter(|c| c.kind() == DECLARATOR)
        .find_map(|inner| declared_name(&inner))
}

/// Returns the text of a direct `NAME` child.
fn child_name(node: &SyntaxNode) -> Option<String> {
    let name = node.children().find(|c| c.kind() == NAME)?;
    let token = name
        .children_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
        .find(|t| t.kind() == IDENT)?;
    Some(token.text().to_owned())
}

/// Reports whether a declarator ends in a parameter list.
fn has_param_list(declarator: &SyntaxNode) -> bool {
    declarator.children().any(|c| c.kind() == PARAM_LIST)
        || declarator
            .children()
            .filter(|c| c.kind() == DECLARATOR)
            .any(|inner| has_param_list(&inner))
}

#[cfg(test)]
mod tests {
    use crate::collect_from;

    #[test]
    fn a_struct_tag_is_a_tag_not_a_type() {
        let headers = collect_from("struct Point { int x; };");
        assert!(headers.is_tag("Point"));
        assert!(!headers.is_type("Point"));
    }

    #[test]
    fn an_enum_constant_is_a_value() {
        let headers = collect_from("enum Color { RED, BLUE };");
        assert!(headers.is_value("RED"));
        assert!(headers.is_value("BLUE"));
        assert!(headers.is_tag("Color"));
    }

    #[test]
    fn a_typedef_of_a_struct_records_both_names() {
        let headers = collect_from("typedef struct Node { int v; } Node;");
        assert!(headers.is_tag("Node"));
        assert!(headers.is_type("Node"));
    }

    #[test]
    fn a_function_pointer_declarator_yields_its_name() {
        let headers = collect_from("int (*handler)(int, void *);");
        assert!(headers.is_value("handler"));
    }

    #[test]
    fn a_function_definition_declares_its_name() {
        let headers = collect_from("static int helper(int a) { return a; }");
        assert!(headers.is_value("helper"));
        assert!(headers.value("helper").is_some_and(|d| d.is_function));
    }

    #[test]
    fn a_name_inside_a_body_stays_local() {
        let headers = collect_from("int f(void) { typedef int local_t; return 0; }");
        assert!(!headers.is_type("local_t"));
    }

    #[test]
    fn several_declarators_share_one_declaration() {
        let headers = collect_from("typedef int a_t, b_t;");
        assert!(headers.is_type("a_t"));
        assert!(headers.is_type("b_t"));
    }
}
