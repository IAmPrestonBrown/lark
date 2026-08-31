//! Rule M-8 enforcement. An interior pointer under a collector that has none.
//!
//! Rule R-1 makes the transpiler read the capabilities of the collector at
//! build time. A collector that moves objects cannot follow an interior
//! pointer, because the pointer has no slot of its own that a copy can update.
//! The check finds every construction of one and reports `LK0320`.
//!
//! The check runs only when the collector lacks the capability, so a build
//! with the default collector pays nothing and sees nothing.
//!
//! | Form | Example |
//! |---|---|
//! | The address of an element | `&items[3]` |
//! | The address of a field | `&item->value` |
//! | Arithmetic on a managed pointer | `items + 2` |
//! | A step along a managed pointer | `items++` |

// This module walks 31 of the kinds, and a list that long in the header helps
// no reader, so it imports the variants. A module that uses a few names spells
// them out instead.
#![allow(clippy::enum_glob_use)]

use std::collections::BTreeSet;

use lark_diag::{Diagnostic, Diagnostics, LK0320};
use lark_span::{SourceId, Span};
use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, child_tokens};

use crate::caps::Capabilities;

/// Reports every interior pointer that the collector cannot follow.
///
/// The function returns at once when the collector supports them, which is the
/// usual case.
pub fn check(
    source: SourceId,
    root: &SyntaxNode,
    caps: Capabilities,
    syntax_errors: &[Span],
    out: &mut Diagnostics,
) {
    if caps.interior_pointers {
        return;
    }
    let managed = managed_names(root);
    if managed.is_empty() {
        return;
    }

    for node in root.descendants() {
        let span = node_span(&node);
        // Rule DQ-4. A construct the parser could not read has no reliable type.
        if syntax_errors.iter().any(|error| overlaps(span, *error)) {
            continue;
        }
        let Some(form) = interior_form(&node, &managed) else {
            continue;
        };
        out.push(
            Diagnostic::new(LK0320, source, span)
                .label(form.label.clone())
                .note(
                    "a collector that moves an object cannot follow an interior pointer, \
                     because the pointer has no root of its own to rewrite"
                        .to_owned(),
                )
                .help(form.help),
        );
    }
}

/// What one interior pointer looks like, and what to say about it.
struct Form {
    label: String,
    help: String,
}

/// Returns the interior pointer that a node forms, when it forms one.
fn interior_form(node: &SyntaxNode, managed: &BTreeSet<String>) -> Option<Form> {
    match node.kind() {
        // `&items[3]` and `&item->value`.
        PREFIX_EXPR if leading_token(node) == Some(AMP) => {
            let operand = node.children().find(|child| is_expression(child.kind()))?;
            let base = base_name(&operand)?;
            if !managed.contains(&base) {
                return None;
            }
            match operand.kind() {
                INDEX_EXPR => Some(Form {
                    label: format!("this takes the address of an element inside `{base}`"),
                    help: "hold the whole object, and pass the index beside it".to_owned(),
                }),
                FIELD_EXPR => Some(Form {
                    label: format!("this takes the address of a field inside `{base}`"),
                    help: "hold the object itself, and read the field where it is used".to_owned(),
                }),
                _ => None,
            }
        }
        // `items + 2` and `items - 2`.
        BIN_EXPR if matches!(operator(node), Some(PLUS | MINUS)) => {
            let base = node
                .children()
                .filter(|child| is_expression(child.kind()))
                .find_map(|child| pointer_name(&child).filter(|name| managed.contains(name)))?;
            Some(Form {
                label: format!("this moves `{base}` to an address inside its object"),
                help: "index the object where the value is used, as `items[2]`".to_owned(),
            })
        }
        // `items++` and `++items`.
        POSTFIX_EXPR | PREFIX_EXPR if matches!(step_operator(node), Some(PLUS2 | MINUS2)) => {
            let operand = node.children().find(|child| is_expression(child.kind()))?;
            let base = pointer_name(&operand).filter(|name| managed.contains(name))?;
            Some(Form {
                label: format!("this steps `{base}` to an address inside its object"),
                help: "keep an index beside the pointer, and leave the pointer alone".to_owned(),
            })
        }
        _ => None,
    }
}

/// Returns the kind of the first token of a node, trivia skipped.
fn leading_token(node: &SyntaxNode) -> Option<lark_syntax::SyntaxKind> {
    child_tokens(node)
        .find(|token| !token.kind().is_trivia())
        .map(|token| token.kind())
}

