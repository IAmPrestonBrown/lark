//! The rules that keep the managed world and the unmanaged world apart.
//!
//! | Code | Rule |
//! |---|---|
//! | `LK0301` | T-5. No implicit conversion between a managed and a raw pointer. |
//! | `LK0310` | M-2. A managed pointer cannot live here. |
//! | `LK0311` | M-3. A managed struct cannot live in unmanaged memory. |
//! | `LK0340` | M-22. A `gc_leaf` function cannot take a managed parameter. |
//! | `LK0400` | O-2. This struct needs the `managed` marker. |
//! | `LK0440` | C-9. An exported signature has no C form. |
//!
//! The checks report only what the front end can decide. Delivery phase A does
//! not read headers, so an unknown type is not an error here.

// This module walks 36 of the kinds, and a list that long in the header helps
// no reader, so it imports the variants. A module that uses a few names spells
// them out instead.
#![allow(clippy::enum_glob_use)]

use std::collections::BTreeSet;

use lark_diag::{Diagnostic, Diagnostics, LK0301, LK0310, LK0311, LK0340, LK0400, LK0440};
use lark_span::{SourceId, Span};
use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, SyntaxToken, child_tokens};

use crate::managed::{Managed, collect};

/// Runs every boundary check over one module.
pub fn check(source: SourceId, root: &SyntaxNode, syntax_errors: &[Span], out: &mut Diagnostics) {
    let managed = collect(root);
    // Rules T-13 and O-24 give an interface value the placement of a managed
    // pointer, so the check needs the names of the interfaces.
    let interfaces = interface_names(root);

    check_records(source, &managed, out);
    let signatures = read_signatures(root);
    check_boundary(source, root, &signatures, out);

    for node in root.descendants() {
        let span = node_span(&node);
        // Rule DQ-4. A construct the parser could not read has no reliable type.
        if syntax_errors.iter().any(|error| overlaps(span, *error)) {
            continue;
        }
        match node.kind() {
            DECLARATION => {
                check_global_placement(source, &node, &interfaces, out);
                check_leaf_parameters(source, &node, out);
                check_exported_abi(source, &node, &managed, &interfaces, out);
            }
            FN_DEF => {
                check_leaf_parameters(source, &node, out);
                check_exported_abi(source, &node, &managed, &interfaces, out);
            }
            CALL_EXPR => check_call(source, &managed, &node, out),
            _ => {}
        }
    }
}

/// Rule O-2. A record that needs a header must carry the `managed` marker.
fn check_records(source: SourceId, managed: &Managed, out: &mut Diagnostics) {
    for record in managed.records.values() {
        if record.marked {
            continue;
        }
        let has_impl = managed.has_impl(&record.name);
        if !record.needs_header(has_impl) {
            continue;
        }
        let reason = if has_impl {
            "an implementation targets it, so it needs a method table".to_owned()
        } else {
            let field = record
                .managed_fields()
                .next()
                .map_or_else(String::new, |field| field.name.clone());
            format!("the field `{field}` is managed, so the collector needs a field map")
        };
        out.push(
            Diagnostic::new(LK0400, source, record.span)
                .label(reason)
                .help(format!("write `managed struct {}`", record.name)),
        );
    }
}

/// Rule M-2. A managed pointer cannot live in a plain global.
fn check_global_placement(
    source: SourceId,
    item: &SyntaxNode,
    interfaces: &BTreeSet<String>,
    out: &mut Diagnostics,
) {
    // A declaration inside a block or a `@global` block is fine.
    let parent = item.parent().map(|node| node.kind());
    if !matches!(parent, Some(SOURCE_FILE)) {
        return;
    }
    if !declares_a_variable(item) {
        return;
    }
    if let Some(marker) = gc_marker(item) {
        out.push(
            Diagnostic::new(LK0310, source, span_of(&marker))
                .label("a managed pointer at file scope has no root")
                .note(
                    "rule M-1 allows one on the stack, in a managed struct, and in a \
                     `@global` block",
                )
                .help("move the declaration into a `@global` block"),
        );
        return;
    }
    // Rules T-13 and O-24. An interface value holds a managed pointer, so the
    // placement rules apply to it the same way.
    let Some(name) = interface_type_name(item, interfaces) else {
        return;
    };
    out.push(
        Diagnostic::new(LK0310, source, span_of(&name))
            .label(format!(
                "an interface value holds a managed pointer, and `{}` at file scope has no root",
                name.text()
            ))
            .note("rules T-13 and O-24 give an interface value the placement of a `gc T*`")
            .help("move the declaration into a `@global` block"),
    );
}

