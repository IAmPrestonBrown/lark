//! The machinery that an interface needs in the emitted C.
//!
//! | Source | Emitted C |
//! |---|---|
//! | `iface I` | A method table type, a unique id, and a fat pointer type |
//! | `impl I for T` | One function per method, plus a thunk with an erased receiver |
//! | `x.m(a)` on a concrete type | A direct call. Rule O-19. |
//! | `x.m(a)` on an interface value | A call through the table. Rule O-20. |
//!
//! A method table entry cannot hold the written signature, because `Self`
//! differs per implementation. Each method therefore gets a thunk that takes
//! `void *` and calls the real function. Casting a function pointer to another
//! signature and calling it is undefined in C, and a thunk is not.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use std::fmt::Write as _;

use lark_types::iface::{Implementation, Interface, Interfaces, Receiver};

/// Returns the C name of the method table type for an interface.
pub fn vtable_type(module: &str, iface: &str) -> String {
    format!("lk_{module}__{iface}__vtable")
}

/// Returns the C name of the unique id for an interface.
pub fn iface_id(module: &str, iface: &str) -> String {
    format!("lk_{module}__{iface}__id")
}

/// Returns the C name of one method implementation.
pub fn method_name(module: &str, iface: &str, target: &str, method: &str) -> String {
    format!("lk_{module}__{iface}__{target}__{method}")
}

/// Returns the C name of the thunk for one method.
pub fn thunk_name(module: &str, iface: &str, target: &str, method: &str) -> String {
    format!("{}__thunk", method_name(module, iface, target, method))
}

/// Returns the C name of one method table instance.
pub fn vtable_name(module: &str, iface: &str, target: &str) -> String {
    format!("lk_{module}__{iface}__{target}__vt")
}

/// Returns the C name of the interface table of a type.
pub fn itable_name(module: &str, target: &str) -> String {
    format!("lk_{module}__{target}__itabs")
}

/// Returns the declarations that one interface needs.
///
/// The fat pointer carries the object and its method table. Rule T-12 makes it
/// two words, so C cannot call a function that takes one.
pub fn interface_declarations(module: &str, interface: &Interface) -> String {
    let name = &interface.name;
    let vtable = vtable_type(module, name);
    let mut out = String::new();

    let _ = writeln!(out, "typedef struct {vtable} {{");
    for method in &interface.methods {
        let mut parameters = vec!["void *".to_owned()];
        parameters.extend(method.parameters.iter().map(|item| erase_name(item)));
        let _ = writeln!(
            out,
            "    {} (*{})({});",
            method.result,
            method.name,
            parameters.join(", ")
        );
    }
    let _ = writeln!(out, "}} {vtable};");
    let _ = writeln!(
        out,
        "typedef struct {name} {{ void *obj; const {vtable} *vt; }} {name};"
    );
    out
}

/// Returns the unique id that an interface needs. See rule O-23.
pub fn interface_id_definition(module: &str, interface: &Interface) -> String {
    let symbol = iface_id(module, &interface.name);
    format!("const char {symbol} = 0;\n")
}

/// Returns the declaration of an interface id, for a header.
pub fn interface_id_declaration(module: &str, interface: &Interface) -> String {
    format!("extern const char {};\n", iface_id(module, &interface.name))
}

/// Returns the thunks and the method table for one implementation.
pub fn implementation_tables(module: &str, item: &Implementation, interface: &Interface) -> String {
    let mut out = String::new();
    let target = &item.target;
    let iface = &item.iface;

    for method in &interface.methods {
        let Some(defined) = item.methods.iter().find(|entry| entry.name == method.name) else {
            continue;
        };
        let real = method_name(module, iface, target, &method.name);
        let thunk = thunk_name(module, iface, target, &method.name);

        let mut parameters = vec!["void *lk_self".to_owned()];
        let mut arguments = Vec::new();
        arguments.push(match defined.receiver {
            // The thunk holds the receiver form that the programmer wrote.
            Receiver::Value => format!("*({target} *)lk_self"),
            _ => format!("({target} *)lk_self"),
        });
        for (index, parameter) in method.parameters.iter().enumerate() {
            let name = format!("lk_arg{index}");
            parameters.push(rename_parameter(parameter, &name));
            arguments.push(name);
        }

        let returns = if method.result.trim() == "void" {
            ""
        } else {
            "return "
        };
        let _ = writeln!(
            out,
            "static {} {thunk}({}) {{",
            method.result,
            parameters.join(", ")
        );
        let _ = writeln!(out, "    {returns}{real}({});", arguments.join(", "));
        let _ = writeln!(out, "}}");
    }

    // The table has external linkage, because rule X-5a reserves the name and
    // an earlier item needs a declaration of it.
    let vtable = vtable_name(module, iface, target);
    let _ = writeln!(out, "const {} {vtable} = {{", vtable_type(module, iface));
    for method in &interface.methods {
        let _ = writeln!(
            out,
            "    {},",
            thunk_name(module, iface, target, &method.name)
        );
    }
    let _ = writeln!(out, "}};");
    out
}

