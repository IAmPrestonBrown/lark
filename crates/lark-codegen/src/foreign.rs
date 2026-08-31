//! Foreign calls and the state that a thread takes around them.
//!
//! | Marker | Emitted C |
//! |---|---|
//! | `gc_safe`, and every unmarked extern | A transition around the call. Rules M-19 and M-21. |
//! | `gc_leaf` | Nothing. Rule M-20. |
//!
//! A transition is a statement pair in the specification. A call is an
//! expression, so the emitted form uses the comma operator and a helper that
//! returns the value.
//!
//! ```c
//! (lark_enter_safe(), lk_leave__i(printf("%d", x)))
//! ```
//!
//! The comma operator sequences the left operand first. The argument of the
//! helper is the call itself, so the order is: enter, call, leave, value.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use lark_resolve::ModuleGraph;
use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, child_tokens};

/// What a foreign function does to the thread that calls it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Contract {
    /// A collection can run during the call. Rule M-19.
    Safe,
    /// The callee triggers no collection, so no transition happens. Rule M-20.
    Leaf,
}

/// Every foreign function that a program declares.
#[derive(Clone, Debug, Default)]
pub struct Foreign {
    functions: BTreeMap<String, (Contract, String)>,
}

impl Foreign {
    /// Returns the contract and the result type of a foreign function.
    pub fn get(&self, name: &str) -> Option<(Contract, &str)> {
        self.functions
            .get(name)
            .map(|(contract, result)| (*contract, result.as_str()))
    }

    /// Reports whether a call to a name needs a transition.
    pub fn needs_transition(&self, name: &str) -> bool {
        matches!(self.get(name), Some((Contract::Safe, _)))
    }
}

/// Reads every foreign declaration of a program.
///
/// A declaration with no body names a function that lives elsewhere. Rule M-21
/// gives an unmarked one the safe contract, because that is always correct.
pub fn collect(graph: &ModuleGraph) -> Foreign {
    let mut found = Foreign::default();
    let mut defined = Vec::new();

    for module in graph.modules() {
        let root = module.parse.syntax();
        for item in root.children() {
            if item.kind() == FN_DEF
                && let Some(name) = declared_name(&item)
            {
                defined.push(name);
            }
        }
    }

    for module in graph.modules() {
        let root = module.parse.syntax();
        for item in root.children().filter(|node| node.kind() == DECLARATION) {
            // A definition is not foreign, and neither is a variable.
            let Some(declarator) = first_declarator(&item) else {
                continue;
            };
            if !declarator
                .children()
                .any(|child| child.kind() == PARAM_LIST)
            {
                continue;
            }
            let Some(name) = declared_name(&item) else {
                continue;
            };
            if defined.contains(&name) {
                continue;
            }

            let Some(specifiers) = item
                .children()
                .find(|child| child.kind() == DECL_SPECIFIERS)
            else {
                continue;
            };
            let leaf = child_tokens(&specifiers)
                .any(|token| token.kind() == IDENT && token.text() == "gc_leaf");
            let contract = if leaf { Contract::Leaf } else { Contract::Safe };
            found.functions.insert(name, (contract, result_type(&item)));
        }
    }

    found
}

/// Returns the C name of the helper that leaves the safe state.
pub fn leave_helper(result: &str) -> String {
    format!("lk_leave__{}", lark_mono::mangle::argument(result))
}

/// Returns the definition of one helper.
pub fn leave_helper_definition(result: &str) -> String {
    let name = leave_helper(result);
    let mut out = String::new();
    let _ = writeln!(out, "static {result} {name}({result} lk_value) {{");
    let _ = writeln!(out, "    lark_leave_safe();");
    let _ = writeln!(out, "    return lk_value;");
    let _ = writeln!(out, "}}");
    out
}

/// Returns the call form that a safe foreign call takes. See rule M-19.
pub fn safe_call(result: &str, call: &str) -> String {
    if result.trim() == "void" {
        return format!("(lark_enter_safe(), {call}, lark_leave_safe())");
    }
    format!("(lark_enter_safe(), {}({call}))", leave_helper(result))
}

/// Returns the result type of a declaration, as C text.
fn result_type(item: &SyntaxNode) -> String {
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
                    "gc" | "export" | "gc_leaf" | "gc_safe" | "init"
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
            if matches!(child.kind(), NAME_REF | PATH)
                && let Some(token) = child.first_token()
            {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(token.text());
            }
        }
    }
    if let Some(declarator) = first_declarator(item) {
        for _ in declarator
            .children()
            .filter(|child| child.kind() == POINTER)
        {
            out.push('*');
        }
    }
    if out.is_empty() {
        "void".to_owned()
    } else {
        out
    }
}

/// Returns the first declarator of an item.
fn first_declarator(item: &SyntaxNode) -> Option<SyntaxNode> {
    for child in item.children() {
        match child.kind() {
            DECLARATOR => return Some(child),
            INIT_DECLARATOR => {
                if let Some(node) = child.children().find(|inner| inner.kind() == DECLARATOR) {
                    return Some(node);
                }
            }
            _ => {}
        }
    }
    None
}

/// Returns the name that an item introduces.
fn declared_name(item: &SyntaxNode) -> Option<String> {
    let declarator = first_declarator(item)?;
    declarator_name(&declarator)
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
