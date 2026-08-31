//! Monomorphization.
//!
//! Rule G-1 makes a Lark generic monomorphic. The transpiler emits one concrete
//! C definition for each distinct set of type arguments, and no runtime
//! machinery exists for a generic.
//!
//! The pass runs over the whole program, because rule G-7 shares one definition
//! across it. Each instantiation belongs to the module that declares the
//! generic.

pub mod mangle;

use std::collections::{BTreeMap, BTreeSet};

use lark_diag::{Diagnostic, Diagnostics, LK0400, LK0500, LK0501, LK0502};
use lark_resolve::ModuleGraph;
use lark_span::{SourceId, Span};
use lark_syntax::SyntaxKind::{
    CALL_EXPR, DECL_SPECIFIERS, DECLARATION, DECLARATOR, FN_DEF, GENERIC_ARGS, GENERIC_PARAMS,
    IDENT, IFACE_DEF, NAME, NAME_EXPR, NAME_REF, PATH, STRUCT_BODY, STRUCT_DEF, TYPE_NAME,
    UNION_DEF,
};
use lark_syntax::{SyntaxNode, child_tokens};

/// How deep one instantiation can reach into another. See rule G-8.
pub const DEPTH_LIMIT: usize = 32;

/// What a generic declaration introduces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A `struct` or a `union`.
    Record,
    /// A function.
    Function,
    /// An interface. Rule O-25 gives each instantiation its own method table.
    Interface,
}

/// One generic declaration.
#[derive(Clone, Debug)]
pub struct Generic {
    /// The name.
    pub name: String,
    /// The module that declares it.
    pub module: String,
    /// The handle of that module's text.
    pub source: SourceId,
    /// What it introduces.
    pub kind: Kind,
    /// The parameter names, in order.
    pub parameters: Vec<String>,
    /// The node that holds the whole declaration.
    pub node: SyntaxNode,
    /// Whether the declaration carries `export`.
    pub exported: bool,
    /// Whether the declaration carries the `managed` marker.
    pub marked: bool,
}

/// One instantiation of a generic.
#[derive(Clone, Debug)]
pub struct Instance {
    /// The generic name.
    pub name: String,
    /// The module that declares the generic, which also emits this instance.
    pub module: String,
    /// What it introduces.
    pub kind: Kind,
    /// The type arguments, as C text.
    pub arguments: Vec<String>,
    /// Whether each argument is a managed pointer.
    pub managed_arguments: Vec<bool>,
    /// The C name of the instance. See rule X-5a.
    pub mangled: String,
    /// Where the program wrote the instantiation.
    ///
    /// Rule DQ-2 reports this beside an error that comes from the substituted
    /// body, so a reader sees which use caused the failure.
    pub span: Span,
    /// The file that holds the instantiation.
    pub source: SourceId,
}

impl Instance {
    /// Reports whether the instance needs an object header.
    ///
    /// Rule G-10 decides per instantiation. An instance with no managed field
    /// carries no header and costs what a plain struct costs.
    #[must_use]
    pub fn needs_header(&self, marked: bool, field_uses_parameter: &[bool]) -> bool {
        if !marked {
            return false;
        }
        self.managed_arguments
            .iter()
            .zip(field_uses_parameter)
            .any(|(managed, used)| *managed && *used)
    }
}

/// Every generic and every instantiation of one program.
#[derive(Clone, Debug, Default)]
pub struct Program {
    /// Every generic declaration, by name.
    pub generics: BTreeMap<String, Generic>,
    /// Every instantiation, by the module that emits it.
    pub instances: BTreeMap<String, Vec<Instance>>,
}

impl Program {
    /// Returns the instantiations that one module emits.
    pub fn instances_of(&self, module: &str) -> &[Instance] {
        self.instances.get(module).map_or(&[], Vec::as_slice)
    }

    /// Returns the generic that a name introduces.
    #[must_use]
    pub fn generic(&self, name: &str) -> Option<&Generic> {
        self.generics.get(name)
    }
}

