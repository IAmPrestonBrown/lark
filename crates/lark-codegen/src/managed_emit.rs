//! The machinery that a managed program needs in the emitted C.
//!
//! | Source | Emitted C |
//! |---|---|
//! | A managed struct | A `typedef` and a `lark_typeinfo`. Rules X-8 and M-5. |
//! | `new T { ... }` | One `lark_new` call through a temporary slot. Rule M-27. |
//! | `new T[n]` | One `lark_alloc_array` call through a temporary slot. |
//! | A function with a managed value | A shadow stack frame. Rules M-10 to M-12. |
//! | A loop | `LARK_POLL();` at the top of its body. Rule M-16. |

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

use std::fmt::Write as _;

use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, child_tokens};
use lark_types::{Managed, Record};

use crate::frame::{FRAME, Plan, RETURN_TEMP};
use crate::names;

/// Reports whether a module needs the runtime library.
pub fn module_uses_runtime(root: &SyntaxNode, managed: &Managed) -> bool {
    // Rule M-5a. A `managed` record puts a `lark_typeinfo` in the generated
    // header, so the header needs the runtime declarations even when nothing
    // in the module allocates. A plain C module declares no such record, and
    // constraint D-1 keeps it free of the runtime.
    if managed.records.values().any(|record| record.marked) {
        return true;
    }
    root.descendants()
        .any(|node| matches!(node.kind(), NEW_EXPR | NEW_ARRAY_EXPR))
        || root
            .descendants()
            .any(|node| node.kind() == FN_DEF && is_init_function(&node))
}

/// Reports whether a function carries the `init` marker. See rule I-1.
pub fn is_init_function(item: &SyntaxNode) -> bool {
    item.children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .any(|specifiers| {
            child_tokens(&specifiers).any(|token| token.kind() == IDENT && token.text() == "init")
        })
}

/// Returns the C name of the type descriptor for a record. See rule X-5a.
pub fn typeinfo_name(module: &str, record: &str) -> String {
    format!("lk_{module}__{record}__ti")
}

