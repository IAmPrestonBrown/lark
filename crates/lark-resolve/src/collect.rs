//! Pass one. Reads every top level declaration and records every name.
//!
//! Rule L-8 gives the two pass shape. This module is the first pass, so a
//! program can reference any top level name from any point in the file.

use std::collections::BTreeSet;

use lark_span::Span;
use lark_syntax::SyntaxKind::{
    DECL_SPECIFIERS, DECLARATION, DECLARATOR, ENUM_DEF, FN_DEF, GENERIC_PARAMS, GLOBAL_BLOCK,
    IDENT, IFACE_DEF, IMPORT_DIRECTIVE, INIT_DECLARATOR, NAME, PARAM_LIST, POINTER, PP_DIRECTIVE,
    STRUCT_DEF, TYPEDEF_KW, UNION_DEF,
};
use lark_syntax::{SyntaxNode, SyntaxToken, child_tokens};

use crate::symbol::{Symbol, SymbolKind, SymbolTable, Visibility};

/// One `@import` directive.
#[derive(Clone, Debug)]
pub struct Import {
    /// The module name that the directive gives.
    pub name: String,
    /// Where the name is written.
    pub span: Span,
}

/// What pass one found in one module.
#[derive(Clone, Debug, Default)]
pub struct Collected {
    /// Every top level name that the module declares.
    pub table: SymbolTable,
    /// Every module that this one imports.
    pub imports: Vec<Import>,
    /// Whether the module holds an `#include` that the front end cannot read.
    ///
    /// Rule L-15 needs this to decide whether the name table is complete.
    pub has_unread_include: bool,
    /// Every macro that the module itself defines. See rule C-2a.
    pub macros: BTreeSet<String>,
}

/// Returns the name that a `#define` line introduces.
fn defined_name(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("define")?;
    if rest
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let name: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() || name.starts_with(|c: char| c.is_numeric()) {
        return None;
    }
    Some(name)
}

/// Reads every top level declaration of one module.
#[must_use]
pub fn collect(root: &SyntaxNode) -> Collected {
    let mut found = Collected::default();

    for token in child_tokens(root) {
        if token.kind() != PP_DIRECTIVE {
            continue;
        }
        let text = token.text();
        if text.contains("include") {
            found.has_unread_include = true;
        }
        // Rule C-2a. A module is not preprocessed, so a name that it defines
        // with `#define` is otherwise unbound. Rule L-15 then reads `a < B` as
        // a generic argument list, and a valid C file fails to parse.
        if let Some(name) = defined_name(text) {
            found.macros.insert(name);
        }
    }

    for item in root.children() {
        match item.kind() {
            IMPORT_DIRECTIVE => collect_import(&item, &mut found),
            IFACE_DEF => collect_iface(&item, &mut found.table),
            GLOBAL_BLOCK => collect_global_block(&item, &mut found.table),
            DECLARATION | FN_DEF => collect_declaration(&item, &mut found.table, None),
            _ => {}
        }
    }

    found
}

/// Records the module name that an `@import` directive gives.
fn collect_import(item: &SyntaxNode, found: &mut Collected) {
    let Some(name) = item.children().find(|child| child.kind() == NAME) else {
        return;
    };
    let Some(token) = name.first_token() else {
        return;
    };
    found.imports.push(Import {
        name: token.text().to_owned(),
        span: span_of(&token),
    });
}

/// Records the name of an interface.
fn collect_iface(item: &SyntaxNode, table: &mut SymbolTable) {
    let visibility = visibility_of(item);
    let Some(token) = item
        .children()
        .find(|child| child.kind() == NAME)
        .and_then(|name| name.first_token())
    else {
        return;
    };
    table.insert(Symbol {
        name: token.text().to_owned(),
        kind: SymbolKind::Iface,
        visibility,
        span: span_of(&token),
        generic: false,
    });
}

