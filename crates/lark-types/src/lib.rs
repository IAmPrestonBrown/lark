//! The Lark type system.
//!
//! The crate holds three parts.
//!
//! - [`ty`] represents a type and interns it in a store, so a type is one word
//!   to copy.
//! - [`lower`] builds a type from a declaration, and places the `gc` qualifier
//!   by rule T-1a.
//! - [`infer`] computes the type of an expression, and the type that `auto`
//!   infers from an initializer.
//! - [`check`](mod@check) enforces the type rules that phase 3 delivers.
//!
//! The checks report only what the front end can decide. Delivery phase A does
//! not read headers, so an unknown name is not an error here.

pub mod boundary;
pub mod caps;
pub mod check;
pub mod globals;
pub mod iface;
pub mod infer;
pub mod interior;
pub mod lower;
pub mod managed;
pub mod ty;

use lark_diag::Diagnostics;
use lark_resolve::Resolution;

pub use check::check;

/// Checks the rules that only a whole program can decide.
///
/// Rule I-1 needs one `init` function in a program that uses managed memory. A
/// single module that a tool checks on its own is not a program, so the build
/// runs this and the check does not.
#[must_use]
pub fn check_program(resolution: &Resolution) -> Diagnostics {
    let mut out = Diagnostics::new();
    let mut total = 0;
    let mut sources = Vec::new();
    let mut uses_runtime = false;
    for module in resolution.graph.modules() {
        let root = module.parse.syntax();
        let found = globals::collect(&root);
        total += found.init_spans.len();
        sources.push((module.source, found.init_spans.len()));
        uses_runtime |= globals::uses_managed_memory(&root);
    }
    globals::check_program(&sources, total, uses_runtime, &mut out);
    out
}
pub use infer::Infer;
pub use lower::{Lowering, Specifiers};
pub use managed::{Field, Managed, Record};
pub use ty::{Common, FloatWidth, IntWidth, NamedKind, TypeId, TypeKind, TypeStore};

/// Runs the type checks over every module of a resolution.
///
/// The diagnostics join the ones that the resolver produced.
#[must_use]
pub fn check_resolution(resolution: &Resolution) -> Diagnostics {
    check_resolution_with(resolution, caps::Capabilities::default())
}

/// Runs the type checks, with the capabilities of the selected collector.
///
/// Rule R-1. A collector that lacks a capability makes the transpiler reject
/// the source rules that depend on it. The language does not change with the
/// collector. The set of programs that it accepts does.
#[must_use]
pub fn check_resolution_with(
    resolution: &Resolution,
    capabilities: caps::Capabilities,
) -> Diagnostics {
    let mut store = TypeStore::new();
    let mut out = Diagnostics::new();
    let mut interfaces: std::collections::BTreeMap<String, iface::Interfaces> =
        std::collections::BTreeMap::new();
    for module in resolution.graph.modules() {
        let syntax_errors: Vec<lark_span::Span> = module
            .parse
            .errors()
            .iter()
            .map(|error| error.span)
            .collect();
        let root = module.parse.syntax();
        check(&mut store, module.source, &root, &syntax_errors, &mut out);
        boundary::check(module.source, &root, &syntax_errors, &mut out);
        // Rule R-1 and rule M-8.
        interior::check(module.source, &root, capabilities, &syntax_errors, &mut out);
        interfaces.insert(module.name.clone(), iface::collect(&root));
    }
    // Rule I-1 counts the `init` markers of the whole program.
    // The interface checks need every module's interfaces, so they run after
    // the first walk collected them.
    for module in resolution.graph.modules() {
        let syntax_errors: Vec<lark_span::Span> = module
            .parse
            .errors()
            .iter()
            .map(|error| error.span)
            .collect();
        if !syntax_errors.is_empty() {
            continue;
        }
        let root = module.parse.syntax();
        iface::check(module.source, &root, &interfaces, &mut out);
        let found = globals::collect(&root);
        globals::check(module.source, &root, &found, &mut out);
    }

    out.sort_by_position();
    out
}
