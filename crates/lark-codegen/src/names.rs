//! Name rules for the emitted C.
//!
//! Rule X-5 keeps the name that the programmer wrote. Rule X-5a reserves the
//! `lk_` prefix for a symbol that the transpiler generates.

use lark_syntax::SyntaxKind::{
    ARROW, DECL_SPECIFIERS, DECLARATION, DECLARATOR, DOT, ENUM_DEF, EXTERN_KW, FN_DEF,
    GLOBAL_BLOCK, IDENT, IFACE_DEF, INIT_DECLARATOR, NAME, PARAM_LIST, PATH, POINTER, STATIC_KW,
    STRUCT_DEF, TYPEDEF_KW, UNION_DEF,
};
use lark_syntax::{SyntaxNode, SyntaxToken, child_tokens};

/// The prefix that rule X-5a reserves for a generated symbol.
pub const GENERATED_PREFIX: &str = "lk_";

/// The words that mark Lark machinery and never reach the emitted C.
const DROPPED_MARKERS: &[&str] = &["export", "gc", "managed", "init", "gc_leaf", "gc_safe"];

/// Reports whether a name belongs to the generated space. See rule X-5a.
#[must_use]
pub fn is_generated_prefix(name: &str) -> bool {
    name.starts_with(GENERATED_PREFIX)
}

/// Returns the header file name for a module.
#[must_use]
pub fn module_header_name(module: &str) -> String {
    format!("{module}.h")
}

/// Reports whether a token marks Lark machinery that the emitted C drops.
///
/// The check needs the position, because rule L-3 makes every one of these
/// words an ordinary identifier elsewhere.
#[must_use]
pub fn is_dropped_marker(token: &SyntaxToken) -> bool {
    if token.kind() != IDENT || !DROPPED_MARKERS.contains(&token.text()) {
        return false;
    }
    let Some(parent) = token.parent() else {
        return false;
    };
    match token.text() {
        "gc" => matches!(parent.kind(), DECL_SPECIFIERS | POINTER),
        "managed" => matches!(parent.kind(), STRUCT_DEF | UNION_DEF),
        "init" | "gc_leaf" | "gc_safe" => parent.kind() == DECL_SPECIFIERS,
        // `export` sits at the head of the item itself.
        _ => matches!(
            parent.kind(),
            DECLARATION | FN_DEF | IFACE_DEF | GLOBAL_BLOCK
        ),
    }
}

/// Returns the C name for the first token of a module path.
///
/// Rule X-5 keeps the name, so `stdio::printf` becomes `printf`.
#[must_use]
pub fn module_path_text(token: &SyntaxToken) -> Option<String> {
    let parent = token.parent()?;
    if parent.kind() != PATH || is_member_path(&parent) {
        return None;
    }
    let names: Vec<SyntaxToken> = child_tokens(&parent)
        .filter(|item| item.kind() == IDENT)
        .collect();
    // Rule N-17. A path reaches any depth, and rule X-5 keeps the last
    // segment, so the first token stands for the whole path.
    let (name, path) = names.split_last()?;
    let first = path.first()?;
    if first.text_range() != token.text_range() {
        return None;
    }
    Some(name.text().to_owned())
}

/// Reports whether a token is a part of a module path that the emitter drops.
///
/// The first name carries the whole path, so the `::` and the second name go.
#[must_use]
pub fn is_dropped_path_part(token: &SyntaxToken) -> bool {
    let Some(parent) = token.parent() else {
        return false;
    };
    if parent.kind() != PATH || is_member_path(&parent) {
        return false;
    }
    let names: Vec<SyntaxToken> = child_tokens(&parent)
        .filter(|item| item.kind() == IDENT)
        .collect();
    // Every token except the first goes, because the first one already stands
    // for the whole path. See `module_path_text`.
    match names.split_first() {
        Some((first, rest)) if !rest.is_empty() => token.text_range() != first.text_range(),
        _ => false,
    }
}

/// Reports whether a path names a member rather than a module.
///
/// `x.Greet::say_hi()` qualifies a method with its interface. See rule O-21.
fn is_member_path(path: &SyntaxNode) -> bool {
    let mut sibling = path.prev_sibling_or_token();
    while let Some(element) = sibling {
        let Some(token) = element.as_token() else {
            return false;
        };
        if token.kind().is_trivia() {
            sibling = element.prev_sibling_or_token();
            continue;
        }
        return matches!(token.kind(), DOT | ARROW);
    }
    false
}

/// Reports whether an item carries the `export` marker.
#[must_use]
pub fn is_exported(item: &SyntaxNode) -> bool {
    child_tokens(item)
        .find(|token| !token.kind().is_trivia())
        .is_some_and(|token| token.kind() == IDENT && token.text() == "export")
}

/// Reports whether a declaration carries a C storage class.
#[must_use]
pub fn has_storage_class(item: &SyntaxNode) -> bool {
    item.children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .flat_map(|specifiers| child_tokens(&specifiers).collect::<Vec<_>>())
        .any(|token| matches!(token.kind(), EXTERN_KW | STATIC_KW | TYPEDEF_KW))
}

/// Reports whether a declaration introduces a variable rather than a prototype.
#[must_use]
pub fn declares_a_variable(item: &SyntaxNode) -> bool {
    declarators_of(item).iter().any(|declarator| {
        !declarator
            .children()
            .any(|child| child.kind() == PARAM_LIST)
    })
}