/// Runs the pass over a whole program.
pub fn collect(graph: &ModuleGraph, out: &mut Diagnostics) -> Program {
    let mut program = Program::default();

    for module in graph.modules() {
        let root = module.parse.syntax();
        for item in root.children() {
            read_generic(&item, &module.name, module.source, &mut program);
        }
    }

    for module in graph.modules() {
        let root = module.parse.syntax();
        // Rule G-2 needs to tell a type from a value, and the module table
        // already answers that.
        let values: BTreeSet<String> = module
            .table
            .iter()
            .filter(|symbol| !symbol.kind.is_type())
            .map(|symbol| symbol.name.clone())
            .collect();
        read_instances(&root, module.source, &values, &mut program, out);
    }

    for list in program.instances.values_mut() {
        list.sort_by(|left, right| left.mangled.cmp(&right.mangled));
        list.dedup_by(|left, right| left.mangled == right.mangled);
        order_instances(list);
    }
    program
}

/// Puts an instantiation after the instantiations that its fields name.
///
/// Rule G-1 and rule X-6a. `Box<Box<int>>` holds a `Box<int>` by value, and C
/// needs the complete type before the field. The name order that the sort
/// gives is not that order, so this pass repairs it.
fn order_instances(list: &mut Vec<Instance>) {
    let mut done: Vec<Instance> = Vec::with_capacity(list.len());
    let mut rest: Vec<Instance> = std::mem::take(list);

    while !rest.is_empty() {
        let ready = |item: &Instance, rest: &[Instance]| {
            !rest.iter().any(|other| {
                other.mangled != item.mangled
                    && item
                        .arguments
                        .iter()
                        .any(|text| text.contains(other.mangled.as_str()))
            })
        };
        let Some(index) = rest.iter().position(|item| ready(item, &rest)) else {
            // A cycle cannot happen, because rule G-8 bounds the depth. If one
            // ever does, keep every instance rather than drop it.
            done.append(&mut rest);
            break;
        };
        done.push(rest.remove(index));
    }
    *list = done;
}

/// Records one generic declaration.
fn read_generic(item: &SyntaxNode, module: &str, source: SourceId, program: &mut Program) {
    let exported = child_tokens(item)
        .find(|token| !token.kind().is_trivia())
        .is_some_and(|token| token.kind() == IDENT && token.text() == "export");

    // Rule O-25. A generic interface is a declaration of its own, so it never
    // reaches the specifier walk below.
    if item.kind() == IFACE_DEF
        && let Some(parameters) = parameter_names(item)
        && let Some(name) = item
            .children()
            .find(|child| child.kind() == NAME)
            .and_then(|node| node.first_token())
            .map(|token| token.text().to_owned())
    {
        program.generics.insert(
            name.clone(),
            Generic {
                name,
                module: module.to_owned(),
                source,
                kind: Kind::Interface,
                parameters,
                node: item.clone(),
                exported,
                marked: false,
            },
        );
        return;
    }

    // A generic record sits inside the declaration specifiers.
    for specifiers in item
        .children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
    {
        for record in specifiers
            .children()
            .filter(|child| matches!(child.kind(), STRUCT_DEF | UNION_DEF))
        {
            let Some(parameters) = parameter_names(&record) else {
                continue;
            };
            let Some(name) = tag_name(&record) else {
                continue;
            };
            let marked = child_tokens(&record)
                .any(|token| token.kind() == IDENT && token.text() == "managed");
            program.generics.insert(
                name.clone(),
                Generic {
                    name,
                    module: module.to_owned(),
                    source,
                    kind: Kind::Record,
                    parameters,
                    node: record.clone(),
                    exported,
                    marked,
                },
            );
        }
    }

    // A generic function carries its parameters in the declarator.
    if matches!(item.kind(), FN_DEF | DECLARATION)
        && let Some(declarator) = item.children().find(|child| child.kind() == DECLARATOR)
        && let Some(parameters) = parameter_names(&declarator)
        && let Some(name) = declarator
            .children()
            .find(|child| child.kind() == NAME)
            .and_then(|node| node.first_token())
    {
        program.generics.insert(
            name.text().to_owned(),
            Generic {
                name: name.text().to_owned(),
                module: module.to_owned(),
                source,
                kind: Kind::Function,
                parameters,
                node: item.clone(),
                exported,
                marked: false,
            },
        );
    }
}

