//! Pass two checks. Reports what the resolver can decide with certainty.
//!
//! Phase A does not read headers, so a name that no module declares can still
//! come from `#include`. The checks therefore report only what they can decide:
//! a qualified path, an export, and a generic base name in a complete module.

use lark_diag::{Diagnostic, Diagnostics, LK0100, LK0600, LK0610, LK0611, LK0612, LK0613, LK0614};
use lark_span::SourceId;
use lark_syntax::SyntaxKind::{
    self, ARROW, BLOCK_STMT, DECL_SPECIFIERS, DECLARATION, DOT, ENUM_BODY, ENUM_DEF, FN_DEF,
    GENERIC_ARGS, IDENT, IFACE_DEF, NAME_REF, NAMESPACE_DEF, PATH, STRUCT_BODY, STRUCT_DEF,
    TYPE_NAME, TYPEDEF_KW, UNION_DEF,
};
use lark_syntax::{SyntaxNode, SyntaxToken, child_tokens};

use crate::collect::span_of;
use crate::module::{ModuleGraph, ModuleId};

/// Runs every check over one module.
pub fn check(graph: &ModuleGraph, id: ModuleId, out: &mut Diagnostics) {
    let Some(module) = graph.get(id) else {
        return;
    };
    let source = module.source;

    for entry in &module.imports {
        if entry.target.is_none() {
            out.push(
                Diagnostic::new(LK0600, source, entry.import.span)
                    .label(format!(
                        "no file named `{}.lark` on the search path",
                        entry.import.name
                    ))
                    .help("add the directory to `paths.search` in lark.toml"),
            );
        }
    }

    let root = module.parse.syntax();
    check_paths(graph, id, source, &root, out);
    check_type_references(graph, id, source, &root, out);
    check_exported_signatures(graph, id, source, &root, out);
    check_namespace_items(source, &root, out);
}

/// Reports a type definition inside a namespace block. See rule N-20.
fn check_namespace_items(source: SourceId, root: &SyntaxNode, out: &mut Diagnostics) {
    for block in root
        .descendants()
        .filter(|node| node.kind() == NAMESPACE_DEF)
    {
        for item in block.children() {
            let defines_type = match item.kind() {
                IFACE_DEF => true,
                DECLARATION | FN_DEF => item
                    .children()
                    .filter(|child| child.kind() == DECL_SPECIFIERS)
                    .any(|specifiers| {
                        child_tokens(&specifiers).any(|token| token.kind() == TYPEDEF_KW)
                            || specifiers.children().any(|child| {
                                matches!(child.kind(), STRUCT_DEF | UNION_DEF | ENUM_DEF)
                                    && child
                                        .children()
                                        .any(|body| matches!(body.kind(), STRUCT_BODY | ENUM_BODY))
                            })
                    }),
                _ => false,
            };
            if !defines_type {
                continue;
            }
            out.push(
                Diagnostic::new(
                    LK0614,
                    source,
                    lark_span::Span::new(
                        u32::from(item.text_range().start()),
                        u32::from(item.text_range().end()),
                    ),
                )
                .label("a namespace block holds no type definition")
                .note(
                    "rule N-20. a block names functions and variables, and a type takes \
                         its namespace from the directory that holds the file",
                )
                .help("move the definition to the top level of the file"),
            );
        }
    }
}

/// Checks every `module::name` reference.
fn check_paths(
    graph: &ModuleGraph,
    id: ModuleId,
    source: SourceId,
    root: &SyntaxNode,
    out: &mut Diagnostics,
) {
    for path in root.descendants().filter(|node| node.kind() == PATH) {
        if is_member_path(&path) {
            // `x.Greet::say_hi()` names an interface, not a module. Rule O-21.
            continue;
        }
        let names: Vec<SyntaxToken> = child_tokens(&path)
            .filter(|token| token.kind() == IDENT)
            .collect();
        // Rule N-17. The last segment is the name, and every segment before it
        // names what holds it. A path of one segment is no path at all.
        let Some((name_token, prefix)) = names.split_last() else {
            continue;
        };
        let Some(module_token) = prefix.first() else {
            continue;
        };
        let qualifier: String = prefix
            .iter()
            .map(|token| token.text().to_owned())
            .collect::<Vec<_>>()
            .join("::");

        // Rule N-19. A namespace of this module answers before an import does,
        // because the block is closer than any other file.
        if let Some(here) = graph.get(id)
            && here.namespaces.contains(&qualifier)
        {
            let full = format!("{qualifier}::{}", name_token.text());
            if here.table.get(&full).is_none() {
                out.push(
                    Diagnostic::new(LK0611, source, span_of(name_token)).label(format!(
                        "namespace `{qualifier}` declares no `{}`",
                        name_token.text()
                    )),
                );
            }
            continue;
        }

        let Some(target) = graph.import_target(id, &qualifier) else {
            out.push(
                Diagnostic::new(LK0613, source, span_of(module_token))
                    .label(format!("`{qualifier}` names no imported module"))
                    .help(format!("add `@import {qualifier}` to this file")),
            );
            continue;
        };

        let Some(other) = graph.get(target) else {
            continue;
        };
        match other.table.get(name_token.text()) {
            None => out.push(
                Diagnostic::new(LK0611, source, span_of(name_token)).label(format!(
                    "module `{}` declares no `{}`",
                    other.name,
                    name_token.text()
                )),
            ),
            Some(symbol) if !symbol.visibility.is_exported() => out.push(
                Diagnostic::new(LK0611, source, span_of(name_token))
                    .label(format!(
                        "`{}` is private to module `{}`",
                        name_token.text(),
                        other.name
                    ))
                    .help(format!("write `export` before the {}", symbol.kind.word())),
            ),
            Some(_) => {}
        }
    }
}