/// Reports whether an item defines a type and declares no object.
///
/// Rule X-4a puts such a definition in the header only.
#[must_use]
pub fn defines_a_type_only(item: &SyntaxNode) -> bool {
    if item.kind() != DECLARATION {
        return false;
    }
    let Some(specifiers) = item
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    else {
        return false;
    };
    if child_tokens(&specifiers).any(|token| token.kind() == TYPEDEF_KW) {
        return true;
    }
    let has_body = specifiers
        .children()
        .any(|child| matches!(child.kind(), STRUCT_DEF | UNION_DEF | ENUM_DEF));
    has_body && declarators_of(item).is_empty()
}

/// Reports whether an item defines a variable rather than a prototype.
///
/// Rule X-4a gives such a definition an `extern` declaration in the header.
#[must_use]
pub fn defines_a_variable(item: &SyntaxNode) -> bool {
    if item.kind() != DECLARATION || defines_a_type_only(item) {
        return false;
    }
    if item
        .children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .any(|specifiers| child_tokens(&specifiers).any(|token| token.kind() == EXTERN_KW))
    {
        return false;
    }
    declares_a_variable(item)
}

/// Returns the name that an item introduces.
///
/// A declaration that only defines a record yields the tag name.
#[must_use]
pub fn declared_name(item: &SyntaxNode) -> Option<String> {
    if let Some(declarator) = declarators_of(item).into_iter().next()
        && let Some(name) = name_in_declarator(&declarator)
    {
        return Some(name);
    }
    item.children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .flat_map(|specifiers| specifiers.children().collect::<Vec<_>>())
        .filter(|child| matches!(child.kind(), STRUCT_DEF | UNION_DEF | ENUM_DEF))
        .find_map(|record| {
            record
                .children()
                .find(|child| child.kind() == NAME)
                .and_then(|node| node.first_token())
                .map(|token| token.text().to_owned())
        })
}

/// Returns the name inside a declarator, through any nesting.
fn name_in_declarator(declarator: &SyntaxNode) -> Option<String> {
    for child in declarator.children() {
        match child.kind() {
            NAME => return child.first_token().map(|token| token.text().to_owned()),
            DECLARATOR => {
                if let Some(name) = name_in_declarator(&child) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

/// Returns the declarators that belong to an item.
#[must_use]
pub fn declarators_of(item: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut found = Vec::new();
    for child in item.children() {
        match child.kind() {
            DECLARATOR => found.push(child),
            INIT_DECLARATOR => {
                found.extend(child.children().filter(|inner| inner.kind() == DECLARATOR));
            }
            _ => {}
        }
    }
    found
}

/// Returns the file name of the header that a module emits.
///
/// Rule X-4b. The name carries `.lark.` so that it can never take the name of
/// a header that a programmer wrote. A file `attribute.c` compiled as
/// `attribute.lark` would otherwise emit `attribute.h`, and the emitted C
/// would then include the generated header rather than the real one. Every
/// type in the real header would disappear, and the compiler would report a
/// missing type rather than a shadowed file.
#[must_use]
pub fn header_file(module: &str) -> String {
    format!("{}.lark.h", flat_module(module))
}

/// Returns the module path that one `@import` directive names.
///
/// Rule N-16 makes the name a path, so every segment counts.
#[must_use]
pub fn import_path(item: &SyntaxNode) -> String {
    item.children()
        .find(|child| child.kind() == NAME)
        .map(|node| {
            lark_syntax::all_tokens(&node)
                .filter(|token| !token.kind().is_trivia())
                .map(|token| token.text().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Returns a module path as one file name segment.
///
/// Rule N-16 makes a module name a path, and the build writes every generated
/// file into one directory, so the separator becomes a pair of underscores.
/// Rule X-5a reserves that shape for a generated name, so no file the
/// programmer wrote takes it.
#[must_use]
pub fn flat_module(module: &str) -> String {
    // The mangler owns the rule, so a file name and a symbol never disagree.
    lark_mono::mangle::module_prefix(module)
        .strip_prefix(GENERATED_PREFIX)
        .unwrap_or(module)
        .to_owned()
}

#[cfg(test)]
mod header_file_tests {
    use super::{flat_module, header_file};

    /// covers: N-18
    #[test]
    fn a_module_path_flattens_for_a_file_and_a_symbol() {
        // No portable file name and no C identifier holds a colon.
        assert_eq!(flat_module("std::collections"), "std__collections");
        assert_eq!(header_file("std::collections"), "std__collections.lark.h");
        // A plain module keeps its own name.
        assert_eq!(flat_module("stdio"), "stdio");
        assert_eq!(header_file("stdio"), "stdio.lark.h");
        // Rule X-5a reserves the prefix, so the symbol side agrees.
        assert_eq!(
            lark_mono::mangle::module_prefix("std::collections"),
            "lk_std__collections"
        );
    }

    /// covers: X-4b
    #[test]
    fn a_generated_header_cannot_take_a_written_name() {
        assert_eq!(header_file("attribute"), "attribute.lark.h");
        // The name a C project writes is never the name Lark emits.
        assert_ne!(header_file("attribute"), "attribute.h");
    }
}