/// Returns a forward `typedef` for every record in a module. See rule X-8.
///
/// The typedef comes before every definition, so a field can name its own
/// record and two records can name each other.
pub fn forward_typedefs(managed: &Managed, needed: Option<&str>) -> String {
    let mut out = String::new();
    for record in managed.records.values() {
        if record.generic {
            continue;
        }
        // A header carries a name only when it exports the record, or when an
        // instantiation in the same header refers to it. A forward typedef
        // gives the name and no layout.
        if let Some(text) = needed
            && !record.exported
            && !text.contains(&record.name)
        {
            continue;
        }
        // Rule X-8. The typedef repeats the keyword that the source used, so
        // the tag namespace entry matches.
        let name = &record.name;
        let keyword = record.keyword.text();
        let _ = writeln!(out, "typedef {keyword} {name} {name};");
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Returns the type descriptor that a record needs.
///
/// `header` selects the header form, which declares the descriptor rather than
/// defining it. See rules M-5 and X-4a.
pub fn record_support(item: &SyntaxNode, managed: &Managed, module: &str, header: bool) -> String {
    if !header {
        // The prologue already declares every descriptor of this module.
        return String::new();
    }
    let mut out = String::new();
    for record in records_in(item) {
        let Some(info) = managed.records.get(&record) else {
            continue;
        };
        // A generic record has one layout per instantiation. Phase 8 emits the
        // descriptor for each one.
        // Rule M-5a. Every non generic record needs a descriptor, so a `new`
        // expression can read the true payload size from it. A program that
        // does not use the runtime cannot name `lark_typeinfo`, and the caller
        // skips this whole function for one.
        if info.generic {
            continue;
        }
        // A header declares only what it exports. The module body declares
        // every descriptor, because a `new` expression can name any of them
        // before the epilogue defines it.
        if header && !info.exported {
            continue;
        }
        let _ = write!(
            out,
            "\nextern const lark_typeinfo {};",
            typeinfo_name(module, &record)
        );
    }
    out
}

/// Returns the field map and the descriptor for one record. See rule M-5.
pub fn typeinfo_definition(record: &Record, module: &str, has_itable: bool) -> String {
    let name = &record.name;
    let symbol = typeinfo_name(module, name);
    let managed_fields: Vec<&str> = record
        .managed_fields()
        .map(|field| field.name.as_str())
        .collect();

    let mut out = String::new();
    let (itable_count, itable) = if has_itable {
        (
            "1u".to_owned(),
            crate::iface_emit::itable_name(module, name),
        )
    } else {
        ("0u".to_owned(), "0".to_owned())
    };
    let itable_count = if has_itable {
        format!("(uint32_t)(sizeof({itable}) / sizeof({itable}[0]))")
    } else {
        itable_count
    };
    let itable_ref = if has_itable { itable } else { "0".to_owned() };

    if managed_fields.is_empty() {
        let _ = writeln!(
            out,
            "const lark_typeinfo {symbol} = {{\n    \"{name}\", sizeof({name}), \
             (uint32_t)_Alignof({name}), 0u, 0, {itable_count}, {itable_ref}\n}};"
        );
        return out;
    }

    let offsets: Vec<String> = managed_fields
        .iter()
        .map(|field| format!("(uint32_t)offsetof({name}, {field})"))
        .collect();
    let _ = writeln!(
        out,
        "static const uint32_t {symbol}_ptrs[] = {{ {} }};",
        offsets.join(", ")
    );
    let _ = writeln!(
        out,
        "const lark_typeinfo {symbol} = {{\n    \"{name}\", sizeof({name}), \
         (uint32_t)_Alignof({name}), {}u, {symbol}_ptrs, {itable_count}, {itable_ref}\n}};",
        managed_fields.len()
    );
    out
}

/// Returns every record tag that an item defines.
fn records_in(item: &SyntaxNode) -> Vec<String> {
    let mut found = Vec::new();
    for specifiers in item
        .children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
    {
        for record in specifiers
            .children()
            .filter(|child| matches!(child.kind(), STRUCT_DEF | UNION_DEF))
        {
            if let Some(name) = record
                .children()
                .find(|child| child.kind() == NAME)
                .and_then(|node| node.first_token())
            {
                found.push(name.text().to_owned());
            }
        }
    }
    found
}

/// Returns the runtime startup that rule I-3 puts in the `init` function.
pub fn startup(roots: &str, torture: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\n    /* lark: runtime startup, rule I-3 */");
    let _ = writeln!(
        out,
        "    lark_gc_config _lk_config = lark_gc_config_default();"
    );
    let _ = writeln!(out, "    _lk_config.roots = {roots};");
    let _ = writeln!(
        out,
        "    _lk_config.torture = {};",
        if torture { "true" } else { "false" }
    );
    let _ = writeln!(out, "    lark_startup_with(_lk_config);");
    out
}

/// Returns the frame declaration for a function. See rules M-10 and M-11.
pub fn frame_declaration(plan: &Plan) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "\n    /* lark: shadow stack frame, {} managed locals, {} temporaries */",
        plan.locals, plan.temps
    );
    let _ = writeln!(
        out,
        "    struct {{ lark_frame_hdr h; void **s[{}]; void *t[{}]; }} {FRAME};",
        plan.slot_len(),
        plan.temp_len()
    );
    let _ = writeln!(out, "    {FRAME}.h.slots = {FRAME}.s;");
    let _ = writeln!(out, "    {FRAME}.h.temps = {FRAME}.t;");
    let _ = writeln!(out, "    {FRAME}.h.nslots = 0u;");
    let _ = writeln!(out, "    {FRAME}.h.ntemps = {}u;", plan.temps);
    for index in 0..plan.temp_len() {
        let _ = writeln!(out, "    {FRAME}.t[{index}] = 0;");
    }
    let _ = writeln!(out, "    lark_frame_push(&{FRAME}.h);");
    // Rule M-10 and rule M-11. A parameter holds its value before the push, so
    // every managed one joins the frame at once.
    for (index, (name, is_interface)) in plan.params.iter().enumerate() {
        let _ = writeln!(
            out,
            "   {}",
            register_local(name, index, *is_interface).trim_end()
        );
    }
    out
}

