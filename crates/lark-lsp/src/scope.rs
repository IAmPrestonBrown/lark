//! What a name means at one place in a file.
//!
//! A language server answers about a position, so it needs the locals of the
//! function that holds the position, not only the module table.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use std::collections::BTreeMap;

use lark_syntax::SyntaxKind::*;
use lark_syntax::SyntaxNode;
use lark_types::{Infer, Lowering, TypeStore};

/// One local variable or parameter.
#[derive(Clone, Debug)]
pub struct Local {
    /// The name that the declaration introduces.
    pub name: String,
    /// The type, as Lark writes it.
    pub type_text: String,
    /// The type name with no `gc` marker and no pointer.
    pub type_name: String,
    /// Where the name is written.
    pub offset: u32,
}

/// Returns every local that is in scope at an offset.
///
/// A declaration is in scope from its own declarator to the end of its block,
/// which is rule L-16 applied to a position rather than to a parse.
pub fn locals_at(root: &SyntaxNode, offset: u32) -> BTreeMap<String, Local> {
    let mut found = BTreeMap::new();
    let Some(function) = enclosing_function(root, offset) else {
        return found;
    };

    for node in function.descendants() {
        let declaration = match node.kind() {
            PARAM => Some(node.clone()),
            DECL_STMT => node.children().find(|child| child.kind() == DECLARATION),
            _ => None,
        };
        let Some(declaration) = declaration else {
            continue;
        };
        // A declaration after the cursor is not in scope yet.
        if u32::from(declaration.text_range().start()) > offset {
            continue;
        }
        let type_text = declared_type(&declaration);
        for (name, at) in declared_names(&declaration) {
            let plain = type_text.replace("gc ", "");
            let type_name = plain.trim_end_matches(['*', ' ']).trim().to_owned();
            found.insert(
                name.clone(),
                Local {
                    name,
                    type_text: type_text.clone(),
                    type_name,
                    offset: at,
                },
            );
        }
    }
    found
}

/// Returns the function definition that holds an offset.
pub fn enclosing_function(root: &SyntaxNode, offset: u32) -> Option<SyntaxNode> {
    root.descendants()
        .filter(|node| node.kind() == FN_DEF)
        .find(|node| {
            let range = node.text_range();
            u32::from(range.start()) <= offset && offset <= u32::from(range.end())
        })
}

/// Returns the type that a declaration names, as Lark writes it.
fn declared_type(declaration: &SyntaxNode) -> String {
    let Some(specifiers) = declaration
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    else {
        return String::new();
    };

    let mut out = String::new();
    for token in lark_syntax::child_tokens(&specifiers) {
        if token.kind().is_trivia() {
            continue;
        }
        if token.kind() == AUTO_KW {
            // Rule T-10. The type comes from the initializer.
            return inferred_type(declaration);
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token.text());
    }
    for child in specifiers.children() {
        if matches!(child.kind(), NAME_REF | PATH)
            && let Some(token) = child.first_token()
        {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(token.text());
        }
    }
    for _ in declaration
        .descendants()
        .filter(|node| node.kind() == POINTER)
    {
        out.push('*');
    }
    out
}

/// Returns the type that `auto` infers. See rule T-10.
fn inferred_type(declaration: &SyntaxNode) -> String {
    let Some(value) = declaration
        .children()
        .find(|child| child.kind() == INIT_DECLARATOR)
        .and_then(|item| item.children().find(|child| is_expression(child.kind())))
    else {
        return String::new();
    };
    let mut store = TypeStore::new();
    let common = store.common();
    let mut infer = Infer {
        lowering: Lowering {
            store: &mut store,
            common,
        },
    };
    let id = infer.inferred(&value);
    if infer.lowering.store.is_error(id) {
        return String::new();
    }
    infer.lowering.store.display(id)
}

/// Returns the names that a declaration introduces, with their offsets.
fn declared_names(declaration: &SyntaxNode) -> Vec<(String, u32)> {
    let mut found = Vec::new();
    for child in declaration.children() {
        let declarator = match child.kind() {
            DECLARATOR => Some(child),
            INIT_DECLARATOR => child.children().find(|inner| inner.kind() == DECLARATOR),
            _ => None,
        };
        if let Some(node) = declarator {
            collect_names(&node, &mut found);
        }
    }
    found
}

/// Adds the name inside a declarator, through any nesting.
fn collect_names(declarator: &SyntaxNode, out: &mut Vec<(String, u32)>) {
    for child in declarator.children() {
        match child.kind() {
            NAME => {
                if let Some(token) = child.first_token() {
                    out.push((
                        token.text().to_owned(),
                        u32::from(token.text_range().start()),
                    ));
                }
            }
            DECLARATOR => collect_names(&child, out),
            _ => {}
        }
    }
}

/// Reports whether a node kind is an expression.
pub fn is_expression(kind: lark_syntax::SyntaxKind) -> bool {
    matches!(
        kind,
        LITERAL_EXPR
            | NAME_EXPR
            | PAREN_EXPR
            | CALL_EXPR
            | INDEX_EXPR
            | FIELD_EXPR
            | METHOD_EXPR
            | POSTFIX_EXPR
            | PREFIX_EXPR
            | CAST_EXPR
            | BIN_EXPR
            | COND_EXPR
            | ASSIGN_EXPR
            | SIZEOF_EXPR
            | ALIGNOF_EXPR
            | NEW_EXPR
            | NEW_ARRAY_EXPR
            | COMPOUND_LITERAL_EXPR
    )
}