/// Returns the token that names an interface, when a declaration uses one as
/// its type.
fn interface_type_name(item: &SyntaxNode, interfaces: &BTreeSet<String>) -> Option<SyntaxToken> {
    // A pointer to an interface value is an ordinary pointer, not the value.
    if item.descendants().any(|node| node.kind() == POINTER) {
        return None;
    }
    let specifiers = item
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)?;
    specifiers
        .descendants()
        .filter(|node| node.kind() == NAME_REF)
        .filter_map(|node| child_tokens(&node).find(|token| token.kind() == IDENT))
        .find(|token| interfaces.contains(token.text()))
}

/// Returns every interface that the module declares.
fn interface_names(root: &SyntaxNode) -> BTreeSet<String> {
    root.descendants()
        .filter(|node| node.kind() == IFACE_DEF)
        .filter_map(|node| {
            node.children()
                .find(|child| child.kind() == NAME)
                .and_then(|name| child_tokens(&name).find(|token| token.kind() == IDENT))
                .map(|token| token.text().to_owned())
        })
        .collect()
}

/// Rule C-9. An exported signature must have a C form.
///
/// C code calls an exported function, so every parameter and the result must
/// be something C can name and pass. Two Lark types cannot.
///
/// An interface value is two words with no C equivalent, which rule C-11
/// states. A `managed struct` by value copies the payload and leaves the
/// header behind, and rule M-4 puts the header at a negative offset, so the
/// copy is no longer a managed object.
///
/// A pointer to either one is fine. `gc Person*` is a `Person*` in C, which
/// rule C-10 states, and a pointer to an interface value is an ordinary
/// pointer.
fn check_exported_abi(
    source: SourceId,
    item: &SyntaxNode,
    managed: &Managed,
    interfaces: &BTreeSet<String>,
    out: &mut Diagnostics,
) {
    if item.parent().map(|node| node.kind()) != Some(SOURCE_FILE) {
        return;
    }
    if !is_exported(item) {
        return;
    }

    for param in item.descendants().filter(|node| node.kind() == PARAM) {
        // A pointer to either type is an ordinary C pointer.
        if param.descendants().any(|node| node.kind() == POINTER) {
            continue;
        }
        let Some(token) = named_type_of(&param) else {
            continue;
        };
        let name = token.text();
        let reason = if interfaces.contains(name) {
            "an interface value is two words, and C has no name for the pair"
        } else if managed
            .records
            .get(name)
            .is_some_and(|record| record.marked)
        {
            "a managed struct carries a header before its payload, and a copy by \
             value leaves the header behind"
        } else {
            continue;
        };
        out.push(
            Diagnostic::new(LK0440, source, span_of(&token))
                .label(format!("`{name}` has no C form as a parameter"))
                .note(format!("rule C-9. {reason}"))
                .help(format!("take a pointer, as `gc {name}*`")),
        );
    }
}

/// Returns the token that names the type of a parameter.
fn named_type_of(param: &SyntaxNode) -> Option<SyntaxToken> {
    let specifiers = param
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)?;
    specifiers
        .descendants()
        .filter(|node| node.kind() == NAME_REF)
        .find_map(|node| child_tokens(&node).find(|token| token.kind() == IDENT))
}

