//! Global blocks, the `init` function, and the order they run in.
//!
//! | Code | Rule |
//! |---|---|
//! | `LK0700` | I-1. No function carries the `init` marker. |
//! | `LK0701` | I-1. More than one function carries it. |
//! | `LK0710` | I-11. `@init` names an unknown global block. |
//! | `LK0711` | I-17. An initializer reads a global that is not ready. |
//!
//! Chapter 07 section 4 gives the order that a run follows.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use std::collections::BTreeMap;

use lark_diag::{Diagnostic, Diagnostics, LK0700, LK0701, LK0710, LK0711};
use lark_span::{SourceId, Span};
use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, SyntaxToken, child_tokens};

/// One global block. See rule I-6.
#[derive(Clone, Debug)]
pub struct Block {
    /// The block name, which is a handle for initialization.
    pub name: String,
    /// The function that the block attaches to. See rule I-12.
    pub attached_to: Option<String>,
    /// The order number, when the attachment gives one. See rule I-13.
    pub order: Option<i64>,
    /// The names that the block declares, in order.
    pub declares: Vec<String>,
    /// Whether the block carries `export`.
    pub exported: bool,
    /// The node that holds the whole block.
    pub node: SyntaxNode,
    /// Where the block name is written.
    pub span: Span,
}

/// Everything that one module says about initialization.
#[derive(Clone, Debug, Default)]
pub struct Globals {
    /// Every global block, by name.
    pub blocks: BTreeMap<String, Block>,
    /// Every block, in declaration order.
    pub order: Vec<String>,
    /// The name of the function that carries `init`, when one does.
    pub init_function: Option<String>,
    /// Where the `init` marker is written.
    pub init_spans: Vec<Span>,
}

impl Globals {
    /// Returns the blocks that attach to a function, in the order they run.
    ///
    /// Chapter 07 section 4. A numbered block runs first, lowest number first.
    /// An unnumbered block follows, in declaration order. A tie between equal
    /// numbers resolves by declaration order.
    pub fn attached_to(&self, function: &str) -> Vec<&Block> {
        let mut numbered: Vec<(usize, &Block)> = Vec::new();
        let mut plain: Vec<(usize, &Block)> = Vec::new();
        for (index, name) in self.order.iter().enumerate() {
            let Some(block) = self.blocks.get(name) else {
                continue;
            };
            if block.attached_to.as_deref() != Some(function) {
                continue;
            }
            if block.order.is_some() {
                numbered.push((index, block));
            } else {
                plain.push((index, block));
            }
        }
        numbered.sort_by_key(|(index, block)| (block.order.unwrap_or(0), *index));
        numbered
            .into_iter()
            .chain(plain)
            .map(|(_, block)| block)
            .collect()
    }
}

/// Reports whether a module uses managed memory.
///
/// Such a module needs the runtime, so rule I-1 needs an `init` function.
pub fn uses_managed_memory(root: &SyntaxNode) -> bool {
    root.descendants().any(|node| match node.kind() {
        NEW_EXPR | NEW_ARRAY_EXPR | GLOBAL_BLOCK | IFACE_DEF | IMPL_DEF => true,
        DECL_SPECIFIERS | POINTER => child_tokens(&node)
            .any(|token| token.kind() == IDENT && matches!(token.text(), "gc" | "managed")),
        _ => false,
    })
}

/// Reads every global block and the `init` marker of one module.
pub fn collect(root: &SyntaxNode) -> Globals {
    let mut found = Globals::default();

    for item in root.children() {
        match item.kind() {
            GLOBAL_BLOCK => {
                if let Some(block) = read_block(&item) {
                    found.order.push(block.name.clone());
                    found.blocks.insert(block.name.clone(), block);
                }
            }
            FN_DEF => {
                let Some(specifiers) = item
                    .children()
                    .find(|child| child.kind() == DECL_SPECIFIERS)
                else {
                    continue;
                };
                let Some(marker) = child_tokens(&specifiers)
                    .find(|token| token.kind() == IDENT && token.text() == "init")
                else {
                    continue;
                };
                found.init_spans.push(span_of(&marker));
                if found.init_function.is_none() {
                    found.init_function = declared_name(&item);
                }
            }
            _ => {}
        }
    }

    found
}

/// Reads one global block.
fn read_block(item: &SyntaxNode) -> Option<Block> {
    let name_node = item.children().find(|child| child.kind() == NAME)?;
    let name_token = name_node.first_token()?;
    let exported = child_tokens(item)
        .find(|token| !token.kind().is_trivia())
        .is_some_and(|token| token.kind() == IDENT && token.text() == "export");

    let attach = item.children().find(|child| child.kind() == GLOBAL_ATTACH);
    let attached_to = attach.as_ref().and_then(|node| {
        node.children()
            .find(|child| child.kind() == NAME_REF)
            .and_then(|child| child.first_token())
            .map(|token| token.text().to_owned())
    });
    let order = attach.as_ref().and_then(|node| {
        child_tokens(node)
            .find(|token| token.kind() == INT_NUMBER)
            .and_then(|token| token.text().parse().ok())
    });

    let declares = item
        .children()
        .filter(|child| matches!(child.kind(), DECLARATION | FN_DEF))
        .flat_map(|child| declared_names(&child))
        .collect();

    Some(Block {
        name: name_token.text().to_owned(),
        attached_to,
        order,
        declares,
        exported,
        node: item.clone(),
        span: span_of(&name_token),
    })
}