/// Returns the generic parameter names of a node, when it has any.
fn parameter_names(node: &SyntaxNode) -> Option<Vec<String>> {
    let list = node
        .children()
        .find(|child| child.kind() == GENERIC_PARAMS)?;
    let names: Vec<String> = list
        .children()
        .filter(|child| child.kind() == NAME)
        .filter_map(|child| child.first_token())
        .map(|token| token.text().to_owned())
        .collect();
    if names.is_empty() { None } else { Some(names) }
}

/// Returns the tag name of a record.
fn tag_name(record: &SyntaxNode) -> Option<String> {
    record
        .children()
        .find(|child| child.kind() == NAME)
        .and_then(|node| node.first_token())
        .map(|token| token.text().to_owned())
}

/// Records every instantiation that one module writes.
fn read_instances(
    root: &SyntaxNode,
    source: SourceId,
    values: &BTreeSet<String>,
    program: &mut Program,
    out: &mut Diagnostics,
) {
    let mut pending: Vec<(String, Vec<String>, usize, Span)> = Vec::new();

    for list in root
        .descendants()
        .filter(|node| node.kind() == GENERIC_ARGS)
    {
        let Some(name) = generic_name_before(&list) else {
            continue;
        };
        let arguments = argument_texts(&list);
        pending.push((name, arguments, 0, node_span(&list)));
    }

    // Rule G-6. A call with no explicit arguments infers them.
    for call in root.descendants().filter(|node| node.kind() == CALL_EXPR) {
        if let Some((name, arguments, span)) = infer_call(program, &call, source, out) {
            pending.push((name, arguments, 0, span));
        }
    }

    let mut seen: BTreeSet<String> = BTreeSet::new();
    while let Some((name, arguments, depth, span)) = pending.pop() {
        // Rule G-8. An instantiation that reaches too deep stops the pass.
        if depth > DEPTH_LIMIT {
            out.push(
                Diagnostic::new(LK0500, source, span)
                    .label(format!("`{name}` reached depth {depth}"))
                    .note(format!("rule G-8 stops the pass at depth {DEPTH_LIMIT}")),
            );
            continue;
        }
        let Some(generic) = program.generics.get(&name).cloned() else {
            continue;
        };

        // Rule G-2. Every argument is a type.
        let mut bad = false;
        for argument in &arguments {
            if is_type_text(argument) && !names_a_value(argument, values) {
                continue;
            }
            out.push(
                Diagnostic::new(LK0502, source, span)
                    .label(format!("`{argument}` names no type"))
                    .note("rule L-7 makes every generic argument a type"),
            );
            bad = true;
        }
        if bad {
            continue;
        }

        // An argument that is itself a generic needs its own instantiation.
        for argument in &arguments {
            if let Some((inner, inner_arguments)) = split_instantiation(argument) {
                pending.push((inner, inner_arguments, depth + 1, span));
            }
        }

        // Rule G-1. From here the arguments are C text, so a nested generic
        // becomes the name of its instantiation.
        let arguments: Vec<String> = arguments
            .iter()
            .map(|text| resolve(program, text))
            .collect();

        let mangled = mangle::instance(&generic.module, &name, &arguments);
        if !seen.insert(mangled.clone()) {
            continue;
        }

        let managed_arguments: Vec<bool> = arguments
            .iter()
            .map(|text| text.trim_start().starts_with("gc "))
            .collect();

        // Rule G-11. A generic struct with no `managed` marker, whose
        // instantiation puts a `gc` field inside it, needs the marker. The
        // collector needs a field map for that instance.
        if generic.kind == Kind::Record && !generic.marked {
            let used = parameters_in_fields(&generic);
            let managed_field = managed_arguments
                .iter()
                .zip(used.iter())
                .any(|(managed, uses)| *managed && *uses);
            if managed_field {
                out.push(
                    Diagnostic::new(LK0400, source, span)
                        .label(format!(
                            "this instantiation of `{name}` holds a managed field"
                        ))
                        .note(format!(
                            "rule G-11. `{name}` carries no `managed` marker, and the \
                             collector needs a field map for an object that holds a `gc` field"
                        ))
                        .help(format!("write `managed struct {name}<...>`")),
                );
            }
        }

        program
            .instances
            .entry(generic.module.clone())
            .or_default()
            .push(Instance {
                name,
                module: generic.module.clone(),
                kind: generic.kind,
                arguments,
                managed_arguments,
                mangled,
                span,
                source,
            });
    }
}