/// Returns the operator of a binary expression.
fn operator(node: &SyntaxNode) -> Option<lark_syntax::SyntaxKind> {
    child_tokens(node)
        .map(|token| token.kind())
        .find(|kind| matches!(kind, PLUS | MINUS | STAR | SLASH | PERCENT))
}

/// Returns the step operator of a prefix or postfix expression.
fn step_operator(node: &SyntaxNode) -> Option<lark_syntax::SyntaxKind> {
    child_tokens(node)
        .map(|token| token.kind())
        .find(|kind| matches!(kind, PLUS2 | MINUS2))
}

/// Returns the name when the expression **is** that name, not a read from it.
///
/// `items` gives `items`, and so does `(items)` and `(gc Cell*) items`.
/// `item->value` gives nothing, because it reads a field rather than naming
/// the pointer. The difference decides pointer arithmetic: `items + 2` moves a
/// managed pointer, and `item->value + 2` adds two integers.
fn pointer_name(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        NAME_EXPR | NAME_REF | PATH => base_name(node),
        PAREN_EXPR | CAST_EXPR => node
            .children()
            .filter(|child| is_expression(child.kind()))
            .find_map(|child| pointer_name(&child)),
        _ => None,
    }
}

/// Returns the name at the base of an expression.
///
/// `items[3]` and `item->value` and `items` all give the name they start from.
/// A form that starts from something else gives nothing, because the check
/// then cannot say whether the base is managed.
fn base_name(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        // A `NAME_EXPR` wraps a `NAME_REF` or a `PATH`, and a `PATH` holds two
        // names. The last one is the name that the expression reads.
        NAME_EXPR | NAME_REF | PATH => lark_syntax::all_tokens(node)
            .filter(|token| token.kind() == IDENT)
            .map(|token| token.text().to_owned())
            .last(),
        INDEX_EXPR | FIELD_EXPR | PAREN_EXPR | CAST_EXPR => node
            .children()
            .filter(|child| is_expression(child.kind()))
            .find_map(|child| base_name(&child)),
        _ => None,
    }
}

/// Returns every name in the file that holds a managed pointer.
///
/// The set is per file rather than per scope. A name that is managed in one
/// function and unmanaged in another is rare, and the wider set reports rather
/// than misses. Rule M-8 protects memory, so a report is the safer error.
fn managed_names(root: &SyntaxNode) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for node in root.descendants() {
        if !matches!(node.kind(), DECLARATION | PARAM | FIELD_DECL) {
            continue;
        }
        if !declares_a_managed_pointer(&node) {
            continue;
        }
        for declarator in node.descendants().filter(|item| item.kind() == DECLARATOR) {
            if let Some(name) = declarator_name(&declarator) {
                found.insert(name);
            }
        }
    }
    found
}

/// Reports whether a declaration introduces a managed pointer.
///
/// Rule T-1a puts the `gc` marker in the specifiers or after a `*`. A `new`
/// in the initializer says the same thing without the marker, because rule L-5
/// infers the type.
fn declares_a_managed_pointer(node: &SyntaxNode) -> bool {
    let marked = node
        .descendants()
        .filter(|item| matches!(item.kind(), DECL_SPECIFIERS | POINTER))
        .any(|item| child_tokens(&item).any(|token| token.kind() == IDENT && token.text() == "gc"));
    if marked {
        return true;
    }
    node.descendants()
        .any(|item| matches!(item.kind(), NEW_EXPR | NEW_ARRAY_EXPR))
}