/// Checks every bare name that appears where a type belongs.
fn check_type_references(
    graph: &ModuleGraph,
    id: ModuleId,
    source: SourceId,
    root: &SyntaxNode,
    out: &mut Diagnostics,
) {
    let Some(module) = graph.get(id) else {
        return;
    };

    for reference in root.descendants().filter(|node| node.kind() == NAME_REF) {
        let Some(parent) = reference.parent() else {
            continue;
        };
        if !is_type_position(parent.kind()) {
            continue;
        }
        let Some(token) = reference.first_token() else {
            continue;
        };
        let name = token.text();
        if module.table.get(name).is_some() {
            continue;
        }

        // Rule N-11 hides an import from a module that imports this one, so a
        // name that an imported module exports still needs its prefix.
        if let Some(owner) = exporting_module(graph, id, name) {
            out.push(
                Diagnostic::new(LK0612, source, span_of(&token))
                    .label(format!("module `{owner}` exports `{name}`"))
                    .help(format!("write `{owner}::{name}`"))
                    .suggest(
                        source,
                        lark_span::Span::at(span_of(&token).start),
                        format!("{owner}::"),
                    ),
            );
            continue;
        }

        // Rule L-15. Without a complete table the name can come from a header.
        let generic = reference
            .next_sibling()
            .is_some_and(|sibling| sibling.kind() == GENERIC_ARGS);
        if generic && !module.table.is_type(name) && !module.has_unread_include {
            out.push(
                Diagnostic::new(LK0100, source, span_of(&token))
                    .label(format!("`{name}` names no type in this module")),
            );
        }
    }
}

/// Checks that an exported signature names no private type. See rule N-10.
fn check_exported_signatures(
    graph: &ModuleGraph,
    id: ModuleId,
    source: SourceId,
    root: &SyntaxNode,
    out: &mut Diagnostics,
) {
    let Some(module) = graph.get(id) else {
        return;
    };

    for item in root.children() {
        if !matches!(item.kind(), DECLARATION | FN_DEF | IFACE_DEF) {
            continue;
        }
        if !is_exported(&item) {
            continue;
        }
        for reference in signature_type_references(&item) {
            let Some(token) = reference.first_token() else {
                continue;
            };
            let Some(symbol) = module.table.get(token.text()) else {
                continue;
            };
            if symbol.kind.is_type() && !symbol.visibility.is_exported() {
                out.push(
                    Diagnostic::new(LK0610, source, span_of(&token))
                        .label(format!("`{}` is private to this module", token.text()))
                        .secondary(source, symbol.span, "declared here")
                        .help(format!("write `export` before the {}", symbol.kind.word())),
                );
            }
        }
    }
}

/// Returns every type reference in the signature of an item.
///
/// A function body is not part of the signature, so the walk stops at a block.
fn signature_type_references(item: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut found = Vec::new();
    let mut stack: Vec<SyntaxNode> = item.children().collect();
    while let Some(node) = stack.pop() {
        if node.kind() == BLOCK_STMT {
            continue;
        }
        if node.kind() == NAME_REF
            && node
                .parent()
                .is_some_and(|parent| is_type_position(parent.kind()))
        {
            found.push(node.clone());
        }
        stack.extend(node.children());
    }
    found
}

/// Returns the name of an imported module that exports a name.
fn exporting_module(graph: &ModuleGraph, id: ModuleId, name: &str) -> Option<String> {
    let module = graph.get(id)?;
    for entry in &module.imports {
        let target = entry.target?;
        let other = graph.get(target)?;
        if other
            .table
            .get(name)
            .is_some_and(|symbol| symbol.visibility.is_exported())
        {
            return Some(other.name.clone());
        }
    }
    None
}

/// Reports whether a path names a member rather than a module.
///
/// `x.Greet::say_hi()` qualifies a method with its interface. See rule O-21.
fn is_member_path(path: &SyntaxNode) -> bool {
    let mut sibling = path.prev_sibling_or_token();
    while let Some(element) = sibling {
        if let Some(token) = element.as_token() {
            if token.kind().is_trivia() {
                sibling = element.prev_sibling_or_token();
                continue;
            }
            return matches!(token.kind(), DOT | ARROW);
        }
        return false;
    }
    false
}

/// Reports whether an item carries the `export` marker.
fn is_exported(item: &SyntaxNode) -> bool {
    child_tokens(item)
        .find(|token| !token.kind().is_trivia())
        .is_some_and(|token| token.kind() == IDENT && token.text() == "export")
}

/// Reports whether a kind holds a type reference.
fn is_type_position(kind: SyntaxKind) -> bool {
    matches!(kind, DECL_SPECIFIERS | GENERIC_ARGS | TYPE_NAME)
}