/// Reports whether an item carries the `export` marker.
fn is_exported(item: &SyntaxNode) -> bool {
    child_tokens(item)
        .find(|token| !token.kind().is_trivia())
        .is_some_and(|token| token.kind() == IDENT && token.text() == "export")
}

/// Rule M-22. A `gc_leaf` function cannot take a managed parameter.
fn check_leaf_parameters(source: SourceId, item: &SyntaxNode, out: &mut Diagnostics) {
    let Some(specifiers) = item
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    else {
        return;
    };
    let is_leaf =
        child_tokens(&specifiers).any(|token| token.kind() == IDENT && token.text() == "gc_leaf");
    if !is_leaf {
        return;
    }
    for parameter in item.descendants().filter(|node| node.kind() == PARAM) {
        let Some(marker) = gc_marker(&parameter) else {
            continue;
        };
        out.push(
            Diagnostic::new(LK0340, source, span_of(&marker))
                .label("a leaf call has no safepoint, so this argument has no root")
                .help("mark the function `gc_safe`, or take a raw pointer"),
        );
    }
}

/// Rules T-5 and M-3. Checks the arguments of one call.
fn check_call(source: SourceId, managed: &Managed, call: &SyntaxNode, out: &mut Diagnostics) {
    let Some(name) = callee_name(call) else {
        return;
    };
    let Some(arguments) = call.children().find(|child| child.kind() == ARG_LIST) else {
        return;
    };

    // Rule M-3. `malloc(sizeof(Person))` puts a managed struct in unmanaged
    // memory, and nothing traces it there.
    if matches!(
        name.as_str(),
        "malloc" | "calloc" | "realloc" | "aligned_alloc"
    ) {
        for sizeof in arguments
            .descendants()
            .filter(|node| node.kind() == SIZEOF_EXPR)
        {
            let Some(type_name) = sizeof_type_name(&sizeof) else {
                continue;
            };
            if !managed.needs_header(&type_name) {
                continue;
            }
            out.push(
                Diagnostic::new(LK0311, source, node_span(&sizeof))
                    .label(format!("`{type_name}` carries an object header"))
                    .note("rule M-3 allows a managed struct on the stack and in the collector heap")
                    .help(format!("write `new {type_name} {{ ... }}`")),
            );
        }
    }
}

/// Reports whether a declaration introduces a variable rather than a prototype.
fn declares_a_variable(item: &SyntaxNode) -> bool {
    declarators_of(item).iter().any(|declarator| {
        !declarator
            .children()
            .any(|child| child.kind() == PARAM_LIST)
    })
}

/// Returns the `gc` marker of a declaration, when it carries one.
fn gc_marker(item: &SyntaxNode) -> Option<SyntaxToken> {
    item.children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .flat_map(|specifiers| child_tokens(&specifiers).collect::<Vec<_>>())
        .find(|token| token.kind() == IDENT && token.text() == "gc")
}

/// Returns the name that a call names, through a path or a plain name.
fn callee_name(call: &SyntaxNode) -> Option<String> {
    let callee = call.children().find(|child| child.kind() != ARG_LIST)?;
    let names: Vec<String> = callee
        .descendants_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
        .filter(|token| token.kind() == IDENT)
        .map(|token| token.text().to_owned())
        .collect();
    names.last().cloned()
}

/// Returns the type name inside a `sizeof`, when it names one.
fn sizeof_type_name(node: &SyntaxNode) -> Option<String> {
    let type_name = node.children().find(|child| child.kind() == TYPE_NAME)?;
    let specifiers = type_name
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)?;
    // A pointer to a managed struct is fine. Only the struct itself is not.
    let declarator = type_name
        .children()
        .find(|child| child.kind() == DECLARATOR);
    if declarator.is_some_and(|node| node.children().any(|child| child.kind() == POINTER)) {
        return None;
    }
    for child in specifiers.children() {
        if matches!(child.kind(), STRUCT_DEF | UNION_DEF) {
            return child
                .children()
                .find(|item| item.kind() == NAME)
                .and_then(|item| item.first_token())
                .map(|token| token.text().to_owned());
        }
        if child.kind() == NAME_REF {
            return child.first_token().map(|token| token.text().to_owned());
        }
    }
    None
}

