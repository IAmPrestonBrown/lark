//! The type rules that phase 3 enforces.
//!
//! The checks report only what the front end can decide. Delivery phase A does
//! not read headers, so an unknown name is not an error here.
//!
//! | Code | Rule |
//! |---|---|
//! | `LK0200` | T-2. `gc` applies only to a pointer type. |
//! | `LK0210` | T-9. An `auto` declaration needs an initializer. |
//! | `LK0211` | T-11. `auto` is not valid in this position. |

use lark_diag::{Diagnostic, Diagnostics, LK0200, LK0210, LK0211};
use lark_span::{SourceId, Span};
use lark_syntax::SyntaxKind::{
    AUTO_KW, DECL_SPECIFIERS, DECL_STMT, DECLARATION, DECLARATOR, EQ, FIELD_DECL, FN_DEF,
    GLOBAL_BLOCK, IDENT, IFACE_METHOD, INIT_DECLARATOR, PARAM,
};
use lark_syntax::{SyntaxNode, child_tokens};

use crate::lower::Lowering;
use crate::ty::TypeStore;

/// Runs every phase 3 check over one module.
///
/// `syntax_errors` holds the spans that the lexer and the parser already
/// reported. Rule DQ-4 suppresses a type diagnostic inside one of them.
pub fn check(
    store: &mut TypeStore,
    source: SourceId,
    root: &SyntaxNode,
    syntax_errors: &[Span],
    out: &mut Diagnostics,
) {
    let common = store.common();
    let mut lowering = Lowering { store, common };

    for node in root.descendants() {
        match node.kind() {
            DECLARATION | FN_DEF | FIELD_DECL | PARAM | IFACE_METHOD => {
                // Rule DQ-4. A construct that the parser could not read has no
                // reliable type.
                let span = node_span(&node);
                if syntax_errors.iter().any(|error| overlaps(span, *error)) {
                    continue;
                }
                check_declaration(&mut lowering, source, &node, out);
            }
            _ => {}
        }
    }
}

/// Reports whether two spans share any byte, or touch at a boundary.
fn overlaps(outer: Span, inner: Span) -> bool {
    inner.start >= outer.start && inner.start <= outer.end
}

/// Checks one declaration.
fn check_declaration(
    lowering: &mut Lowering<'_>,
    source: SourceId,
    node: &SyntaxNode,
    out: &mut Diagnostics,
) {
    let Some(specifiers) = node
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    else {
        return;
    };
    let info = lowering.specifiers(&specifiers);

    if info.is_inference {
        check_inference(source, node, &specifiers, out);
        return;
    }

    // Rule T-2. Every `gc` marker needs a pointer level.
    if info.gc_count == 0 {
        return;
    }
    let plain = crate::lower::Specifiers {
        gc_count: 0,
        ..info
    };
    let declarators = declarators_of(node);
    if declarators.is_empty() {
        report_bad_gc(lowering, source, &specifiers, info.base, info.gc_count, out);
        return;
    }
    for declarator in declarators {
        let (_, ok) = lowering.declarator(&info, &declarator);
        if !ok {
            let (written, _) = lowering.declarator(&plain, &declarator);
            report_bad_gc(lowering, source, &specifiers, written, info.gc_count, out);
        }
    }
}

/// Returns the number of pointer levels at the top of a type.
fn pointer_levels(store: &TypeStore, mut id: crate::ty::TypeId) -> usize {
    let mut levels = 0;
    while let crate::ty::TypeKind::Pointer { target, .. } = store.kind(id) {
        levels += 1;
        id = *target;
    }
    levels
}

/// Checks an `auto` declaration. See rules T-9 and T-11.
fn check_inference(
    source: SourceId,
    node: &SyntaxNode,
    specifiers: &SyntaxNode,
    out: &mut Diagnostics,
) {
    let auto_span = child_tokens(specifiers)
        .find(|token| token.kind() == AUTO_KW)
        .map_or_else(|| node_span(node), |token| token_span(&token));

    // Rule T-11. `auto` belongs to a block scope variable, or to a declaration
    // inside a `@global` block.
    if !inference_position_is_valid(node) {
        out.push(
            Diagnostic::new(LK0211, source, auto_span)
                .label("`auto` infers the type of a variable")
                .note("rule T-11 allows it on a local variable and in a `@global` block"),
        );
        return;
    }

    // Rule T-9. Inference needs something to infer from.
    for declarator in node
        .children()
        .filter(|child| child.kind() == INIT_DECLARATOR)
    {
        let has_initializer = child_tokens(&declarator).any(|token| token.kind() == EQ);
        if !has_initializer {
            let span = declarator
                .children()
                .find(|child| child.kind() == DECLARATOR)
                .map_or_else(|| node_span(&declarator), |child| node_span(&child));
            out.push(
                Diagnostic::new(LK0210, source, span)
                    .label("`auto` takes the type from the initializer")
                    .help("write `= value`, or name the type"),
            );
        }
    }
}

/// Reports whether `auto` inference is legal at this place. See rule T-11.
fn inference_position_is_valid(node: &SyntaxNode) -> bool {
    if node.kind() != DECLARATION {
        return false;
    }
    match node.parent() {
        Some(parent) => matches!(parent.kind(), DECL_STMT | GLOBAL_BLOCK),
        None => false,
    }
}

/// Reports a `gc` marker that found no pointer. See rule T-2.
fn report_bad_gc(
    lowering: &Lowering<'_>,
    source: SourceId,
    specifiers: &SyntaxNode,
    written: crate::ty::TypeId,
    gc_count: usize,
    out: &mut Diagnostics,
) {
    let span = child_tokens(specifiers)
        .find(|token| token.kind() == IDENT && token.text() == "gc")
        .map_or_else(|| node_span(specifiers), |token| token_span(&token));
    let levels = pointer_levels(lowering.store, written);
    let label = if levels == 0 {
        format!(
            "`{}` is not a pointer type",
            lowering.store.display(written)
        )
    } else {
        format!(
            "`{}` has {levels} pointer level, and the declaration carries {gc_count} `gc` markers",
            lowering.store.display(written)
        )
    };
    out.push(
        Diagnostic::new(LK0200, source, span)
            .label(label)
            .note("rule T-1a puts one `gc` on each pointer level"),
    );
}

/// Returns the declarators that belong to a declaration.
fn declarators_of(node: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut found = Vec::new();
    for child in node.children() {
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

/// Returns the span of a node.
fn node_span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

/// Returns the span of a token.
fn token_span(token: &lark_syntax::SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}
