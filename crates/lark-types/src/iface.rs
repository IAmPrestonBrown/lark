//! Interfaces, implementations, and the rules that govern them.
//!
//! | Code | Rule |
//! |---|---|
//! | `LK0410` | O-13. The implementation is missing a function. |
//! | `LK0411` | O-13. The implementation declares an extra function. |
//! | `LK0412` | O-14. An interface applies only to a managed struct. |
//! | `LK0413` | O-15. An implementation lives with its interface or its type. |
//! | `LK0420` | O-18. The address of a stack object is not a managed pointer. |
//! | `LK0421` | O-21. The method name is ambiguous across two interfaces. |
//! | `LK0430` | O-12. An interface function needs a receiver. |

use std::collections::BTreeMap;

use lark_diag::{Diagnostic, Diagnostics, LK0410, LK0411, LK0412, LK0413, LK0420, LK0421, LK0430};
use lark_span::{SourceId, Span};
use lark_syntax::SyntaxKind::{
    DECL_SPECIFIERS, DECL_STMT, DECLARATION, DECLARATOR, FN_DEF, GENERIC_ARGS, GENERIC_PARAMS,
    IDENT, IFACE_DEF, IFACE_METHOD, IMPL_DEF, METHOD_EXPR, NAME, NAME_EXPR, NAME_REF, PARAM,
    PARAM_LIST, PATH, POINTER, TYPE_NAME,
};
use lark_syntax::{SyntaxNode, SyntaxToken, child_tokens};

use crate::managed::{Managed, collect as collect_managed};

/// How an interface function takes its receiver. See rule O-11.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Receiver {
    /// `Self this`. The callee gets a copy.
    Value,
    /// `gc Self* this`. The callee can mutate.
    Pointer,
    /// The declaration has no receiver, which rule O-12 forbids.
    Missing,
}

/// One function that an interface declares.
#[derive(Clone, Debug)]
pub struct Method {
    /// The method name.
    pub name: String,
    /// How the function takes its receiver.
    pub receiver: Receiver,
    /// Every parameter after the receiver, as C text.
    pub parameters: Vec<String>,
    /// The result type, as C text.
    pub result: String,
    /// Where the name is written.
    pub span: Span,
}

/// One interface that a module declares.
#[derive(Clone, Debug)]
pub struct Interface {
    /// The interface name.
    pub name: String,
    /// Whether the declaration carries `export`.
    pub exported: bool,
    /// Every function, in declaration order.
    pub methods: Vec<Method>,
    /// The generic parameter names, in order. Empty for a plain interface.
    ///
    /// Rule O-25. Each set of arguments gives one method table, the way rule
    /// G-1 gives one record layout.
    pub parameters: Vec<String>,
    /// Where the name is written.
    pub span: Span,
}

impl Interface {
    /// Returns the function with a name.
    #[must_use]
    pub fn method(&self, name: &str) -> Option<&Method> {
        self.methods.iter().find(|method| method.name == name)
    }
}

/// One implementation that a module declares.
#[derive(Clone, Debug)]
pub struct Implementation {
    /// The interface that the implementation satisfies.
    pub iface: String,
    /// The arguments written after the interface name, as C text.
    ///
    /// Rule O-26. `impl Show<int> for Buf` names one instantiation, and the
    /// emitter builds a method table for that one alone.
    pub iface_args: Vec<String>,
    /// The type that it targets.
    pub target: String,
    /// Every function that it defines, by name.
    pub methods: Vec<Method>,
    /// Where the interface name is written.
    pub span: Span,
    /// Where the target name is written.
    pub target_span: Span,
}

/// Every interface and implementation that one module declares.
#[derive(Clone, Debug, Default)]
pub struct Interfaces {
    /// Every interface, by name.
    pub interfaces: BTreeMap<String, Interface>,
    /// Every implementation, in declaration order.
    pub implementations: Vec<Implementation>,
}

impl Interfaces {
    /// Returns every interface that a type implements.
    #[must_use]
    pub fn interfaces_of(&self, target: &str) -> Vec<&Implementation> {
        self.implementations
            .iter()
            .filter(|item| item.target == target)
            .collect()
    }

    /// Returns the implementations that declare a method name for a type.
    #[must_use]
    pub fn find_method(&self, target: &str, method: &str) -> Vec<&Implementation> {
        self.implementations
            .iter()
            .filter(|item| item.target == target)
            .filter(|item| item.methods.iter().any(|entry| entry.name == method))
            .collect()
    }
}

