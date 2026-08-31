//! The C that a generic instantiation becomes.
//!
//! Rule G-1 makes a Lark generic monomorphic. One concrete definition exists
//! per distinct set of type arguments, and no runtime machinery exists for a
//! generic.
//!
//! Rule G-10 decides per instantiation whether a conditionally managed record
//! carries an object header. `Box<int>` costs what a plain struct costs, and
//! `Box<gc Person*>` carries a header and a field map.

use std::collections::BTreeMap;

use lark_mono::{Generic, Instance};
use lark_syntax::SyntaxKind::{
    DECL_SPECIFIERS, DECLARATOR, FIELD_DECL, GENERIC_PARAMS, IDENT, NAME, NAME_REF, POINTER,
    STRUCT_BODY, STRUCT_DEF, UNION_DEF,
};
use lark_syntax::SyntaxNode;

/// Returns the substitution map for one instantiation.
pub fn substitutions(generic: &Generic, instance: &Instance) -> BTreeMap<String, String> {
    generic
        .parameters
        .iter()
        .cloned()
        .zip(instance.arguments.iter().map(|text| strip_gc(text)))
        .collect()
}

/// Returns the C form of a type argument, with the Lark marker removed.
///
/// The emitted C has no `gc`, so `gc Person*` becomes `Person*`.
pub fn strip_gc(text: &str) -> String {
    text.replace("gc ", "")
}

/// Reports whether one instantiation of a record needs an object header.
///
/// Rule G-10. The record must carry the `managed` marker, and a field must hold
/// a managed pointer after substitution.
pub fn needs_header(generic: &Generic, instance: &Instance) -> bool {
    if !generic.marked {
        return false;
    }
    if has_direct_managed_field(&generic.node) {
        return true;
    }
    let used = parameters_in_fields(generic);
    instance
        .managed_arguments
        .iter()
        .zip(used.iter())
        .any(|(managed, uses)| *managed && *uses)
}

/// Returns the managed field names of one instantiation, in order.
pub fn managed_fields(generic: &Generic, instance: &Instance) -> Vec<String> {
    let Some(body) = generic
        .node
        .children()
        .find(|child| child.kind() == STRUCT_BODY)
    else {
        return Vec::new();
    };
    let managed_by_parameter: BTreeMap<&String, bool> = generic
        .parameters
        .iter()
        .zip(instance.managed_arguments.iter().copied())
        .collect();

    let mut found = Vec::new();
    for field in body.children().filter(|child| child.kind() == FIELD_DECL) {
        let direct = field_has_gc(&field);
        let by_parameter = field_type_names(&field)
            .iter()
            .any(|name| managed_by_parameter.get(name).copied().unwrap_or(false));
        if !direct && !by_parameter {
            continue;
        }
        for declarator in field.children().filter(|child| child.kind() == DECLARATOR) {
            if let Some(name) = declarator_name(&declarator) {
                found.push(name);
            }
        }
    }
    found
}

/// Reports whether a record holds a field that carries `gc` directly.
fn has_direct_managed_field(record: &SyntaxNode) -> bool {
    let Some(body) = record.children().find(|child| child.kind() == STRUCT_BODY) else {
        return false;
    };
    body.children()
        .filter(|child| child.kind() == FIELD_DECL)
        .any(|field| field_has_gc(&field))
}

/// Reports whether a field declaration carries the `gc` marker.
fn field_has_gc(field: &SyntaxNode) -> bool {
    field
        .descendants_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
        .any(|token| {
            token.kind() == IDENT
                && token.text() == "gc"
                && token
                    .parent()
                    .is_some_and(|parent| matches!(parent.kind(), DECL_SPECIFIERS | POINTER))
        })
}

/// Returns the type names that a field mentions.
fn field_type_names(field: &SyntaxNode) -> Vec<String> {
    field
        .children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .flat_map(|specifiers| specifiers.children().collect::<Vec<_>>())
        .filter(|child| child.kind() == NAME_REF)
        .filter_map(|child| child.first_token())
        .map(|token| token.text().to_owned())
        .collect()
}

/// Returns, per parameter, whether a field mentions it.
fn parameters_in_fields(generic: &Generic) -> Vec<bool> {
    let Some(body) = generic
        .node
        .children()
        .find(|child| child.kind() == STRUCT_BODY)
    else {
        return vec![false; generic.parameters.len()];
    };
    let mentioned: Vec<String> = body
        .children()
        .filter(|child| child.kind() == FIELD_DECL)
        .flat_map(|field| field_type_names(&field))
        .collect();
    generic
        .parameters
        .iter()
        .map(|parameter| mentioned.iter().any(|name| name == parameter))
        .collect()
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

/// Reports whether an item declares a generic.
pub fn declares_a_generic(item: &SyntaxNode) -> bool {
    if item.children().any(|child| {
        child.kind() == DECLARATOR && child.children().any(|node| node.kind() == GENERIC_PARAMS)
    }) {
        return true;
    }
    item.children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .flat_map(|specifiers| specifiers.children().collect::<Vec<_>>())
        .filter(|child| matches!(child.kind(), STRUCT_DEF | UNION_DEF))
        .any(|record| record.children().any(|node| node.kind() == GENERIC_PARAMS))
}

/// Returns the record body of a generic, for the emitter to re-render.
pub fn record_body(generic: &Generic) -> Option<SyntaxNode> {
    generic
        .node
        .children()
        .find(|child| child.kind() == STRUCT_BODY)
}

/// Returns the marker text that names a generic declaration in the output.
pub fn placeholder(generic: &Generic) -> String {
    format!(
        "/* lark: generic {}<{}>, one definition per instantiation */",
        generic.name,
        generic.parameters.join(", ")
    )
}
