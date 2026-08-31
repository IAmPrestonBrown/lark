//! What one function needs from the shadow stack.
//!
//! Rule M-10 gives a frame only to a function that holds a managed value. Rule
//! M-27 gives one temporary slot to each `new` expression.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use std::collections::BTreeSet;

use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, child_tokens};

/// The name of the frame that the emitter declares.
pub const FRAME: &str = "_lk_frame";

/// The name of the temporary that a `return` uses.
pub const RETURN_TEMP: &str = "_lk_result";

/// What one function needs.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// The number of managed locals, which need an address slot each.
    pub locals: usize,
    /// The number of `new` expressions, which need a value slot each.
    pub temps: usize,
    /// The return type, as the emitter writes it.
    pub return_type: String,
    /// Whether the function returns nothing.
    pub returns_void: bool,
    /// Every managed parameter, with whether it is an interface value.
    ///
    /// Rule M-10. A parameter is a local, so a collection that moves objects
    /// must be able to update it. Without a slot, the callee keeps the address
    /// that the caller passed, and that address is stale after the move.
    pub params: Vec<(String, bool)>,
}

impl Plan {
    /// Reports whether the function needs a frame at all.
    pub fn needs_frame(&self) -> bool {
        self.locals > 0 || self.temps > 0
    }

    /// Returns the array length that C accepts, which is never zero.
    pub fn slot_len(&self) -> usize {
        self.locals.max(1)
    }

    /// Returns the temporary array length that C accepts.
    pub fn temp_len(&self) -> usize {
        self.temps.max(1)
    }
}

/// Reads what one function definition needs.
///
/// `interfaces` names every interface in scope. Rule O-24 makes an interface
/// value hold a managed pointer, so it needs a slot too.
pub fn plan(item: &SyntaxNode, interfaces: &BTreeSet<String>) -> Plan {
    let mut found = Plan {
        return_type: return_type_of(item),
        ..Plan::default()
    };
    found.returns_void = found.return_type.trim() == "void";

    found.params = managed_params(item, interfaces);
    found.locals += found.params.len();

    let Some(body) = item.children().find(|child| child.kind() == BLOCK_STMT) else {
        return found;
    };
    for node in body.descendants() {
        match node.kind() {
            DECL_STMT => {
                found.locals += managed_declarator_count(&node, interfaces);
            }
            NEW_EXPR | NEW_ARRAY_EXPR => found.temps += 1,
            _ => {}
        }
    }
    found
}

/// Returns every managed parameter of a function definition, in order.
///
/// Rule M-10 and rule M-11. A parameter already has a value at the push, so
/// the emitter registers all of them at once.
fn managed_params(item: &SyntaxNode, interfaces: &BTreeSet<String>) -> Vec<(String, bool)> {
    let Some(list) = item.descendants().find(|node| node.kind() == PARAM_LIST) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for param in list.children().filter(|child| child.kind() == PARAM) {
        let interface = interface_name(&param, interfaces);
        if !declaration_is_managed(&param) && interface.is_none() {
            continue;
        }
        let Some(name) = param
            .children()
            .find(|child| child.kind() == DECLARATOR)
            .and_then(|node| declarator_name(&node))
        else {
            continue;
        };
        found.push((name, interface.is_some()));
    }
    found
}

/// Returns the number of managed declarators in a declaration statement.
pub fn managed_declarator_count(node: &SyntaxNode, interfaces: &BTreeSet<String>) -> usize {
    let Some(declaration) = node.children().find(|child| child.kind() == DECLARATION) else {
        return 0;
    };
    if !declaration_is_managed(&declaration) && interface_name(&declaration, interfaces).is_none() {
        return 0;
    }
    declaration
        .children()
        .filter(|child| child.kind() == INIT_DECLARATOR)
        .count()
        .max(usize::from(
            declaration
                .children()
                .any(|child| child.kind() == DECLARATOR),
        ))
}

/// Reports whether a declaration introduces a managed pointer.
///
/// Rule T-1a puts the `gc` marker in the specifiers or after a `*`.
pub fn declaration_is_managed(declaration: &SyntaxNode) -> bool {
    let in_specifiers = declaration
        .children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .any(|specifiers| {
            child_tokens(&specifiers).any(|token| token.kind() == IDENT && token.text() == "gc")
        });
    if in_specifiers {
        return true;
    }
    declaration
        .descendants()
        .filter(|node| node.kind() == POINTER)
        .any(|node| child_tokens(&node).any(|token| token.kind() == IDENT && token.text() == "gc"))
}

/// Returns the interface that a declaration names, when it names one.
///
/// Rule O-24 makes an interface value hold a managed pointer, so the slot holds
/// the address of its object field.
pub fn interface_name(declaration: &SyntaxNode, interfaces: &BTreeSet<String>) -> Option<String> {
    if declaration.descendants().any(|node| node.kind() == POINTER) {
        return None;
    }
    let specifiers = declaration
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)?;
    let name = specifiers
        .children()
        .find(|child| child.kind() == NAME_REF)
        .and_then(|child| child.first_token())
        .map(|token| token.text().to_owned())?;
    if interfaces.contains(&name) {
        Some(name)
    } else {
        None
    }
}

/// Returns the names that a declaration introduces, in order.
pub fn declared_names(declaration: &SyntaxNode) -> Vec<String> {
    let mut found = Vec::new();
    for child in declaration.children() {
        let declarator = match child.kind() {
            DECLARATOR => Some(child),
            INIT_DECLARATOR => child.children().find(|inner| inner.kind() == DECLARATOR),
            _ => None,
        };
        if let Some(node) = declarator
            && let Some(name) = declarator_name(&node)
        {
            found.push(name);
        }
    }
    found
}

/// Returns the name inside a declarator, through any nesting.
fn declarator_name(declarator: &SyntaxNode) -> Option<String> {
    for child in declarator.children() {
        match child.kind() {
            NAME => return child.first_token().map(|token| token.text().to_owned()),
            DECLARATOR => {
                if let Some(name) = declarator_name(&child) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

/// Returns the return type of a function definition, as C text.
///
/// The type is the declaration specifiers plus the pointers of the declarator.
/// A declarator that returns a function pointer needs more, and rule M-12 then
/// falls back to a plain pop.
fn return_type_of(item: &SyntaxNode) -> String {
    let mut out = String::new();
    if let Some(specifiers) = item
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    {
        for token in child_tokens(&specifiers) {
            if token.kind().is_trivia() {
                continue;
            }
            if token.kind() == IDENT
                && matches!(
                    token.text(),
                    "gc" | "init" | "gc_leaf" | "gc_safe" | "export"
                )
            {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(token.text());
        }
        for child in specifiers.children() {
            if matches!(child.kind(), NAME_REF | PATH) {
                if !out.is_empty() {
                    out.push(' ');
                }
                if let Some(token) = child.first_token() {
                    out.push_str(token.text());
                }
            }
        }
    }
    if let Some(declarator) = item.children().find(|child| child.kind() == DECLARATOR) {
        for _ in declarator
            .children()
            .filter(|child| child.kind() == POINTER)
        {
            out.push('*');
        }
    }
    out
}
