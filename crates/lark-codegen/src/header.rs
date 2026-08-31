//! The module header.
//!
//! Rule X-4 puts every exported declaration in the header, and nothing else.
//! A module that imports another one includes its header.

use std::fmt::Write as _;

use lark_resolve::Module;
use lark_syntax::SyntaxKind::{
    BLOCK_STMT, COMMA, EQ, IFACE_DEF, IMPORT_DIRECTIVE, INIT_DECLARATOR, NAME, SEMICOLON,
    WHITESPACE,
};
use lark_syntax::{SyntaxNode, all_tokens};
use lark_types::Managed;
use lark_types::iface::Interfaces;

use crate::{iface_emit, managed_emit, names};

/// Builds the header text for a module.
pub fn header_text(
    module: &Module,
    managed: &Managed,
    interfaces: &Interfaces,
    instances: &str,
    uses_runtime: bool,
) -> String {
    let guard = guard_name(&module.name);
    let mut out = String::new();

    let _ = writeln!(
        out,
        "/* Generated from {}.lark. Do not edit. */",
        module.name
    );
    let _ = writeln!(out, "#ifndef {guard}");
    let _ = writeln!(out, "#define {guard}");
    let _ = writeln!(out);
    if uses_runtime {
        let _ = writeln!(out, "#include <lark_rt.h>");
        let _ = writeln!(out);
    }

    let root = module.parse.syntax();
    let mut wrote_include = false;
    for item in root
        .children()
        .filter(|item| item.kind() == IMPORT_DIRECTIVE)
    {
        let name = item
            .children()
            .find(|child| child.kind() == NAME)
            .and_then(|node| node.first_token())
            .map_or_else(String::new, |token| token.text().to_owned());
        let _ = writeln!(out, "#include \"{}\"", names::header_file(&name));
        wrote_include = true;
    }
    if wrote_include {
        let _ = writeln!(out);
    }

    // Rule X-8. Every record needs its name before any use of it. A generic
    // instantiation below can name a private record, so the header carries a
    // typedef for that one too. A forward typedef gives the name and no layout.
    let typedefs = managed_emit::forward_typedefs(managed, Some(instances));
    if !typedefs.is_empty() {
        out.push_str(&typedefs);
    }

    // An exported interface gives its method table type and its id, so another
    // module can hold a value of that interface. See rules T-12 and O-23.
    for interface in interfaces.interfaces.values() {
        if !interface.exported {
            continue;
        }
        out.push_str(&iface_emit::interface_declarations(&module.name, interface));
        out.push_str(&iface_emit::interface_id_declaration(
            &module.name,
            interface,
        ));
    }

    // Rule G-1. A generic has no C form, and its instantiations follow.
    if !instances.trim().is_empty() {
        out.push_str(instances);
    }

    for item in root.children() {
        if !names::is_exported(&item) {
            continue;
        }
        if item.kind() == IFACE_DEF {
            // The declarations above already carry it.
            continue;
        }
        if crate::generic_emit::declares_a_generic(&item) {
            continue;
        }
        let text = declaration_of(&item);
        if text.trim().is_empty() {
            continue;
        }
        let _ = writeln!(out, "{}", text.trim());
        // Rules X-8 and M-5. A record gets a typedef, and a managed one also
        // declares its field map.
        let support = managed_emit::record_support(&item, managed, &module.name, true);
        if !support.trim().is_empty() {
            let _ = writeln!(out, "{}", support.trim());
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "#endif");
    out
}

/// Returns the declaration form of an item, with no body.
///
/// A function definition becomes a prototype. A type definition stays whole.
fn declaration_of(item: &SyntaxNode) -> String {
    let mut out = String::new();
    let mut skip_space = false;

    // Rule X-4a. A variable gets an `extern` declaration and no initializer.
    let variable = names::defines_a_variable(item);
    if variable {
        out.push_str("extern ");
    }

    for token in all_tokens(item) {
        // The body belongs to the module, not to the header.
        if token.parent().is_some_and(|parent| is_inside_body(&parent)) {
            continue;
        }
        if variable && is_inside_initializer(&token) {
            // The space that separated the name from the `=` goes with it.
            if token.kind() == EQ {
                while out.ends_with(' ') {
                    out.pop();
                }
            }
            continue;
        }
        if skip_space && token.kind() == WHITESPACE && !token.text().contains('\n') {
            skip_space = false;
            continue;
        }
        skip_space = false;

        if names::is_dropped_marker(&token) {
            skip_space = true;
            continue;
        }
        if let Some(text) = names::module_path_text(&token) {
            out.push_str(&text);
            continue;
        }
        if names::is_dropped_path_part(&token) {
            continue;
        }
        out.push_str(token.text());
    }

    // A prototype and a record definition both end with a semicolon in C.
    let trimmed = out.trim_end().to_owned();
    if trimmed.ends_with(';') {
        trimmed
    } else {
        format!("{trimmed};")
    }
}

/// Reports whether a token belongs to an initializer.
///
/// The `=` and everything after it define the object, so a header drops both.
fn is_inside_initializer(token: &lark_syntax::SyntaxToken) -> bool {
    let Some(parent) = token.parent() else {
        return false;
    };
    let inside = parent.kind() == INIT_DECLARATOR
        || parent
            .ancestors()
            .any(|node| node.kind() == INIT_DECLARATOR);
    if !inside {
        return false;
    }
    if token.kind() == EQ {
        return true;
    }
    // Anything after the `=` sits in a node that is not the declarator.
    let mut previous = token.prev_token();
    while let Some(item) = previous {
        if item.kind() == EQ {
            return true;
        }
        if matches!(item.kind(), SEMICOLON | COMMA) {
            return false;
        }
        previous = item.prev_token();
    }
    false
}

/// Reports whether a node sits inside a function body.
fn is_inside_body(node: &SyntaxNode) -> bool {
    if node.kind() == BLOCK_STMT {
        return true;
    }
    node.ancestors()
        .any(|ancestor| ancestor.kind() == BLOCK_STMT)
}

/// Returns the include guard name for a module.
fn guard_name(module: &str) -> String {
    let mut out = String::from("LARK_");
    for character in module.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_uppercase());
        } else {
            out.push('_');
        }
    }
    out.push_str("_H");
    out
}