/// Records every declaration inside a `@global` block.
///
/// Rule I-6 makes each one a global variable of the module.
fn collect_global_block(item: &SyntaxNode, table: &mut SymbolTable) {
    let outer = visibility_of(item);
    for child in item.children() {
        if matches!(child.kind(), DECLARATION | FN_DEF) {
            collect_declaration(&child, table, Some(outer));
        }
    }
}

/// Records the names that one declaration or function definition introduces.
fn collect_declaration(item: &SyntaxNode, table: &mut SymbolTable, inherited: Option<Visibility>) {
    let visibility = match inherited {
        Some(Visibility::Exported) => Visibility::Exported,
        _ => visibility_of(item),
    };

    let specifiers = item
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS);
    let mut is_typedef = false;

    if let Some(specifiers) = &specifiers {
        is_typedef = child_tokens(specifiers).any(|token| token.kind() == TYPEDEF_KW);

        for record in specifiers.children() {
            if matches!(record.kind(), STRUCT_DEF | UNION_DEF | ENUM_DEF) {
                collect_record(&record, table, visibility);
            }
        }
    }

    for declarator in top_declarators(item) {
        let Some(token) = declarator_name(&declarator) else {
            continue;
        };
        let kind = if is_typedef {
            SymbolKind::Type
        } else if item.kind() == FN_DEF || declarator_is_function(&declarator) {
            SymbolKind::Function
        } else {
            SymbolKind::Global
        };
        table.insert(Symbol {
            name: token.text().to_owned(),
            kind,
            visibility,
            span: span_of(&token),
            generic: has_generic_params(&declarator),
        });
    }
}

/// Records the tag name of a struct, a union, or an enum.
fn collect_record(record: &SyntaxNode, table: &mut SymbolTable, visibility: Visibility) {
    let Some(token) = record
        .children()
        .find(|child| child.kind() == NAME)
        .and_then(|name| name.first_token())
    else {
        return;
    };
    table.insert(Symbol {
        name: token.text().to_owned(),
        kind: SymbolKind::Type,
        visibility,
        span: span_of(&token),
        generic: record
            .children()
            .any(|child| child.kind() == GENERIC_PARAMS),
    });
}

/// Returns the declarators that belong to the item itself.
fn top_declarators(item: &SyntaxNode) -> Vec<SyntaxNode> {
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

/// Returns the name that a declarator introduces.
///
/// The search descends into a nested declarator, as in `int (*f)(void)`, and
/// never into a parameter list.
fn declarator_name(declarator: &SyntaxNode) -> Option<SyntaxToken> {
    for child in declarator.children() {
        match child.kind() {
            NAME => return child.first_token(),
            DECLARATOR => {
                if let Some(token) = declarator_name(&child) {
                    return Some(token);
                }
            }
            _ => {}
        }
    }
    None
}

/// Reports whether a declarator names a function rather than a variable.
fn declarator_is_function(declarator: &SyntaxNode) -> bool {
    let pointer_inside = declarator
        .children()
        .filter(|child| child.kind() == DECLARATOR)
        .any(|nested| nested.children().any(|inner| inner.kind() == POINTER));
    !pointer_inside
        && declarator
            .children()
            .any(|child| child.kind() == PARAM_LIST)
}

/// Reports whether a declarator carries generic parameters.
fn has_generic_params(declarator: &SyntaxNode) -> bool {
    declarator
        .children()
        .any(|child| child.kind() == GENERIC_PARAMS)
}

/// Returns the visibility that an item declares.
///
/// Rule N-5 makes a declaration private by default. Rule N-6 exports it with
/// the `export` marker, which the parser leaves as an identifier token.
fn visibility_of(item: &SyntaxNode) -> Visibility {
    let first = child_tokens(item).find(|token| !token.kind().is_trivia());
    match first {
        Some(token) if token.kind() == IDENT && token.text() == "export" => Visibility::Exported,
        _ => Visibility::Private,
    }
}

/// Returns the span of a token.
pub(crate) fn span_of(token: &SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}