/// Runs every initialization check over one module.
///
/// `program_init_count` is the number of `init` functions in the whole program,
/// because rule I-1 counts across every module.
pub fn check(source: SourceId, root: &SyntaxNode, found: &Globals, out: &mut Diagnostics) {
    // Rule I-1. Two or more markers is a problem in the module that holds them.
    if found.init_spans.len() > 1 {
        for span in found.init_spans.iter().skip(1) {
            out.push(
                Diagnostic::new(LK0701, source, *span)
                    .label("a program starts the runtime once")
                    .secondary(source, found.init_spans[0], "the first marker is here")
                    .note("rule I-1 allows exactly one `init` function"),
            );
        }
    }
    // Rule I-11. `@init` names a block that the module declares.
    for statement in root.descendants().filter(|node| node.kind() == INIT_STMT) {
        let Some(token) = statement
            .children()
            .find(|child| child.kind() == NAME_REF)
            .and_then(|child| child.first_token())
        else {
            continue;
        };
        if found.blocks.contains_key(token.text()) {
            continue;
        }
        out.push(
            Diagnostic::new(LK0710, source, span_of(&token))
                .label(format!("no `@global {}` block exists here", token.text()))
                .help("declare the block, or correct the name"),
        );
    }

    check_order(source, root, found, out);
}

/// Rule I-17. An initializer must not read a global that runs later.
fn check_order(source: SourceId, root: &SyntaxNode, found: &Globals, out: &mut Diagnostics) {
    let mut functions: Vec<&String> = found
        .blocks
        .values()
        .filter_map(|block| block.attached_to.as_ref())
        .collect();
    functions.sort();
    functions.dedup();

    for function in functions {
        let ordered = found.attached_to(function);
        report_out_of_order(source, &ordered, out);
    }

    // An explicit `@init` also fixes an order, and rule I-16 leaves it to the
    // programmer.
    for body in root.descendants().filter(|node| node.kind() == BLOCK_STMT) {
        let mut ordered: Vec<&Block> = Vec::new();
        for statement in body.children().filter(|node| node.kind() == INIT_STMT) {
            let Some(token) = statement
                .children()
                .find(|child| child.kind() == NAME_REF)
                .and_then(|child| child.first_token())
            else {
                continue;
            };
            if let Some(block) = found.blocks.get(token.text()) {
                ordered.push(block);
            }
        }
        if ordered.len() > 1 {
            report_out_of_order(source, &ordered, out);
        }
    }
}

/// Reports a read of a global that a later block declares.
fn report_out_of_order(source: SourceId, ordered: &[&Block], out: &mut Diagnostics) {
    for (position, block) in ordered.iter().enumerate() {
        let later = &ordered[position + 1..];
        for reference in block
            .node
            .descendants()
            .filter(|node| node.kind() == NAME_REF)
        {
            let Some(token) = reference.first_token() else {
                continue;
            };
            let Some(owner) = later
                .iter()
                .find(|item| item.declares.iter().any(|name| name == token.text()))
            else {
                continue;
            };
            out.push(
                Diagnostic::new(LK0711, source, span_of(&token))
                    .label(format!("`{}` runs after `{}`", owner.name, block.name))
                    .secondary(source, owner.span, "declared here")
                    .note("rule I-16 leaves the order to the programmer")
                    .help(format!("initialize `{}` first", owner.name)),
            );
        }
    }
}

/// Reports whether a program has exactly one `init` function. See rule I-1.
///
/// A program that uses no managed memory needs no marker, because it starts no
/// runtime.
pub fn check_program(
    sources: &[(SourceId, usize)],
    total: usize,
    uses_runtime: bool,
    out: &mut Diagnostics,
) {
    if total > 0 || !uses_runtime {
        return;
    }
    let Some((source, _)) = sources.first() else {
        return;
    };
    out.push(
        Diagnostic::new(LK0700, *source, Span::at(0))
            .label("no function carries the `init` marker")
            .note("rule I-3 puts the runtime startup in that function")
            .help("write `init` before the entry point, as in `init void main(void)`"),
    );
}

/// Returns the names that an item introduces.
fn declared_names(item: &SyntaxNode) -> Vec<String> {
    let mut found = Vec::new();
    for child in item.children() {
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

/// Returns the first name that an item introduces.
fn declared_name(item: &SyntaxNode) -> Option<String> {
    declared_names(item).into_iter().next()
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

/// Returns the span of a token.
fn span_of(token: &SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}