/// Reports, for each parameter, whether a field of the record uses it.
///
/// Rule G-10 and rule G-11 both ask this. A parameter that appears only in a
/// method signature puts no field in the object.
fn parameters_in_fields(generic: &Generic) -> Vec<bool> {
    let Some(body) = generic
        .node
        .children()
        .find(|child| child.kind() == STRUCT_BODY)
    else {
        return vec![false; generic.parameters.len()];
    };
    let text = body.text().to_string();
    generic
        .parameters
        .iter()
        .map(|parameter| {
            text.split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|word| word == parameter)
        })
        .collect()
}

/// Returns the generic name that a `GENERIC_ARGS` node belongs to.
fn generic_name_before(list: &SyntaxNode) -> Option<String> {
    let mut sibling = list.prev_sibling();
    while let Some(node) = sibling {
        if matches!(node.kind(), NAME_REF | PATH) {
            return node.first_token().map(|token| token.text().to_owned());
        }
        sibling = node.prev_sibling();
    }
    None
}

/// Returns the text of each argument in a list.
fn argument_texts(list: &SyntaxNode) -> Vec<String> {
    list.children()
        .filter(|child| child.kind() == TYPE_NAME)
        .map(|child| type_text(&child))
        .collect()
}

/// Returns the C text of a type, with the `gc` marker kept.
///
/// The marker decides the mangle, so every caller must use this one function.
/// See rule X-5a.
pub fn type_text(node: &SyntaxNode) -> String {
    let mut out = String::new();
    for token in node
        .descendants_with_tokens()
        .filter_map(lark_syntax::NodeOrToken::into_token)
    {
        if token.kind().is_trivia() {
            continue;
        }
        if token.kind() == IDENT && token.text() == "managed" {
            continue;
        }
        if !out.is_empty() && token.kind() == IDENT {
            out.push(' ');
        }
        out.push_str(token.text());
    }
    out
}

/// Reports whether a text names a type rather than a value.
fn is_type_text(text: &str) -> bool {
    let head = text.trim_start_matches("gc ").trim();
    let head = head.trim_end_matches(['*', ' ']);
    !head.is_empty()
        && head
            .chars()
            .next()
            .is_some_and(|item| item.is_ascii_alphabetic() || item == '_')
        && !head
            .chars()
            .next()
            .is_some_and(|item| item.is_ascii_digit())
}

/// Reports whether a type argument names a value rather than a type.
fn names_a_value(text: &str, values: &BTreeSet<String>) -> bool {
    let head = text
        .trim_start_matches("gc ")
        .trim()
        .trim_end_matches(['*', ' ']);
    values.contains(head)
}

/// Splits `Name<Arg, ...>` into its parts, when the text is one.
fn split_instantiation(text: &str) -> Option<(String, Vec<String>)> {
    let open = text.find('<')?;
    let close = matching_angle(text, open)?;
    let name = text[..open].trim().to_owned();
    let arguments = split_arguments(&text[open + 1..close])
        .into_iter()
        .map(|item| item.trim().to_owned())
        .collect();
    Some((name, arguments))
}