/// Returns the interface table of one type. See rule O-23.
pub fn itable_definition(module: &str, target: &str, items: &[&Implementation]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "static const lark_itable_ent {}[] = {{",
        itable_name(module, target)
    );
    for item in items {
        let _ = writeln!(
            out,
            "    {{ &{}, &{} }},",
            iface_id(module, &item.iface),
            vtable_name(module, &item.iface, target)
        );
    }
    let _ = writeln!(out, "}};");
    out
}

/// What a call site knows about its receiver.
#[derive(Clone, Copy, Debug)]
pub struct CallSite<'a> {
    /// The module that holds the implementation.
    pub module: &'a str,
    /// The type of the receiver.
    pub target: &'a str,
    /// The method name.
    pub method: &'a str,
    /// The interface prefix, when the call carries one. See rule O-21.
    pub iface: Option<&'a str>,
    /// The receiver, as C text.
    pub receiver: &'a str,
    /// Whether the receiver is a pointer.
    pub receiver_is_pointer: bool,
}

/// Returns the direct call that a concrete receiver produces. See rule O-19.
pub fn direct_call(site: CallSite<'_>, interfaces: &Interfaces, arguments: &str) -> Option<String> {
    let CallSite {
        module,
        target,
        method,
        iface,
        receiver,
        receiver_is_pointer,
    } = site;
    let candidates = interfaces.find_method(target, method);
    let item = match iface {
        Some(name) => candidates.into_iter().find(|entry| entry.iface == name)?,
        None => candidates.into_iter().next()?,
    };
    let defined = item.methods.iter().find(|entry| entry.name == method)?;

    // Rule O-18. The receiver adapts to the form that the method declares.
    let value = match (defined.receiver, receiver_is_pointer) {
        (Receiver::Value, true) => format!("*{receiver}"),
        (Receiver::Pointer | Receiver::Missing, false) => format!("&{receiver}"),
        _ => receiver.to_owned(),
    };

    let call = method_name(module, &item.iface, target, method);
    if arguments.trim().is_empty() {
        return Some(format!("{call}({value})"));
    }
    Some(format!("{call}({value}, {arguments})"))
}

/// Returns the call through a method table. See rule O-20.
pub fn dynamic_call(receiver: &str, method: &str, arguments: &str) -> String {
    if arguments.trim().is_empty() {
        return format!("{receiver}.vt->{method}({receiver}.obj)");
    }
    format!("{receiver}.vt->{method}({receiver}.obj, {arguments})")
}

/// Returns the fat pointer that a conversion builds. See rule O-22.
pub fn interface_value(module: &str, iface: &str, target: &str, value: &str) -> String {
    format!(
        "(({iface}){{ (void *){value}, &{} }})",
        vtable_name(module, iface, target)
    )
}

/// Returns a parameter type with the name removed.
fn erase_name(parameter: &str) -> String {
    let trimmed = parameter.trim();
    match trimmed.rsplit_once(char::is_whitespace) {
        Some((head, _)) => head.trim().to_owned(),
        None => trimmed
            .trim_end_matches(|item: char| item.is_alphanumeric() || item == '_')
            .trim()
            .to_owned(),
    }
}

/// Returns a parameter with a new name.
fn rename_parameter(parameter: &str, name: &str) -> String {
    format!("{} {name}", erase_name(parameter))
}