/// Returns the name that a declarator introduces.
fn declarator_name(declarator: &SyntaxNode) -> Option<String> {
    for child in declarator.children() {
        match child.kind() {
            NAME => {
                return child_tokens(&child)
                    .find(|token| token.kind() == IDENT)
                    .map(|token| token.text().to_owned());
            }
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

/// Reports whether a kind is an expression.
fn is_expression(kind: lark_syntax::SyntaxKind) -> bool {
    matches!(
        kind,
        LITERAL_EXPR
            | NAME_EXPR
            | PAREN_EXPR
            | CALL_EXPR
            | INDEX_EXPR
            | FIELD_EXPR
            | METHOD_EXPR
            | PREFIX_EXPR
            | POSTFIX_EXPR
            | BIN_EXPR
            | CAST_EXPR
            | NEW_EXPR
            | NEW_ARRAY_EXPR
    )
}

/// Returns the span of a node.
fn node_span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

/// Reports whether two spans share a byte.
fn overlaps(left: Span, right: Span) -> bool {
    left.start < right.end && right.start < left.end
}

#[cfg(test)]
mod tests {
    // A helper in a test proves a failure by panicking.
    #![allow(clippy::panic)]

    use lark_diag::Diagnostics;
    use lark_span::SourceMap;
    use lark_syntax::{NoNames, parse};

    use super::check;
    use crate::caps::Capabilities;

    /// Runs the check over one file under a collector that has no interior
    /// pointers, and returns the codes it reported.
    fn codes(source: &str) -> Vec<u16> {
        let parsed = parse(source, &NoNames);
        let mut sources = SourceMap::new();
        let Ok(id) = sources.add(std::path::PathBuf::from("t.lark"), source.to_owned()) else {
            return Vec::new();
        };
        let mut out = Diagnostics::new();
        let caps = Capabilities::of("semispace").unwrap_or_default();
        check(id, &parsed.syntax(), caps, &[], &mut out);
        out.items().iter().map(|item| item.code.number()).collect()
    }

    /// The head of every case, so each one names the same managed type.
    const HEAD: &str = "managed struct Cell { gc Cell* next; int value; }\n";

    /// Runs one function body under the moving collector.
    fn body(text: &str) -> Vec<u16> {
        codes(&format!("{HEAD}int f(void) {{ {text} }}"))
    }

    /// covers: M-8, R-1
    #[test]
    fn the_address_of_an_element_is_an_interior_pointer() {
        let found =
            body("gc Cell* items = new Cell[8]; gc Cell* one = &items[3]; return one->value;");
        assert_eq!(found, vec![320]);
    }

    /// covers: M-8, R-1
    #[test]
    fn the_address_of_a_field_is_an_interior_pointer() {
        let found =
            body("gc Cell* item = new Cell { .value = 1 }; gc int* p = &item->value; return *p;");
        assert_eq!(found, vec![320]);
    }

    /// covers: M-8, R-1
    #[test]
    fn arithmetic_on_a_managed_pointer_is_an_interior_pointer() {
        let found =
            body("gc Cell* items = new Cell[8]; gc Cell* two = items + 2; return two->value;");
        assert_eq!(found, vec![320]);
        let back =
            body("gc Cell* items = new Cell[8]; gc Cell* two = items - 1; return two->value;");
        assert_eq!(back, vec![320]);
    }

    /// covers: M-8, R-1
    #[test]
    fn a_step_along_a_managed_pointer_is_an_interior_pointer() {
        let found = body("gc Cell* items = new Cell[8]; items++; return items->value;");
        assert_eq!(found, vec![320]);
    }

    /// A collector with the capability accepts every one of them.
    /// covers: M-8, R-1
    #[test]
    fn a_collector_with_the_capability_reports_nothing() {
        let source = format!(
            "{HEAD}int f(void) {{ gc Cell* items = new Cell[8]; \
             gc Cell* one = &items[3]; return one->value; }}"
        );
        let parsed = parse(&source, &NoNames);
        let mut sources = SourceMap::new();
        let Ok(id) = sources.add(std::path::PathBuf::from("t.lark"), source.clone()) else {
            panic!("cannot add the source");
        };
        for name in ["precise-marksweep", "arena"] {
            let mut out = Diagnostics::new();
            let caps = Capabilities::of(name).unwrap_or_default();
            check(id, &parsed.syntax(), caps, &[], &mut out);
            assert!(out.items().is_empty(), "{name} reported something");
        }
    }

    /// An unmanaged pointer forms no interior pointer, whatever the collector.
    #[test]
    fn an_unmanaged_pointer_reports_nothing() {
        let found = body("int plain[4]; int* p = &plain[1]; return *p;");
        assert!(found.is_empty(), "an unmanaged array reported {found:?}");
    }

    /// Reading through a managed pointer is not the same as taking its address.
    #[test]
    fn reading_a_field_reports_nothing() {
        let found = body(
            "gc Cell* item = new Cell { .value = 1 }; return item->value + item->next->value;",
        );
        assert!(found.is_empty(), "a read reported {found:?}");
    }

    /// An index that the program reads rather than addresses is legal.
    #[test]
    fn reading_an_element_reports_nothing() {
        let found = body("gc Cell* items = new Cell[8]; return items[3].value;");
        assert!(found.is_empty(), "a read reported {found:?}");
    }
}