/// Reads every interface and implementation of one module.
#[must_use]
pub fn collect(root: &SyntaxNode) -> Interfaces {
    let mut found = Interfaces::default();
    for item in root.children() {
        match item.kind() {
            IFACE_DEF => {
                if let Some(interface) = read_interface(&item) {
                    found.interfaces.insert(interface.name.clone(), interface);
                }
            }
            IMPL_DEF => {
                if let Some(implementation) = read_implementation(&item) {
                    found.implementations.push(implementation);
                }
            }
            _ => {}
        }
    }
    found
}

/// Reads one interface declaration.
fn read_interface(item: &SyntaxNode) -> Option<Interface> {
    let name_node = item.children().find(|child| child.kind() == NAME)?;
    let name_token = name_node.first_token()?;
    let exported = child_tokens(item)
        .find(|token| !token.kind().is_trivia())
        .is_some_and(|token| token.kind() == IDENT && token.text() == "export");

    let methods = item
        .children()
        .filter(|child| child.kind() == IFACE_METHOD)
        .filter_map(|child| read_method(&child))
        .collect();

    let parameters = item
        .children()
        .find(|child| child.kind() == GENERIC_PARAMS)
        .map(|list| {
            list.children()
                .filter(|child| child.kind() == NAME)
                .filter_map(|child| child.first_token())
                .map(|token| token.text().to_owned())
                .collect()
        })
        .unwrap_or_default();

    Some(Interface {
        name: name_token.text().to_owned(),
        exported,
        methods,
        parameters,
        span: span_of(&name_token),
    })
}

/// Reads one implementation declaration.
fn read_implementation(item: &SyntaxNode) -> Option<Implementation> {
    let names: Vec<SyntaxToken> = item
        .children()
        .filter(|child| child.kind() == NAME_REF)
        .filter_map(|child| child.first_token())
        .collect();
    let [iface, target] = names.as_slice() else {
        return None;
    };

    // Rule O-26. The first argument list belongs to the interface, and it sits
    // before the `for`, so the tree order decides which one this is.
    let iface_args = item
        .children()
        .find(|child| child.kind() == GENERIC_ARGS)
        .filter(|list| list.text_range().start() < target.text_range().start())
        .map(|list| {
            list.children()
                .filter(|child| child.kind() == TYPE_NAME)
                .map(|child| {
                    let words: Vec<String> = lark_syntax::all_tokens(&child)
                        .filter(|token| !token.kind().is_trivia())
                        .map(|token| token.text().to_owned())
                        .collect();
                    words.join(" ")
                })
                .collect()
        })
        .unwrap_or_default();

    let methods = item
        .children()
        .filter(|child| child.kind() == FN_DEF)
        .filter_map(|child| read_method(&child))
        .collect();

    Some(Implementation {
        iface_args,
        iface: iface.text().to_owned(),
        target: target.text().to_owned(),
        methods,
        span: span_of(iface),
        target_span: span_of(target),
    })
}

/// Reads one function signature, from an interface or an implementation.
fn read_method(item: &SyntaxNode) -> Option<Method> {
    let declarator = item.children().find(|child| child.kind() == DECLARATOR)?;
    let name_node = declarator.children().find(|child| child.kind() == NAME)?;
    let name_token = name_node.first_token()?;

    let result = type_text(
        item.children()
            .find(|child| child.kind() == DECL_SPECIFIERS)
            .as_ref(),
    );
    let list = declarator
        .children()
        .find(|child| child.kind() == PARAM_LIST);
    let params: Vec<SyntaxNode> = list
        .map(|node| {
            node.children()
                .filter(|child| child.kind() == PARAM)
                .collect()
        })
        .unwrap_or_default();

    let receiver = params.first().map_or(Receiver::Missing, receiver_of);
    let parameters = params.iter().skip(1).map(parameter_text).collect();

    Some(Method {
        name: name_token.text().to_owned(),
        receiver,
        parameters,
        result,
        span: span_of(&name_token),
    })
}

/// Returns how a parameter takes the receiver. See rule O-11.
fn receiver_of(param: &SyntaxNode) -> Receiver {
    let Some(specifiers) = param
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)
    else {
        return Receiver::Missing;
    };
    let names_self = specifiers
        .children()
        .filter(|child| child.kind() == NAME_REF)
        .filter_map(|child| child.first_token())
        .any(|token| token.text() == "Self");
    if !names_self {
        // An implementation writes the concrete type in place of `Self`.
        let named = specifiers
            .children()
            .any(|child| matches!(child.kind(), NAME_REF | PATH));
        if !named {
            return Receiver::Missing;
        }
    }
    let is_pointer = param.descendants().any(|node| node.kind() == POINTER);
    if is_pointer {
        Receiver::Pointer
    } else {
        Receiver::Value
    }
}