/// Returns the index of the `>` that closes the `<` at `open`.
///
/// A plain search for the last `>` cannot read `Box<Pair<int, char>>`, so
/// every caller counts the depth instead.
fn matching_angle(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in text.char_indices().skip(open) {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits an argument list on the commas that are not inside a nested list.
fn split_arguments(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// Replaces every `Name<Args>` in a type with the name of its instantiation.
///
/// Rule G-1. A generic has no C form, so `Box<Box<int>>` must name the inner
/// instantiation rather than repeat the Lark spelling, which no C compiler
/// reads. The result is the name that `Program::instances` also carries, so
/// the use and the definition agree by construction.
#[must_use]
pub fn resolve(program: &Program, text: &str) -> String {
    let Some(open) = text.find('<') else {
        return text.to_owned();
    };
    let head = &text[..open];
    let start = head
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map_or(0, |index| index + 1);
    let name = &head[start..];
    let (Some(generic), Some(close)) = (program.generic(name), matching_angle(text, open)) else {
        return text.to_owned();
    };

    let arguments: Vec<String> = split_arguments(&text[open + 1..close])
        .into_iter()
        // An argument can itself be a generic, so it takes the same pass.
        .map(|item| resolve(program, item.trim()))
        .collect();
    let mangled = mangle::instance(&generic.module, name, &arguments);
    format!("{}{mangled}{}", &text[..start], &text[close + 1..])
}

/// Infers the type arguments of a call with no explicit list. See rule G-6.
fn infer_call(
    program: &Program,
    call: &SyntaxNode,
    source: SourceId,
    out: &mut Diagnostics,
) -> Option<(String, Vec<String>, Span)> {
    let callee = call.children().find(|child| child.kind() == NAME_EXPR)?;
    if callee.children().any(|child| child.kind() == GENERIC_ARGS) {
        // The call gives the arguments, so nothing needs inference.
        return None;
    }
    let name = callee.first_token()?.text().to_owned();
    let generic = program.generics.get(&name)?;
    if generic.kind != Kind::Function {
        return None;
    }

    // Rule G-6a. Version 1 asks for the list rather than inferring it.
    out.push(
        Diagnostic::new(LK0501, source, node_span(call))
            .label(format!(
                "`{name}` takes {} type arguments",
                generic.parameters.len()
            ))
            .note("rule G-6a leaves inference to a later version")
            .help(format!(
                "write `{name}<{}>(...)`",
                generic.parameters.join(", ")
            )),
    );
    None
}

/// Returns the span of a node.
fn node_span(node: &SyntaxNode) -> Span {
    let range = node.text_range();
    Span::new(u32::from(range.start()), u32::from(range.end()))
}

#[cfg(test)]
mod tests {
    use super::{matching_angle, split_arguments, split_instantiation};

    #[test]
    fn an_angle_matches_across_a_nested_list() {
        let text = "Box<Pair<int, char>>*";
        let close = matching_angle(text, 3).expect("a close angle");
        assert_eq!(&text[3..=close], "<Pair<int, char>>");
    }

    #[test]
    fn a_split_keeps_a_nested_list_whole() {
        assert_eq!(split_arguments("int, char"), vec!["int", " char"]);
        assert_eq!(
            split_arguments("int, Pair<char, long>"),
            vec!["int", " Pair<char, long>"]
        );
    }

    #[test]
    fn an_instantiation_splits_at_the_outer_angle() {
        let (name, arguments) = split_instantiation("Box<Box<int>>").expect("an instantiation");
        assert_eq!(name, "Box");
        assert_eq!(arguments, vec!["Box<int>"]);
    }

    #[test]
    fn a_plain_type_is_no_instantiation() {
        assert!(split_instantiation("int").is_none());
    }
}