/// Returns the registration of one managed local. See rule M-11.
///
/// An interface value holds its object in a field, so the slot points at that
/// field rather than at the value. See rule O-24.
pub fn register_local(name: &str, index: usize, is_interface: bool) -> String {
    let target = if is_interface {
        format!("{name}.obj")
    } else {
        name.to_owned()
    };
    format!(
        " {FRAME}.s[{index}] = (void **)&{target}; {FRAME}.h.nslots = {}u;",
        index + 1
    )
}

/// Returns a `return` that pops the frame after the expression. See rule M-12.
pub fn return_statement(plan: &Plan, value: &str) -> String {
    let value = value.trim_end_matches(';').trim();
    if plan.returns_void || value.is_empty() {
        return format!("{{ lark_frame_pop(&{FRAME}.h); return; }}");
    }
    format!(
        "{{ {} {RETURN_TEMP} = ({value}); lark_frame_pop(&{FRAME}.h); return {RETURN_TEMP}; }}",
        plan.return_type
    )
}

/// Returns the call that one `new` expression becomes. See rules O-4 and M-27.
pub fn new_expression(
    node: &SyntaxNode,
    type_name: &str,
    descriptor: &str,
    has_header: bool,
    payload: &str,
    temp: Option<usize>,
) -> String {
    let call = if node.kind() == NEW_ARRAY_EXPR {
        let count = payload;
        if has_header {
            format!("lark_alloc_array({descriptor}, {count})")
        } else {
            // A plain element has no field map, so the count is in bytes.
            format!("lark_alloc_array({descriptor}, ({count}) * sizeof({type_name}))")
        }
    } else {
        format!("lark_new({descriptor}, &({type_name}){payload})")
    };

    match temp {
        // Rule M-27. The result goes to a temporary slot first, so a later
        // allocation in the same expression cannot free it.
        Some(index) if node.kind() == NEW_EXPR => {
            // Rule M-28. The allocation runs before the initializer, because
            // the allocation is a safepoint. An initializer that names a
            // managed local reads it after the collection, so a collector that
            // moves the object gives the new address rather than the old one.
            let slot = format!("{FRAME}.t[{index}]");
            let store = format!("*({type_name} *){slot} = ({type_name}){payload}");
            format!("({slot} = lark_new({descriptor}, 0), {store}, ({type_name} *){slot})")
        }
        Some(index) => {
            format!("({FRAME}.t[{index}] = {call}, ({type_name} *){FRAME}.t[{index}])")
        }
        None => format!("({type_name} *){call}"),
    }
}

/// Returns the element type that a `new` expression names.
pub fn element_type(node: &SyntaxNode) -> Option<String> {
    let type_name = node.children().find(|child| child.kind() == TYPE_NAME)?;
    let specifiers = type_name
        .children()
        .find(|child| child.kind() == DECL_SPECIFIERS)?;

    let mut words = Vec::new();
    for token in child_tokens(&specifiers) {
        if token.kind().is_trivia() {
            continue;
        }
        if token.kind() == IDENT && matches!(token.text(), "gc" | "managed") {
            continue;
        }
        words.push(token.text().to_owned());
    }
    for child in specifiers.children() {
        match child.kind() {
            NAME_REF => {
                if let Some(token) = child.first_token() {
                    words.push(token.text().to_owned());
                }
            }
            // Rule N-4 and rule X-5. `mod::Name` names a record of another
            // module, and the emitted C keeps only the part after the `::`.
            PATH => {
                let idents: Vec<String> = child_tokens(&child)
                    .filter(|token| token.kind() == IDENT)
                    .map(|token| token.text().to_owned())
                    .collect();
                if let [module, name] = idents.as_slice() {
                    words.push(format!("{module}::{name}"));
                } else if let Some(last) = idents.last() {
                    words.push(last.clone());
                }
            }
            _ => {}
        }
    }
    if words.is_empty() {
        None
    } else {
        Some(words.join(" "))
    }
}

/// The unused import keeps the name rules visible to this module.
const _: fn(&SyntaxNode) -> bool = names::is_exported;