/// Returns the text of a parameter, as the emitted C writes it.
fn parameter_text(param: &SyntaxNode) -> String {
    let mut out = String::new();
    for token in param
        .descendants_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
    {
        if token.kind().is_trivia() {
            continue;
        }
        if token.kind() == IDENT && matches!(token.text(), "gc" | "managed") {
            continue;
        }
        if !out.is_empty() && token.kind() == IDENT {
            out.push(' ');
        }
        out.push_str(token.text());
    }
    out
}

/// Returns the text of a type, with the Lark markers removed.
fn type_text(specifiers: Option<&SyntaxNode>) -> String {
    let Some(node) = specifiers else {
        return "void".to_owned();
    };
    let mut out = String::new();
    for token in node
        .descendants_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
    {
        if token.kind().is_trivia() {
            continue;
        }
        if token.kind() == IDENT && matches!(token.text(), "gc" | "managed") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(token.text());
    }
    if out.is_empty() {
        "void".to_owned()
    } else {
        out
    }
}

/// Returns the span of a token.
fn span_of(token: &SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

/// Runs every interface check over one module.
pub fn check(
    source: SourceId,
    root: &SyntaxNode,
    imported: &BTreeMap<String, Interfaces>,
    out: &mut Diagnostics,
) {
    let found = collect(root);
    let managed = collect_managed(root);

    check_receivers(source, &found, out);
    check_implementations(source, &found, &managed, imported, out);
    check_ambiguous_calls(source, root, &found, out);
    check_receiver_adaptation(source, root, &found, out);
}

/// Rule O-12. Every interface function needs a receiver.
fn check_receivers(source: SourceId, found: &Interfaces, out: &mut Diagnostics) {
    for interface in found.interfaces.values() {
        for method in &interface.methods {
            if method.receiver != Receiver::Missing {
                continue;
            }
            out.push(
                Diagnostic::new(LK0430, source, method.span)
                    .label("an interface declares no static function")
                    .help("write `Self this` or `gc Self* this` as the first parameter"),
            );
        }
    }
}

/// Rules O-13, O-14, and O-15.
fn check_implementations(
    source: SourceId,
    found: &Interfaces,
    managed: &Managed,
    imported: &BTreeMap<String, Interfaces>,
    out: &mut Diagnostics,
) {
    for item in &found.implementations {
        // Rule O-14. An interface applies only to a managed struct.
        if let Some(record) = managed.records.get(&item.target)
            && !record.marked
        {
            out.push(
                Diagnostic::new(LK0412, source, item.target_span)
                    .label(format!("`{}` carries no object header", item.target))
                    .help(format!("write `managed struct {}`", item.target)),
            );
        }

        // Rule O-15. The implementation lives with its interface or its type.
        let local_iface = found.interfaces.contains_key(&item.iface);
        let local_target = managed.records.contains_key(&item.target);
        if !local_iface && !local_target {
            out.push(
                Diagnostic::new(LK0413, source, item.span)
                    .label(format!(
                        "this module declares neither `{}` nor `{}`",
                        item.iface, item.target
                    ))
                    .note("rule O-15 stops two modules from defining conflicting implementations"),
            );
            continue;
        }

        // Rule O-13. The implementation matches the interface exactly.
        let Some(interface) = find_interface(&item.iface, found, imported) else {
            continue;
        };
        for declared in &interface.methods {
            if item.methods.iter().any(|entry| entry.name == declared.name) {
                continue;
            }
            out.push(
                Diagnostic::new(LK0410, source, item.span)
                    .label(format!("`{}` declares `{}`", interface.name, declared.name))
                    .secondary(source, declared.span, "declared here")
                    .help(format!("define `{}` in this implementation", declared.name)),
            );
        }
        for defined in &item.methods {
            if interface.method(&defined.name).is_some() {
                continue;
            }
            out.push(
                Diagnostic::new(LK0411, source, defined.span)
                    .label(format!(
                        "`{}` declares no `{}`",
                        interface.name, defined.name
                    ))
                    .note("rule O-9 gives an interface no place for an extra function"),
            );
        }
    }
}

/// Returns an interface from this module or from an imported one.
fn find_interface<'a>(
    name: &str,
    found: &'a Interfaces,
    imported: &'a BTreeMap<String, Interfaces>,
) -> Option<&'a Interface> {
    if let Some(interface) = found.interfaces.get(name) {
        return Some(interface);
    }
    imported
        .values()
        .find_map(|other| other.interfaces.get(name))
}