/// Returns the declarators that belong to an item.
fn declarators_of(item: &SyntaxNode) -> Vec<SyntaxNode> {
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

/// Reports whether two spans meet.
fn overlaps(outer: Span, inner: Span) -> bool {
    inner.start >= outer.start && inner.start <= outer.end
}

/// Returns the span of a node.
fn node_span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

/// Returns the span of a token.
fn span_of(token: &SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

/* -- Rule T-5, the managed boundary -------------------------------------- */

/// What the front end knows about the managed state of a value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// The value is a managed pointer.
    Managed,
    /// The value is a raw pointer or a plain value.
    Raw,
    /// The front end cannot tell, so no rule fires.
    Unknown,
}

/// The signature of one function, as far as the boundary check needs it.
#[derive(Clone, Debug)]
struct Signature {
    parameters: Vec<Kind>,
    variadic: bool,
}

/// Every function signature that a module declares.
type Signatures = std::collections::BTreeMap<String, Signature>;

/// Reads the signature of every function in a module.
fn read_signatures(root: &SyntaxNode) -> Signatures {
    let mut found = Signatures::new();
    for item in root
        .descendants()
        .filter(|node| matches!(node.kind(), DECLARATION | FN_DEF))
    {
        let Some(name) = declared_name(&item) else {
            continue;
        };
        let Some(list) = item.descendants().find(|node| node.kind() == PARAM_LIST) else {
            continue;
        };
        let mut parameters = Vec::new();
        let mut variadic = false;
        for parameter in list.children().filter(|child| child.kind() == PARAM) {
            if child_tokens(&parameter).any(|token| token.kind() == ELLIPSIS) {
                variadic = true;
                continue;
            }
            parameters.push(declaration_kind(&parameter));
        }
        found.insert(
            name,
            Signature {
                parameters,
                variadic,
            },
        );
    }
    found
}

/// Returns the managed state that a declaration gives its declarator.
fn declaration_kind(item: &SyntaxNode) -> Kind {
    let Some(specifiers) = item
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    else {
        return Kind::Unknown;
    };
    let has_gc = child_tokens(&specifiers)
        .any(|token| token.kind() == IDENT && token.text() == "gc")
        || item
            .descendants()
            .filter(|node| node.kind() == POINTER)
            .any(|node| {
                child_tokens(&node).any(|token| token.kind() == IDENT && token.text() == "gc")
            });
    if has_gc {
        return Kind::Managed;
    }
    // A pointer with no marker is a raw pointer.
    let is_pointer = item.descendants().any(|node| node.kind() == POINTER);
    if is_pointer { Kind::Raw } else { Kind::Unknown }
}

/// Returns the name that an item introduces.
fn declared_name(item: &SyntaxNode) -> Option<String> {
    for declarator in declarators_of(item) {
        if let Some(name) = declarator_name(&declarator) {
            return Some(name);
        }
    }
    None
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

/// Checks every call and every initializer for a crossing. See rule T-5.
fn check_boundary(
    source: SourceId,
    root: &SyntaxNode,
    signatures: &Signatures,
    out: &mut Diagnostics,
) {
    let mut locals: std::collections::BTreeMap<String, Kind> = std::collections::BTreeMap::new();

    // A declaration comes before its uses, so one walk in document order builds
    // the map and checks against it at the same time.
    for node in root.descendants() {
        match node.kind() {
            PARAM => {
                if let Some(name) = declared_name(&node) {
                    locals.insert(name, declaration_kind(&node));
                }
            }
            DECLARATION => {
                let kind = declaration_kind(&node);
                if let Some(name) = declared_name(&node) {
                    locals.insert(name, kind);
                }
                check_initializer(source, &node, kind, &locals, out);
            }
            CALL_EXPR => check_arguments(source, &node, signatures, &locals, out),
            _ => {}
        }
    }
}

/// Checks that an initializer does not cross the boundary.
fn check_initializer(
    source: SourceId,
    item: &SyntaxNode,
    target: Kind,
    locals: &std::collections::BTreeMap<String, Kind>,
    out: &mut Diagnostics,
) {
    if target == Kind::Unknown {
        return;
    }
    for declarator in item
        .children()
        .filter(|child| child.kind() == INIT_DECLARATOR)
    {
        let Some(value) = declarator
            .children()
            .find(|child| is_expression(child.kind()))
        else {
            continue;
        };
        let actual = expression_kind(&value, locals);
        report_crossing(source, &value, target, actual, "the declaration", out);
    }
}

/// Checks that every argument of a call matches its parameter.
fn check_arguments(
    source: SourceId,
    call: &SyntaxNode,
    signatures: &Signatures,
    locals: &std::collections::BTreeMap<String, Kind>,
    out: &mut Diagnostics,
) {
    let Some(name) = callee_name(call) else {
        return;
    };
    let Some(signature) = signatures.get(&name) else {
        return;
    };
    let Some(list) = call.children().find(|child| child.kind() == ARG_LIST) else {
        return;
    };

    let arguments: Vec<SyntaxNode> = list
        .children()
        .filter(|child| is_expression(child.kind()))
        .collect();
    for (index, argument) in arguments.iter().enumerate() {
        let Some(expected) = signature.parameters.get(index) else {
            // A variadic argument has no declared type.
            let _ = signature.variadic;
            break;
        };
        let actual = expression_kind(argument, locals);
        report_crossing(source, argument, *expected, actual, "the parameter", out);
    }
}

/// Reports a crossing between the managed world and the unmanaged world.
fn report_crossing(
    source: SourceId,
    value: &SyntaxNode,
    expected: Kind,
    actual: Kind,
    subject: &str,
    out: &mut Diagnostics,
) {
    if expected == Kind::Unknown || actual == Kind::Unknown || expected == actual {
        return;
    }
    let (from, to, cast) = if actual == Kind::Managed {
        ("a managed pointer", "a raw pointer", "(void*)")
    } else {
        ("a raw pointer", "a managed pointer", "(gc void*)")
    };
    out.push(
        Diagnostic::new(LK0301, source, node_span(value))
            .label(format!("this is {from}, and {subject} takes {to}"))
            .note("rule T-5 allows no implicit conversion in either direction")
            .help(format!("write the cast `{cast}`"))
            .suggest(source, Span::at(node_span(value).start), cast.to_owned()),
    );
}

/// Returns the managed state of an expression.
fn expression_kind(value: &SyntaxNode, locals: &std::collections::BTreeMap<String, Kind>) -> Kind {
    match value.kind() {
        // Rules O-4 and O-6 both yield a managed pointer.
        NEW_EXPR | NEW_ARRAY_EXPR => Kind::Managed,
        CAST_EXPR => match value.children().find(|child| child.kind() == TYPE_NAME) {
            Some(type_name) => declaration_kind(&type_name),
            None => Kind::Unknown,
        },
        PAREN_EXPR => match value.children().find(|child| is_expression(child.kind())) {
            Some(inner) => expression_kind(&inner, locals),
            None => Kind::Unknown,
        },
        NAME_EXPR => {
            let name = value
                .descendants_with_tokens()
                .filter_map(lark_syntax::NodeOrToken::into_token)
                .find(|token| token.kind() == IDENT)
                .map(|token| token.text().to_owned());
            match name.and_then(|item| locals.get(&item).copied()) {
                Some(kind) => kind,
                None => Kind::Unknown,
            }
        }
        // A string literal has static storage, so rule T-8 accepts it either way.
        _ => Kind::Unknown,
    }
}

/// Reports whether a node kind is an expression.
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