/// Rule O-21. A method name that two interfaces declare needs a prefix.
fn check_ambiguous_calls(
    source: SourceId,
    root: &SyntaxNode,
    found: &Interfaces,
    out: &mut Diagnostics,
) {
    // Two interfaces on one type that declare the same name make the bare form
    // ambiguous, whatever the receiver is.
    let mut seen: BTreeMap<(String, String), usize> = BTreeMap::new();
    for item in &found.implementations {
        for method in &item.methods {
            *seen
                .entry((item.target.clone(), method.name.clone()))
                .or_insert(0) += 1;
        }
    }
    let ambiguous: Vec<String> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|((_, method), _)| method.clone())
        .collect();
    if ambiguous.is_empty() {
        return;
    }

    for call in root.descendants().filter(|node| node.kind() == METHOD_EXPR) {
        let Some(reference) = call.children().find(|child| child.kind() == NAME_REF) else {
            // A `PATH` child means the call already carries the prefix.
            continue;
        };
        let Some(token) = reference.first_token() else {
            continue;
        };
        if !ambiguous.iter().any(|name| name == token.text()) {
            continue;
        }
        let owners: Vec<String> = found
            .implementations
            .iter()
            .filter(|item| item.methods.iter().any(|entry| entry.name == token.text()))
            .map(|item| item.iface.clone())
            .collect();
        out.push(
            Diagnostic::new(LK0421, source, span_of(&token))
                .label(format!(
                    "`{}` and `{}` both declare it",
                    owners[0], owners[1]
                ))
                .help(format!("write `.{}::{}()`", owners[0], token.text())),
        );
    }
}

/// Rule O-18. The address of a stack object is not a managed pointer.
fn check_receiver_adaptation(
    source: SourceId,
    root: &SyntaxNode,
    found: &Interfaces,
    out: &mut Diagnostics,
) {
    // A local of record type lives on the stack. A method with a `gc Self*`
    // receiver needs a managed pointer, and `&local` is not one.
    let mut stack_locals: BTreeMap<String, String> = BTreeMap::new();
    for node in root.descendants() {
        match node.kind() {
            DECL_STMT => {
                if let Some((name, type_name)) = stack_record_local(&node) {
                    stack_locals.insert(name, type_name);
                }
            }
            METHOD_EXPR => {
                let Some(receiver) = node.children().find(|child| child.kind() == NAME_EXPR) else {
                    continue;
                };
                let Some(token) = receiver.first_token() else {
                    continue;
                };
                let Some(type_name) = stack_locals.get(token.text()) else {
                    continue;
                };
                let Some(method) = node
                    .children()
                    .find(|child| matches!(child.kind(), NAME_REF | PATH))
                    .and_then(|child| child.first_token())
                else {
                    continue;
                };
                let wants_pointer = found
                    .find_method(type_name, method.text())
                    .iter()
                    .filter_map(|item| {
                        item.methods
                            .iter()
                            .find(|entry| entry.name == method.text())
                    })
                    .any(|entry| entry.receiver == Receiver::Pointer);
                if !wants_pointer {
                    continue;
                }
                out.push(
                    Diagnostic::new(LK0420, source, span_of(&token))
                        .label(format!("`{}` lives on the stack", token.text()))
                        .note("rule O-18 needs a managed pointer for a `gc Self*` receiver")
                        .help(format!("allocate it with `new {type_name} {{ ... }}`")),
                );
            }
            _ => {}
        }
    }
}

/// Returns the name and record type of a local that lives on the stack.
fn stack_record_local(node: &SyntaxNode) -> Option<(String, String)> {
    let declaration = node.children().find(|child| child.kind() == DECLARATION)?;
    let specifiers = declaration
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)?;
    // A pointer is not a stack object, whatever it refers to.
    if declaration.descendants().any(|item| item.kind() == POINTER) {
        return None;
    }
    let type_name = specifiers
        .children()
        .find(|child| child.kind() == NAME_REF)
        .and_then(|child| child.first_token())
        .map(|token| token.text().to_owned())?;
    let name = declaration
        .descendants()
        .find(|item| item.kind() == NAME)
        .and_then(|item| item.first_token())
        .map(|token| token.text().to_owned())?;
    Some((name, type_name))
}
