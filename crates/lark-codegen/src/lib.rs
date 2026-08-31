//! The Lark C emitter.
//!
//! The emitter walks the lossless tree and writes source text. It keeps every
//! token that C already understands, and transforms only what Lark adds. The
//! output therefore holds the programmer's own formatting and comments, which
//! rule X-2 requires.
//!
//! | Source | Emitted C |
//! |---|---|
//! | `@import m` | `#include "m.h"` |
//! | `mod::f(a)` | `f(a)`. Rule X-5 keeps the name. |
//! | `export` | Removed. The item joins the module header. |
//! | `gc`, `managed`, `init`, `gc_leaf`, `gc_safe` | Removed. |
//! | Anything else | Kept, byte for byte. |
//!
//! A later phase adds the machinery that a managed program needs: the object
//! header, the shadow stack, the method tables, and the monomorphic generics.

// This module walks 64 of the kinds, and a list that long in the header helps
// no reader, so it imports the variants. A module that uses a few names spells
// them out instead.
#![allow(clippy::enum_glob_use)]

mod foreign;
mod frame;
mod generic_emit;
mod global_emit;
mod header;
mod iface_emit;
mod managed_emit;
pub mod names;
pub mod reach;

use std::fmt::Write as _;

use lark_mono::{Kind, Program};
use lark_resolve::{Module, ModuleGraph, ModuleId};
use lark_syntax::SyntaxKind::*;
use lark_syntax::{NodeOrToken, SyntaxNode, SyntaxToken, child_tokens};
use lark_types::globals::Globals;
use lark_types::iface::Interfaces;
use lark_types::{Managed, Record};

use frame::{FRAME, Plan};

pub use header::header_text;
pub use names::{is_generated_prefix, module_header_name};

/// How the emitter writes its output.
#[derive(Clone, Debug)]
pub struct Options {
    /// Whether the output carries `#line` directives. See rule X-3.
    pub line_directives: bool,
    /// The name that a `#line` directive prints.
    ///
    /// A real build leaves this empty, so the directive names the file that a
    /// debugger opens. A test sets it, so a snapshot holds no machine path.
    pub source_name: Option<String>,
    /// The stack root mechanism, from `gc.roots` in `lark.toml`.
    pub roots: Roots,
    /// Whether every safepoint runs a full collection. See rule F-3.
    pub torture: bool,
    /// Whether the collector needs a call for a store of a managed pointer.
    ///
    /// Rule R-2. A collector that walks part of the heap cannot find a
    /// pointer from the part it skips, so the store is recorded. Every other
    /// collector gets a plain store and pays nothing.
    pub write_barrier: bool,
}

/// The stack root mechanism. See rules M-10 and M-13.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Roots {
    /// Precise roots from a shadow stack.
    #[default]
    ShadowStack,
    /// Roots from a scan of the machine stack.
    Conservative,
}

impl Roots {
    /// Returns the runtime constant for the mode.
    #[must_use]
    pub const fn constant(self) -> &'static str {
        match self {
            Self::ShadowStack => "LARK_ROOTS_SHADOW_STACK",
            Self::Conservative => "LARK_ROOTS_CONSERVATIVE",
        }
    }

    /// Reads the mode from the value that `lark.toml` holds.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        if value == "conservative" {
            Self::Conservative
        } else {
            Self::ShadowStack
        }
    }
}

impl Default for Options {
    fn default() -> Self {
        Self {
            line_directives: true,
            source_name: None,
            roots: Roots::ShadowStack,
            torture: false,
            write_barrier: false,
        }
    }
}

/// The place that the foreign call helpers take in the output.
///
/// The walk learns which result types it needs only after it reaches the calls,
/// so the prologue leaves a marker and the run fills it.
const FOREIGN_ANCHOR: &str = "\u{0}lark-foreign-helpers\u{0}";

/// One line of the emitted C, and the source line it came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineEntry {
    /// The one based line in the emitted C.
    pub emitted: u32,
    /// The one based line in the `.lark` source.
    pub source: u32,
}

/// What the emitter produced for one module.
#[derive(Clone, Debug)]
pub struct Emitted {
    /// The module body.
    pub c: String,
    /// The declarations that other modules can use. See rule X-4.
    pub header: String,
    /// The map from the emitted C back to the source. See rule X-3.
    pub line_map: Vec<LineEntry>,
    /// Whether the module needs the runtime library.
    pub uses_runtime: bool,
}

impl Emitted {
    /// Returns the line map in the form that a snapshot test compares.
    #[must_use]
    pub fn line_map_text(&self, source_name: &str) -> String {
        let mut out = String::new();
        for entry in &self.line_map {
            let _ = writeln!(out, "{source_name}:{} -> c:{}", entry.source, entry.emitted);
        }
        out
    }
}

/// Emits the C for one module.
#[must_use]
pub fn emit(
    graph: &ModuleGraph,
    id: ModuleId,
    options: &Options,
    program: &Program,
) -> Option<Emitted> {
    let module = graph.get(id)?;
    let root = module.parse.syntax();
    let managed = lark_types::managed::collect(&root);
    let interfaces = lark_types::iface::collect(&root);
    let globals = lark_types::globals::collect(&root);
    let foreign = foreign::collect(graph);
    let uses_runtime = managed_emit::module_uses_runtime(&root, &managed)
        || !interfaces.implementations.is_empty()
        || !globals.blocks.is_empty()
        || program.instances_of(&module.name).iter().any(|instance| {
            program
                .generic(&instance.name)
                .is_some_and(|generic| generic_emit::needs_header(generic, instance))
        });

    // Rule M-18. The analysis needs the whole program, so it runs once here.
    let reach = reach::analyze(graph, &foreign);

    let imported: std::collections::BTreeMap<String, Managed> = graph
        .modules()
        .iter()
        .filter(|item| item.name != module.name)
        .map(|item| {
            (
                item.name.clone(),
                lark_types::managed::collect(&item.parse.syntax()),
            )
        })
        .collect();

    let mut emitter = Emitter {
        reach,
        hoisted: std::collections::HashMap::new(),
        module,
        managed,
        interfaces,
        globals,
        foreign,
        leave_helpers: std::collections::BTreeSet::new(),
        program,
        substitutions: std::collections::BTreeMap::new(),
        uses_runtime,
        out: String::new(),
        line_map: Vec::new(),
        options: options.clone(),
        pending_skip_space: false,
        frame: None,
        next_local: 0,
        next_temp: 0,
        implementing: None,
        current_function: None,
        record_instances: String::new(),
        exported: exported_names(&root),
        locals: std::collections::BTreeMap::new(),
        imported,
    };
    emitter.run();

    Some(Emitted {
        header: header_text(
            module,
            &emitter.managed,
            &emitter.interfaces,
            &emitter.record_instances,
            uses_runtime,
        ),
        c: emitter.out,
        line_map: emitter.line_map,
        uses_runtime,
    })
}

/// The state that one emit carries.
struct Emitter<'a> {
    module: &'a Module,
    managed: Managed,
    interfaces: Interfaces,
    globals: Globals,
    foreign: foreign::Foreign,
    /// The result types that need a helper to leave the safe state.
    leave_helpers: std::collections::BTreeSet<String>,
    program: &'a Program,
    /// The type arguments of the instantiation that the emitter is inside.
    substitutions: std::collections::BTreeMap<String, String>,
    uses_runtime: bool,
    out: String,
    line_map: Vec<LineEntry>,
    options: Options,
    /// True when the emitter dropped a marker and must drop the space after it.
    pending_skip_space: bool,
    /// The plan for the function that the emitter is inside.
    frame: Option<Plan>,
    next_local: usize,
    next_temp: usize,
    /// The interface and target of the implementation that the emitter is in.
    implementing: Option<(String, String)>,
    /// The name of the function that the emitter is inside.
    current_function: Option<String>,
    /// The record instantiations, which the header carries. See rule G-1.
    record_instances: String,
    /// Every name that the module exports. See rule N-6.
    exported: std::collections::BTreeSet<String>,
    /// Which functions can reach an allocation. See rule M-18.
    reach: reach::Reach,
    /// Allocations that a surrounding allocation already emitted.
    ///
    /// Rule M-28a hoists a nested `new` out of an initializer, so the map says
    /// which temporary slot already holds it. A node in the map renders as a
    /// read of that slot rather than as a second allocation.
    hoisted: std::collections::HashMap<(u32, u32), (usize, String)>,
    /// The type of every local in scope, and whether it is a pointer.
    locals: std::collections::BTreeMap<String, (String, bool)>,
    /// The managed records of every other module, by module name.
    ///
    /// Rule M-5a needs the descriptor of the record that a `new` names, and
    /// rule N-4 lets that record live in another module.
    imported: std::collections::BTreeMap<String, Managed>,
}

impl Emitter<'_> {
    /// Writes the whole module.
    ///
    /// The walk keeps every root level token, so a comment and a `#include`
    /// both reach the output in the place the programmer wrote them.
    fn run(&mut self) {
        let root = self.module.parse.syntax();

        self.write_prologue();
        for element in root.children_with_tokens() {
            match element {
                NodeOrToken::Node(item) => {
                    self.write_line_directive(&item);
                    self.write_item(&item);
                }
                NodeOrToken::Token(token) => {
                    // Rule C-3a. An `#include` already stands in the prologue,
                    // so writing it twice would include the header twice.
                    if is_include_directive(&token) {
                        continue;
                    }
                    self.out.push_str(token.text());
                }
            }
        }
        self.write_epilogue();
        self.write_leave_helpers();
    }

    /// Writes the tables that every earlier item needs.
    ///
    /// A method table names a thunk, an interface table names a method table,
    /// and a type descriptor names an interface table. Each one therefore comes
    /// after the definitions it holds.
    fn write_epilogue(&mut self) {
        let module = self.module.name.clone();
        let mut out = String::new();

        for interface in self.interfaces.interfaces.values() {
            out.push_str(&iface_emit::interface_id_definition(&module, interface));
        }

        for item in &self.interfaces.implementations {
            let Some(interface) = self.interfaces.interfaces.get(&item.iface) else {
                continue;
            };
            out.push('\n');
            out.push_str(&iface_emit::implementation_tables(&module, item, interface));
        }

        let mut targets: Vec<String> = self
            .interfaces
            .implementations
            .iter()
            .map(|item| item.target.clone())
            .collect();
        targets.sort();
        targets.dedup();
        for target in &targets {
            let items = self.interfaces.interfaces_of(target);
            out.push('\n');
            out.push_str(&iface_emit::itable_definition(&module, target, &items));
        }

        // Rule M-5a gives every record a descriptor, but only a program that
        // uses the runtime can name `lark_typeinfo`. A plain C file declares
        // structs and never allocates one, so it gets no table at all.
        for record in self.managed.described_records() {
            if record.generic || !self.uses_runtime {
                continue;
            }
            let has_itable = targets.iter().any(|target| target == &record.name);
            out.push('\n');
            out.push_str(&managed_emit::typeinfo_definition(
                record, &module, has_itable,
            ));
        }

        if !out.trim().is_empty() {
            self.out.push_str("\n/* lark: tables */\n");
            self.out.push_str(&out);
        }
        self.write_instance_bodies();
    }

    /// Writes one helper per result type that a safe call needs.
    fn write_leave_helpers(&mut self) {
        let helpers: Vec<String> = self.leave_helpers.iter().cloned().collect();
        let mut text = String::new();
        if !helpers.is_empty() {
            text.push_str("/* lark: foreign call helpers, rule M-19 */\n");
            for result in helpers {
                text.push_str(&foreign::leave_helper_definition(&result));
            }
            text.push('\n');
        }
        // The marker always goes, so a module with no safe call keeps none.
        if let Some(position) = self.out.find(FOREIGN_ANCHOR) {
            self.out
                .replace_range(position..position + FOREIGN_ANCHOR.len(), &text);
        }
    }

    /// Writes the body of every function instantiation, and every field map.
    fn write_instance_bodies(&mut self) {
        let module = self.module.name.clone();
        let instances: Vec<lark_mono::Instance> = self.program.instances_of(&module).to_vec();
        if instances.is_empty() {
            return;
        }
        let _ = writeln!(self.out, "\n/* lark: generic bodies */");

        for instance in instances.iter().filter(|item| item.kind == Kind::Function) {
            let Some(generic) = self.program.generic(&instance.name).cloned() else {
                continue;
            };
            let previous = std::mem::replace(
                &mut self.substitutions,
                generic_emit::substitutions(&generic, instance),
            );
            let head = self.render_function_head(&generic, &instance.mangled);
            let body = generic
                .node
                .children()
                .find(|child| child.kind() == BLOCK_STMT);
            let body = body.map_or_else(|| "{ }".to_owned(), |node| self.render(&node));
            self.substitutions = previous;
            let _ = writeln!(self.out, "static {head} {body}");
        }

        // Rule G-13. A managed instantiation gets its own field map, because
        // the offsets differ per instantiation.
        for instance in instances.iter().filter(|item| item.kind == Kind::Record) {
            let Some(generic) = self.program.generic(&instance.name).cloned() else {
                continue;
            };
            // Rule M-5a. Every instantiation gets a descriptor, even one with
            // no managed field, because `lark_new` copies `size` bytes.
            let fields = if generic_emit::needs_header(&generic, instance) {
                generic_emit::managed_fields(&generic, instance)
            } else {
                Vec::new()
            };
            let name = instance.mangled.clone();
            if fields.is_empty() {
                let _ = writeln!(
                    self.out,
                    "const lark_typeinfo {name}__ti = {{\n    \"{name}\", sizeof({name}), \
                     (uint32_t)_Alignof({name}), 0u, 0, 0u, 0\n}};"
                );
                continue;
            }
            let offsets: Vec<String> = fields
                .iter()
                .map(|field| format!("(uint32_t)offsetof({name}, {field})"))
                .collect();
            let _ = writeln!(
                self.out,
                "static const uint32_t {name}__ti_ptrs[] = {{ {} }};",
                offsets.join(", ")
            );
            let _ = writeln!(
                self.out,
                "const lark_typeinfo {name}__ti = {{\n    \"{name}\", sizeof({name}), \
                 (uint32_t)_Alignof({name}), {}u, {name}__ti_ptrs, 0u, 0\n}};",
                fields.len()
            );
        }
    }

    /// Writes the header comment and the module's own include.
    fn write_prologue(&mut self) {
        let name = self.module.name.clone();
        let _ = writeln!(self.out, "/* Generated from {name}.lark. Do not edit. */");
        let _ = writeln!(self.out, "#include \"{}\"", names::header_file(&name));
        let _ = writeln!(self.out);
        // Rule X-8. Every record needs its name before any use of it.
        let typedefs = managed_emit::forward_typedefs(&self.managed, None);
        if !typedefs.is_empty() {
            self.out.push_str(&typedefs);
        }

        // Every table lives in the epilogue, because it names a definition that
        // comes later in the file. A declaration here lets any earlier item use
        // it. An exported interface carries its own declarations in the header.
        let module = self.module.name.clone();
        for interface in self.interfaces.interfaces.values() {
            if interface.exported {
                continue;
            }
            let text = iface_emit::interface_declarations(&module, interface);
            self.out.push_str(&text);
        }
        for interface in self.interfaces.interfaces.values() {
            if interface.exported {
                continue;
            }
            let _ = writeln!(
                self.out,
                "extern const char {};",
                iface_emit::iface_id(&module, &interface.name)
            );
        }
        for item in &self.interfaces.implementations {
            let _ = writeln!(
                self.out,
                "extern const {} {};",
                iface_emit::vtable_type(&module, &item.iface),
                iface_emit::vtable_name(&module, &item.iface, &item.target)
            );
        }
        // Rule M-5a. Only a program that uses the runtime can name
        // `lark_typeinfo`, so a plain C file gets no descriptor.
        for record in self.managed.described_records() {
            if record.generic || !self.uses_runtime {
                continue;
            }
            let _ = writeln!(
                self.out,
                "extern const lark_typeinfo {};",
                managed_emit::typeinfo_name(&module, &record.name)
            );
        }
        self.out.push('\n');

        self.write_includes();
        self.write_local_types();
        self.write_instances();
        self.write_forward_declarations();
        // The helpers land here once the walk knows which result types a safe
        // call needs.
        self.out.push_str(FOREIGN_ANCHOR);
    }

    /// Writes one forward declaration per function definition.
    ///
    /// C needs a declaration before a call, and a Lark program can call a
    /// function that the file defines later. See rule X-5b.
    /// Writes every `#include` of the module, before anything that needs it.
    ///
    /// Rule C-3a. A forward declaration can name `size_t` or `FILE`, so the
    /// header must come first. The directive keeps its text, and the body no
    /// longer writes it where the programmer put it.
    fn write_includes(&mut self) {
        let root = self.module.parse.syntax();
        let mut wrote = false;
        for token in root
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
        {
            if !is_include_directive(&token) {
                continue;
            }
            self.out.push_str(token.text().trim());
            self.out.push('\n');
            wrote = true;
        }
        if wrote {
            self.out.push('\n');
        }
    }

    /// Writes every local `typedef` of the module, before anything that uses it.
    ///
    /// Rule X-6a. A forward declaration can name a type that the module itself
    /// declares, and rule L-8 makes a module order independent, so the type
    /// must come first. An exported type already stands in the header.
    fn write_local_types(&mut self) {
        let root = self.module.parse.syntax();
        let mut wrote = false;
        for item in root.children() {
            if !is_local_typedef(&item) || names::is_exported(&item) {
                continue;
            }
            if generic_emit::declares_a_generic(&item) {
                continue;
            }
            if !wrote {
                let _ = writeln!(self.out, "/* lark: local types */");
                wrote = true;
            }
            self.out.push_str(item.text().to_string().trim());
            self.out.push('\n');
        }
        if wrote {
            self.out.push('\n');
        }
    }

    fn write_forward_declarations(&mut self) {
        let root = self.module.parse.syntax();
        let mut wrote = false;

        for item in root.children() {
            // Rule G-1. A generic has no C form, and the instantiation pass
            // already declared every instance.
            if generic_emit::declares_a_generic(&item) {
                continue;
            }
            match item.kind() {
                FN_DEF => {
                    // C11 6.9.1. An old style definition names its parameters
                    // without types, so a prototype built from it is not valid
                    // C. The definition itself declares the function.
                    if item.children().any(|child| child.kind() == KR_PARAM_LIST) {
                        continue;
                    }
                    let text = self.function_declaration(&item);
                    if text.trim().is_empty() {
                        continue;
                    }
                    if !wrote {
                        let _ = writeln!(self.out, "/* lark: forward declarations */");
                        wrote = true;
                    }
                    let _ = writeln!(self.out, "{text};");
                }
                IMPL_DEF => {
                    let names: Vec<String> = item
                        .children()
                        .filter(|child| child.kind() == NAME_REF)
                        .filter_map(|child| child.first_token())
                        .map(|token| token.text().to_owned())
                        .collect();
                    let [iface, target] = names.as_slice() else {
                        continue;
                    };
                    let previous = self.implementing.take();
                    self.implementing = Some((iface.clone(), target.clone()));
                    for method in item.children().filter(|child| child.kind() == FN_DEF) {
                        let text = self.function_declaration(&method);
                        if text.trim().is_empty() {
                            continue;
                        }
                        if !wrote {
                            let _ = writeln!(self.out, "/* lark: forward declarations */");
                            wrote = true;
                        }
                        let _ = writeln!(self.out, "static {text};");
                    }
                    self.implementing = previous;
                }
                _ => {}
            }
        }
        if wrote {
            self.out.push('\n');
        }
    }

    /// Returns the declaration form of a function definition, with no body.
    fn function_declaration(&mut self, item: &SyntaxNode) -> String {
        let mut out = String::new();
        if self.implementing.is_none() && self.needs_static(item) {
            out.push_str("static ");
        }
        for child in item.children() {
            match child.kind() {
                BLOCK_STMT => break,
                DECL_SPECIFIERS | DECLARATOR => {
                    if !out.is_empty() && !out.ends_with(' ') {
                        out.push(' ');
                    }
                    out.push_str(&self.render(&child));
                }
                _ => {}
            }
        }
        out
    }

    /// Reports whether an item needs the `static` marker. See rule X-5b.
    fn needs_static(&self, item: &SyntaxNode) -> bool {
        if !needs_static(item, &self.exported) {
            return false;
        }
        // Rule X-5d. A header that the module includes already declares the
        // symbol to the rest of the program, so the symbol is public and the
        // `static` marker would contradict the prototype. A plain C file gets
        // the linkage that C gives it.
        !names::declared_name(item).is_some_and(|name| self.module.headers.is_value(&name))
    }

    /// Returns the names of every function that this module defines.
    fn defined_functions(&self) -> std::collections::BTreeSet<String> {
        self.module
            .parse
            .syntax()
            .children()
            .filter(|item| item.kind() == FN_DEF)
            .filter_map(|item| names::declared_name(&item))
            .collect()
    }

    /// Writes one concrete definition per instantiation. See rule G-1.
    fn write_instances(&mut self) {
        let module = self.module.name.clone();
        let instances: Vec<lark_mono::Instance> = self.program.instances_of(&module).to_vec();
        if instances.is_empty() {
            return;
        }

        let _ = writeln!(self.out, "/* lark: generic instantiations */");

        // A record instantiation is a type, so it lives in the header. Another
        // module that imports this one then sees it.
        let mut records = String::new();
        for instance in instances.iter().filter(|item| item.kind == Kind::Record) {
            let name = instance.mangled.clone();
            let _ = writeln!(records, "typedef struct {name} {name};");
        }
        for instance in instances.iter().filter(|item| item.kind == Kind::Record) {
            let Some(generic) = self.program.generic(&instance.name).cloned() else {
                continue;
            };
            let Some(body) = generic_emit::record_body(&generic) else {
                continue;
            };
            let previous = std::mem::replace(
                &mut self.substitutions,
                generic_emit::substitutions(&generic, instance),
            );
            let text = self.render(&body);
            self.substitutions = previous;
            let _ = writeln!(records, "struct {} {text};", instance.mangled);

            // Rule M-5a. The descriptor exists for every instantiation, so
            // the declaration does too.
            let _ = writeln!(
                records,
                "extern const lark_typeinfo {}__ti;",
                instance.mangled
            );
        }
        self.record_instances = records;

        // A function instantiation gets a prototype here and a body in the
        // epilogue, so it can call anything the module declares.
        for instance in instances.iter().filter(|item| item.kind == Kind::Function) {
            let Some(generic) = self.program.generic(&instance.name).cloned() else {
                continue;
            };
            let previous = std::mem::replace(
                &mut self.substitutions,
                generic_emit::substitutions(&generic, instance),
            );
            let text = self.render_function_head(&generic, &instance.mangled);
            self.substitutions = previous;
            let _ = writeln!(self.out, "static {text};");
        }
        self.out.push('\n');
    }

    /// Returns the head of a function, with a new name and no body.
    fn render_function_head(&mut self, generic: &lark_mono::Generic, name: &str) -> String {
        let mut out = String::new();
        for child in generic.node.children() {
            match child.kind() {
                BLOCK_STMT => break,
                DECL_SPECIFIERS => {
                    out.push_str(&self.render(&child));
                    out.push(' ');
                }
                DECLARATOR => {
                    out.push_str(&self.render_declarator(&child, name));
                }
                _ => {}
            }
        }
        out
    }

    /// Returns a declarator with its name replaced and its parameters dropped.
    fn render_declarator(&mut self, declarator: &SyntaxNode, name: &str) -> String {
        let mut out = String::new();
        for child in declarator.children_with_tokens() {
            match child {
                NodeOrToken::Node(node) if node.kind() == NAME => out.push_str(name),
                NodeOrToken::Node(node) if node.kind() == GENERIC_PARAMS => {}
                NodeOrToken::Node(node) => out.push_str(&self.render(&node)),
                NodeOrToken::Token(token) => {
                    if !token.kind().is_trivia() {
                        out.push_str(token.text());
                    }
                }
            }
        }
        out
    }

    /// Writes one top level item.
    fn write_item(&mut self, item: &SyntaxNode) {
        match item.kind() {
            IMPORT_DIRECTIVE => {
                self.write_include(item);
                return;
            }
            IFACE_DEF => {
                self.write_interface(item);
                return;
            }
            IMPL_DEF => {
                self.write_implementation(item);
                return;
            }
            GLOBAL_BLOCK => {
                self.write_global_block(item);
                return;
            }
            _ => {}
        }

        // Rule G-1. A generic emits one definition per instantiation, and the
        // prologue already carries them.
        if generic_emit::declares_a_generic(item)
            && let Some(name) = names::declared_name(item)
            && let Some(generic) = self.program.generic(&name)
        {
            self.out.push_str(&generic_emit::placeholder(generic));
            return;
        }

        // Rule X-4a. An exported type lives in the header only. Its field map
        // stays in the module, because the header holds no definition.
        if names::is_exported(item) && names::defines_a_type_only(item) {
            let name = names::declared_name(item).unwrap_or_else(|| "the type".to_owned());
            let _ = write!(
                self.out,
                "/* lark: {name} is in {} */",
                names::header_file(&self.module.name)
            );
            let text = managed_emit::record_support(item, &self.managed, &self.module.name, false);
            self.out.push_str(&text);
            return;
        }

        if item.kind() == FN_DEF {
            self.write_function(item);
            return;
        }

        // A prototype for a name that this module defines is a forward
        // declaration, and the prologue already carries one. See rule X-5b.
        if item.kind() == DECLARATION
            && !item.children().any(|child| child.kind() == BLOCK_STMT)
            && let Some(name) = names::declared_name(item)
            && self.defined_functions().contains(&name)
        {
            let _ = write!(self.out, "/* lark: `{name}` is declared above */");
            return;
        }

        // Rule X-6a. A local typedef already stands in the prologue.
        if is_local_typedef(item) && !names::is_exported(item) {
            let name = names::declared_name(item).unwrap_or_default();
            let _ = write!(self.out, "/* lark: `{name}` is declared above */");
            return;
        }

        if self.needs_static(item) {
            self.out.push_str("static ");
        }
        self.write_node(item);

        // Rule O-25 drops the semicolon after a `}` body. C needs it.
        if item.kind() == DECLARATION && item.text().to_string().trim_end().ends_with('}') {
            self.out.push(';');
        }

        // Rule M-5. A managed record gets its field map.
        if item.kind() == DECLARATION {
            let text = managed_emit::record_support(item, &self.managed, &self.module.name, false);
            self.out.push_str(&text);
        }
    }

    /// Returns the name of every interface in scope.
    fn interface_names(&self) -> std::collections::BTreeSet<String> {
        self.interfaces.interfaces.keys().cloned().collect()
    }

    /// Writes one global block. See rules I-6 through I-10.
    fn write_global_block(&mut self, item: &SyntaxNode) {
        let Some(name) = item
            .children()
            .find(|child| child.kind() == NAME)
            .and_then(|node| node.first_token())
            .map(|token| token.text().to_owned())
        else {
            return;
        };
        let module = self.module.name.clone();
        let guard = global_emit::guard_name(&module, &name);
        let init = global_emit::init_name(&module, &name);

        let _ = writeln!(self.out, "/* lark: @global {name} */");

        // Rule I-7. Every declaration becomes a file scope variable, and C
        // gives one with no initializer a zero value.
        let declarations: Vec<SyntaxNode> = item
            .children()
            .filter(|child| matches!(child.kind(), DECLARATION | FN_DEF))
            .collect();
        let mut roots = Vec::new();
        let mut initializers = Vec::new();
        for declaration in &declarations {
            let (head, value) = self.split_declaration(declaration);
            let _ = writeln!(self.out, "{head};");
            if frame::declaration_is_managed(declaration) {
                for global in frame::declared_names(declaration) {
                    roots.push(global);
                }
            }
            if let Some((target, text)) = value {
                initializers.push(format!("{target} = {text};"));
            }
        }

        // Rule I-10. A guard makes a second call do nothing.
        let _ = writeln!(self.out, "static int {guard};");
        let temps = item
            .descendants()
            .filter(|node| matches!(node.kind(), NEW_EXPR | NEW_ARRAY_EXPR))
            .count();

        let _ = writeln!(self.out, "static void {init}(void) {{");
        let _ = writeln!(self.out, "    if ({guard}) {{ return; }}");
        let _ = writeln!(self.out, "    {guard} = 1;");
        if temps > 0 {
            let plan = Plan {
                temps,
                ..Plan::default()
            };
            self.out.push_str(&managed_emit::frame_declaration(&plan));
        }
        // Rule I-8. A managed global joins the root set before anything runs.
        for global in &roots {
            let _ = writeln!(self.out, "    lark_root_register((void **)&{global}, 1);");
        }
        // Rule I-9. The initializers run in declaration order.
        for text in &initializers {
            let _ = writeln!(self.out, "    {text}");
        }
        if temps > 0 {
            let _ = writeln!(self.out, "    lark_frame_pop(&{FRAME}.h);");
        }
        let _ = write!(self.out, "}}");
    }

    /// Splits a declaration into its head and its initializer.
    ///
    /// The head becomes a file scope variable, and the initializer moves into
    /// the block's function. See rule I-7.
    fn split_declaration(
        &mut self,
        declaration: &SyntaxNode,
    ) -> (String, Option<(String, String)>) {
        let mut head = String::new();
        let mut value = None;

        for child in declaration.children_with_tokens() {
            match child {
                NodeOrToken::Node(node) if node.kind() == INIT_DECLARATOR => {
                    let declarator = node.children().find(|item| item.kind() == DECLARATOR);
                    if let Some(item) = &declarator {
                        if !head.is_empty() {
                            head.push(' ');
                        }
                        head.push_str(&self.render(item));
                    }
                    let initial = node
                        .children()
                        .find(|item| is_expression(item.kind()) || item.kind() == INIT_LIST);
                    if let Some(item) = initial {
                        let name = frame::declared_names(declaration)
                            .first()
                            .cloned()
                            .unwrap_or_default();
                        value = Some((name, self.render(&item)));
                    }
                }
                NodeOrToken::Node(node) => {
                    head.push(' ');
                    head.push_str(&self.render(&node));
                }
                NodeOrToken::Token(token) => {
                    if token.kind() == SEMICOLON || token.kind().is_trivia() {
                        continue;
                    }
                    head.push_str(token.text());
                }
            }
        }
        (head.trim().to_owned(), value)
    }

    /// Writes `@init name;` as a call to the block's initializer. See rule I-9.
    fn write_init_statement(&mut self, node: &SyntaxNode) {
        let Some(name) = node
            .children()
            .find(|child| child.kind() == NAME_REF)
            .and_then(|child| child.first_token())
            .map(|token| token.text().to_owned())
        else {
            return;
        };
        let call = global_emit::init_name(&self.module.name, &name);
        let _ = write!(self.out, "{call}();");
    }

    /// Writes a note where an interface stands.
    ///
    /// The prologue already carries its method table type and its fat pointer
    /// type, and the epilogue carries its unique id.
    fn write_interface(&mut self, item: &SyntaxNode) {
        let Some(name) = item
            .children()
            .find(|child| child.kind() == NAME)
            .and_then(|node| node.first_token())
        else {
            return;
        };
        let _ = write!(self.out, "/* lark: interface {} */", name.text());
    }

    /// Writes the functions of one implementation. See rule O-16.
    fn write_implementation(&mut self, item: &SyntaxNode) {
        let names: Vec<String> = item
            .children()
            .filter(|child| child.kind() == NAME_REF)
            .filter_map(|child| child.first_token())
            .map(|token| token.text().to_owned())
            .collect();
        let [iface, target] = names.as_slice() else {
            return;
        };

        let previous = self.implementing.take();
        self.implementing = Some((iface.clone(), target.clone()));
        for method in item.children().filter(|child| child.kind() == FN_DEF) {
            self.out.push('\n');
            self.write_function(&method);
        }
        self.implementing = previous;
    }

    /// Writes a function definition, with its shadow stack frame.
    fn write_function(&mut self, item: &SyntaxNode) {
        let mut plan = frame::plan(item, &self.interface_names());
        let is_init = managed_emit::is_init_function(item);

        // Rule M-10 with rule M-18. A managed parameter needs a slot only when
        // a collection can run while the function does. A function that cannot
        // reach an allocation reaches no safepoint either, so it keeps the
        // zero cost that constraint D-1 promises.
        if !self.reach.needs_poll(names::declared_name(item).as_deref()) {
            plan.locals -= plan.params.len();
            plan.params.clear();
        }
        let plan = plan;

        // A method of an implementation always has internal linkage, because
        // the method table holds its address.
        if self.implementing.is_some() || self.needs_static(item) {
            self.out.push_str("static ");
        }

        let previous_locals = std::mem::take(&mut self.locals);
        self.locals = local_types(item);
        let previous_function =
            std::mem::replace(&mut self.current_function, names::declared_name(item));

        let previous = self.frame.take();
        let previous_local = self.next_local;
        let previous_temp = self.next_temp;
        self.frame = if plan.needs_frame() {
            Some(plan.clone())
        } else {
            None
        };
        self.next_local = plan.params.len();
        self.next_temp = 0;

        for element in item.children_with_tokens() {
            match element {
                NodeOrToken::Node(child) if child.kind() == BLOCK_STMT => {
                    self.write_function_body(&child, &plan, is_init);
                }
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }

        self.frame = previous;
        self.next_local = previous_local;
        self.next_temp = previous_temp;
        self.locals = previous_locals;
        self.current_function = previous_function;
    }

    /// Writes the body of a function, with the frame around it.
    fn write_function_body(&mut self, body: &SyntaxNode, plan: &Plan, is_init: bool) {
        let mut opened = false;
        for element in body.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) if token.kind() == L_CURLY && !opened => {
                    opened = true;
                    self.out.push('{');
                    // Rule I-3. The runtime starts before anything else, and a
                    // frame push before it reaches no thread.
                    if is_init {
                        let startup = managed_emit::startup(
                            self.options.roots.constant(),
                            self.options.torture,
                        );
                        self.out.push_str(&startup);
                    }
                    if plan.needs_frame() {
                        self.out.push_str(&managed_emit::frame_declaration(plan));
                    }
                    // Rules I-12 and I-15. An attached block runs after the
                    // startup, in the order that chapter 07 section 4 gives.
                    self.write_attached_blocks();
                }
                NodeOrToken::Token(token) if token.kind() == R_CURLY => {
                    // A body that ends with a return already popped the frame.
                    if plan.needs_frame() && !ends_with_return(body) {
                        let _ = writeln!(self.out, "    lark_frame_pop(&{FRAME}.h);");
                    }
                    self.out.push('}');
                }
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }
    }

    /// Writes the calls for every block attached to this function.
    fn write_attached_blocks(&mut self) {
        let Some(name) = self.current_function.clone() else {
            return;
        };
        let module = self.module.name.clone();
        let blocks: Vec<String> = self
            .globals
            .attached_to(&name)
            .iter()
            .map(|block| block.name.clone())
            .collect();
        if blocks.is_empty() {
            return;
        }
        let _ = writeln!(
            self.out,
            "\n    /* lark: attached global blocks, rule I-12 */"
        );
        for block in blocks {
            let _ = writeln!(
                self.out,
                "    {}();",
                global_emit::init_name(&module, &block)
            );
        }
    }

    /// Writes one node, and every child under it.
    fn write_node(&mut self, node: &SyntaxNode) {
        match node.kind() {
            // Rule R-2. A store of a managed pointer into a managed object
            // goes through the barrier, and only for a collector that asks.
            ASSIGN_EXPR if self.write_barrier_store(node) => return,
            INIT_DECLARATOR if self.write_interface_initializer(node) => return,
            DECL_SPECIFIERS if self.write_inferred_type(node) => return,
            NEW_EXPR | NEW_ARRAY_EXPR => {
                // Rule M-28a. A `new` inside the initializer is a safepoint,
                // so it runs before this allocation rather than between this
                // allocation and its stores. Every nested one lands in its own
                // temporary slot first, and the initializer then reads slots.
                if let Some((index, type_name)) = self.hoisted.get(&range_key(node)).cloned() {
                    let _ = write!(self.out, "({type_name} *){FRAME}.t[{index}]");
                    return;
                }
                let prefix = self.hoist_nested_allocations(node);

                let (type_name, descriptor, has_header) = self.allocation_target(node);
                // The initializer and the count both hold expressions, so they
                // pass through the emitter rather than through raw text.
                let payload = self.allocation_payload(node);
                let text = managed_emit::new_expression(
                    node,
                    &type_name,
                    &descriptor,
                    has_header,
                    &payload,
                    self.frame.as_ref().map(|_| self.next_temp),
                );
                if self.frame.is_some() {
                    self.next_temp += 1;
                }
                if prefix.is_empty() {
                    self.out.push_str(&text);
                } else {
                    let _ = write!(self.out, "({prefix}{text})");
                }
                return;
            }
            DECL_STMT if self.frame.is_some() => {
                self.write_decl_stmt(node);
                return;
            }
            RETURN_STMT if self.frame.is_some() => {
                self.write_return(node);
                return;
            }
            // Rule M-11. A managed local declared in a `for` initializer needs
            // its registration, and a registration is a statement, which no
            // `for` header holds. The loop moves into a block instead.
            FOR_STMT if self.frame.is_some() && self.managed_for_init(node).is_some() => {
                self.write_for_hoisted(node);
                return;
            }
            // Rule M-16 puts a poll at a loop back edge, and rule M-18 takes
            // it back out of a function that cannot reach an allocation.
            WHILE_STMT | FOR_STMT | DO_STMT
                if self.uses_runtime && self.reach.needs_poll(self.current_function.as_deref()) =>
            {
                self.write_loop(node);
                return;
            }
            METHOD_EXPR if self.write_method_call(node) => return,
            INIT_STMT => {
                self.write_init_statement(node);
                return;
            }
            CALL_EXPR if self.write_foreign_call(node) => return,
            // Rule G-5. A use of a generic names its instantiation.
            GENERIC_ARGS => return,
            NAME_REF | PATH if self.write_generic_use(node) => return,
            NAME => {
                // Rule O-16. A method of an implementation carries a name that
                // says which interface and which type it belongs to.
                // Only the method's own name changes. A parameter keeps the
                // name that the programmer wrote.
                if let Some((iface, target)) = self.implementing.clone()
                    && node.parent().is_some_and(|parent| {
                        parent.kind() == DECLARATOR
                            && parent.parent().is_some_and(|item| item.kind() == FN_DEF)
                    })
                    && let Some(token) = node.first_token()
                {
                    let name =
                        iface_emit::method_name(&self.module.name, &iface, &target, token.text());
                    self.out.push_str(&name);
                    return;
                }
            }
            _ => {}
        }
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }
    }

    /// Returns the type, the descriptor, and whether an allocation has a header.
    ///
    /// A generic instantiation carries its own name and its own field map. See
    /// rules G-1 and G-13.
    fn allocation_target(&mut self, node: &SyntaxNode) -> (String, String, bool) {
        if let Some((mangled, _has_header)) = self.generic_allocation(node) {
            // Rule M-5a and rule G-13. Every instantiation has a descriptor of
            // its own, so the size is right even with no managed field.
            let descriptor = format!("&{mangled}__ti");
            return (mangled, descriptor, true);
        }
        let written = managed_emit::element_type(node).unwrap_or_default();
        // Rule N-4. `mod::Name` names a record of another module, and rule X-5
        // keeps only the name in the emitted C.
        let (owner, name) = match written.split_once("::") {
            Some((module, name)) => (module.to_owned(), name.to_owned()),
            None => (self.module.name.clone(), written),
        };
        let records = if owner == self.module.name {
            &self.managed
        } else {
            match self.imported.get(&owner) {
                Some(found) => found,
                None => &self.managed,
            }
        };
        // Rule M-5a. Every managed record has a descriptor, because `lark_new`
        // copies `size` bytes from the initializer. `lark_bytes_type` says one
        // byte, so a record without its own descriptor lost every field past
        // the first byte.
        let described = records.has_record(&name);
        let has_header = records.needs_header(&name);
        let descriptor = if described {
            format!("&{}", managed_emit::typeinfo_name(&owner, &name))
        } else {
            "&lark_bytes_type".to_owned()
        };
        (name, descriptor, described || has_header)
    }

    /// Emits every `new` nested inside the initializer of one `new`.
    ///
    /// Rule M-28a. An allocation is a safepoint, so no allocation may run
    /// between an outer allocation and the stores that fill it. A collector
    /// that moves objects would otherwise write through the address that the
    /// outer allocation returned before the move.
    ///
    /// The result is a prefix of comma separated expressions. Each one fills a
    /// temporary slot, and the map records the slot so that the initializer
    /// reads it instead of allocating again.
    fn hoist_nested_allocations(&mut self, node: &SyntaxNode) -> String {
        if self.frame.is_none() {
            return String::new();
        }
        let inner: Vec<SyntaxNode> = node
            .descendants()
            .skip(1)
            .filter(|child| matches!(child.kind(), NEW_EXPR | NEW_ARRAY_EXPR))
            .filter(|child| !self.hoisted.contains_key(&range_key(child)))
            .collect();
        if inner.is_empty() {
            return String::new();
        }

        let mut prefix = String::new();
        for child in inner {
            // A deeper allocation goes first, so the recursion bottoms out.
            let deeper = self.hoist_nested_allocations(&child);
            prefix.push_str(&deeper);

            let (type_name, descriptor, has_header) = self.allocation_target(&child);
            let payload = self.allocation_payload(&child);
            let index = self.next_temp;
            let text = managed_emit::new_expression(
                &child,
                &type_name,
                &descriptor,
                has_header,
                &payload,
                Some(index),
            );
            self.next_temp += 1;
            self.hoisted
                .insert(range_key(&child), (index, type_name.clone()));
            prefix.push_str(&text);
            prefix.push_str(", ");
        }
        prefix
    }

    /// Returns the initializer of a `new`, or the count of a `new T[n]`.
    fn allocation_payload(&mut self, node: &SyntaxNode) -> String {
        if node.kind() == NEW_ARRAY_EXPR {
            let count = node.children().find(|child| is_expression(child.kind()));
            return count.map_or_else(|| "0".to_owned(), |child| self.render(&child));
        }
        let list = node.children().find(|child| child.kind() == INIT_LIST);
        list.map_or_else(|| "{ 0 }".to_owned(), |child| self.render(&child))
    }

    /// Returns the instance name and header state of a generic allocation.
    fn generic_allocation(&self, node: &SyntaxNode) -> Option<(String, bool)> {
        let type_name = node.children().find(|child| child.kind() == TYPE_NAME)?;
        let specifiers = type_name
            .children()
            .find(|child| child.kind() == DECL_SPECIFIERS)?;
        let reference = specifiers
            .children()
            .find(|child| child.kind() == NAME_REF)?;
        let name = reference.first_token()?.text().to_owned();
        let generic = self.program.generic(&name)?;
        let list = reference
            .next_sibling()
            .filter(|item| item.kind() == GENERIC_ARGS)?;
        let arguments: Vec<String> = list
            .children()
            .filter(|child| child.kind() == TYPE_NAME)
            .map(|child| lark_mono::resolve(self.program, &lark_mono::type_text(&child)))
            .collect();
        let mangled = lark_mono::mangle::instance(&generic.module, &name, &arguments);
        let instance = self
            .program
            .instances_of(&generic.module)
            .iter()
            .find(|item| item.mangled == mangled)?;
        Some((mangled, generic_emit::needs_header(generic, instance)))
    }

    /// Writes the mangled name of a generic use, and reports whether it did.
    fn write_generic_use(&mut self, node: &SyntaxNode) -> bool {
        let Some(token) = node.first_token() else {
            return false;
        };
        let Some(generic) = self.program.generic(token.text()) else {
            return false;
        };
        let Some(list) = node
            .next_sibling()
            .filter(|item| item.kind() == GENERIC_ARGS)
        else {
            return false;
        };
        // The mangle must match the one that the pass computed, so both read
        // the text and resolve a nested generic the same way. See rule X-5a.
        let arguments: Vec<String> = list
            .children()
            .filter(|child| child.kind() == TYPE_NAME)
            .map(|child| lark_mono::resolve(self.program, &lark_mono::type_text(&child)))
            .collect();
        let mangled = lark_mono::mangle::instance(&generic.module, &generic.name, &arguments);
        self.out.push_str(&mangled);
        true
    }

    /// Writes the type that `auto` infers, and reports whether it did.
    ///
    /// C11 has no `auto` inference, so the emitted C names the type. Rule T-10
    /// gives the type, and rule L-5 tells inference from the storage class.
    fn write_inferred_type(&mut self, node: &SyntaxNode) -> bool {
        let tokens: Vec<SyntaxToken> = child_tokens(node)
            .filter(|item| !item.kind().is_trivia())
            .collect();
        let [only] = tokens.as_slice() else {
            return false;
        };
        if only.kind() != AUTO_KW {
            return false;
        }
        let Some(declaration) = node.parent() else {
            return false;
        };
        let Some(value) = declaration
            .children()
            .find(|child| child.kind() == INIT_DECLARATOR)
            .and_then(|item| item.children().find(|child| is_expression(child.kind())))
        else {
            return false;
        };

        let mut store = lark_types::TypeStore::new();
        let common = store.common();
        let mut infer = lark_types::Infer {
            lowering: lark_types::Lowering {
                store: &mut store,
                common,
            },
        };
        let inferred = infer.inferred(&value);
        if infer.lowering.store.is_error(inferred) {
            return false;
        }
        let text = infer.lowering.store.display(inferred);
        // The emitted C has no `gc`, and rule T-10 keeps it only in the type.
        let text = text.replace("gc ", "");
        // Rule G-1. A generic has no C form, so the inferred type names the
        // instantiation rather than the generic. Without this, `auto` writes
        // `Box<int>*`, which is not a C type at all.
        let text = lark_mono::resolve(self.program, text.trim());
        // Rule X-5. `mod::Name` keeps only the name in the emitted C.
        let text = strip_module_prefix(&text);
        self.out.push_str(&text);
        true
    }

    /// Writes the conversion that builds an interface value. See rule O-22.
    ///
    /// Returns false when the declaration is not an interface value, so the
    /// caller writes it as it stands.
    fn write_interface_initializer(&mut self, node: &SyntaxNode) -> bool {
        let Some(declaration) = node.parent() else {
            return false;
        };
        let names = self.interface_names();
        let Some(iface) = frame::interface_name(&declaration, &names) else {
            return false;
        };
        let Some(value) = node.children().find(|child| child.kind() == NAME_EXPR) else {
            return false;
        };
        let Some(token) = value.first_token() else {
            return false;
        };
        let Some((target, _)) = self.locals.get(token.text()).cloned() else {
            return false;
        };
        if self.interfaces.find_method(&target, "").is_empty()
            && self
                .interfaces
                .interfaces_of(&target)
                .iter()
                .all(|item| item.iface != iface)
        {
            return false;
        }

        let Some(declarator) = node.children().find(|child| child.kind() == DECLARATOR) else {
            return false;
        };
        self.write_node(&declarator);
        self.out.push_str(" = ");
        let text = iface_emit::interface_value(&self.module.name, &iface, &target, token.text());
        self.out.push_str(&text);
        true
    }

    /// Writes a method call, and reports whether it produced one.
    ///
    /// Rule O-19 makes a call on a concrete type a direct call.
    fn write_method_call(&mut self, node: &SyntaxNode) -> bool {
        let Some(receiver) = node.children().find(|child| is_expression(child.kind())) else {
            return false;
        };
        let Some(selector) = node
            .children()
            .find(|child| matches!(child.kind(), NAME_REF | PATH))
        else {
            return false;
        };
        let names: Vec<String> = child_tokens(&selector)
            .filter(|token| token.kind() == IDENT)
            .map(|token| token.text().to_owned())
            .collect();
        let (iface, method) = match names.as_slice() {
            [method] => (None, method.clone()),
            [iface, method] => (Some(iface.clone()), method.clone()),
            _ => return false,
        };

        let receiver_text = self.render(&receiver);
        let arguments = node
            .children()
            .find(|child| child.kind() == ARG_LIST)
            .map(|list| {
                list.children()
                    .filter(|child| is_expression(child.kind()))
                    .map(|child| self.render(&child))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        let Some(name) = receiver.first_token().map(|token| token.text().to_owned()) else {
            return false;
        };
        let Some((type_name, is_pointer)) = self.locals.get(&name).cloned() else {
            return false;
        };

        // Rule O-20. An interface value carries its own method table.
        if self.interfaces.interfaces.contains_key(&type_name) {
            let text = iface_emit::dynamic_call(&receiver_text, &method, &arguments);
            self.out.push_str(&text);
            return true;
        }

        let site = iface_emit::CallSite {
            module: &self.module.name,
            target: &type_name,
            method: &method,
            iface: iface.as_deref(),
            receiver: &receiver_text,
            receiver_is_pointer: is_pointer,
        };
        let call = iface_emit::direct_call(site, &self.interfaces, &arguments);
        match call {
            Some(text) => {
                self.out.push_str(&text);
                true
            }
            None => false,
        }
    }

    /// Writes a foreign call with its transition, and reports whether it did.
    ///
    /// Rule M-19 puts the thread in the safe state around a `gc_safe` call, so
    /// a collection can run while the callee runs. Rule M-20 leaves a
    /// `gc_leaf` call alone.
    fn write_foreign_call(&mut self, node: &SyntaxNode) -> bool {
        // A module that links no runtime has no transition to make.
        if !self.uses_runtime {
            return false;
        }
        let Some(name) = callee_name(node) else {
            return false;
        };
        if !self.foreign.needs_transition(&name) {
            return false;
        }
        let Some((_, result)) = self.foreign.get(&name) else {
            return false;
        };
        let result = result.to_owned();

        // The rendered call keeps every argument transform, including a nested
        // foreign call. The counted transitions in the runtime handle nesting.
        let mut call = String::new();
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Node(child) => call.push_str(&self.render(&child)),
                NodeOrToken::Token(token) => {
                    if !token.kind().is_trivia() {
                        call.push_str(token.text());
                    }
                }
            }
        }

        if result.trim() != "void" {
            self.leave_helpers.insert(result.clone());
        }
        let text = foreign::safe_call(&result, &call);
        self.out.push_str(&text);
        true
    }

    /// Writes a store of a managed pointer through the barrier.
    ///
    /// Rule R-2. The barrier performs the store, so the emitted C holds one
    /// call and no assignment. A collector that needs no barrier never
    /// reaches this, because the option is off.
    ///
    /// The test is on the field name rather than on the type of the base. A
    /// barrier where none is needed is correct and only slower, and rule Y-1
    /// says which way a doubt goes.
    fn write_barrier_store(&mut self, node: &SyntaxNode) -> bool {
        if !self.options.write_barrier {
            return false;
        }
        // A compound assignment reads the field first, so it is arithmetic
        // rather than a pointer store.
        if !child_tokens(node).any(|token| token.kind() == EQ) {
            return false;
        }
        let mut children = node.children().filter(|child| is_expression(child.kind()));
        let Some(target) = children.next() else {
            return false;
        };
        let Some(value) = children.next() else {
            return false;
        };
        if !matches!(target.kind(), FIELD_EXPR | INDEX_EXPR) {
            return false;
        }
        if !self.stores_a_managed_field(&target) {
            return false;
        }

        let place = self.render(&target);
        let text = self.render(&value);
        let _ = write!(self.out, "lark_write_barrier((void **)&{place}, {text})");
        true
    }

    /// Reports whether an assignment target names a managed field.
    ///
    /// The module knows every record and every field that carries `gc`. A
    /// name that matches one is a managed field, whatever the base is.
    fn stores_a_managed_field(&self, target: &SyntaxNode) -> bool {
        let Some(name) = last_name_of(target) else {
            return false;
        };
        self.managed
            .records
            .values()
            .flat_map(Record::managed_fields)
            .any(|field| field.name == name)
    }

    /// Returns the emitted text for one node.
    fn render(&mut self, node: &SyntaxNode) -> String {
        let mut nested = Emitter {
            reach: self.reach.clone(),
            hoisted: self.hoisted.clone(),
            module: self.module,
            managed: self.managed.clone(),
            imported: self.imported.clone(),
            interfaces: self.interfaces.clone(),
            globals: self.globals.clone(),
            foreign: self.foreign.clone(),
            leave_helpers: std::collections::BTreeSet::new(),
            program: self.program,
            substitutions: self.substitutions.clone(),
            uses_runtime: self.uses_runtime,
            out: String::new(),
            line_map: Vec::new(),
            options: self.options.clone(),
            pending_skip_space: false,
            frame: self.frame.clone(),
            next_local: self.next_local,
            next_temp: self.next_temp,
            implementing: self.implementing.clone(),
            current_function: self.current_function.clone(),
            record_instances: String::new(),
            exported: self.exported.clone(),
            locals: self.locals.clone(),
        };
        nested.write_node(node);
        self.next_temp = nested.next_temp;
        // A nested render can hoist an allocation of its own, and the slot it
        // took must not be taken again.
        self.hoisted.extend(nested.hoisted);
        nested.out.trim().to_owned()
    }

    /// Writes a declaration statement, and registers any managed local.
    fn write_decl_stmt(&mut self, node: &SyntaxNode) {
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }
        let Some(declaration) = node.children().find(|child| child.kind() == DECLARATION) else {
            return;
        };
        let names = self.interface_names();
        let interface = frame::interface_name(&declaration, &names);
        if !frame::declaration_is_managed(&declaration) && interface.is_none() {
            return;
        }
        for name in frame::declared_names(&declaration) {
            let index = self.next_local;
            self.next_local += 1;
            self.out.push_str(&managed_emit::register_local(
                &name,
                index,
                interface.is_some(),
            ));
        }
    }

    /// Writes a return statement that pops the frame after the expression.
    fn write_return(&mut self, node: &SyntaxNode) {
        let Some(plan) = self.frame.clone() else {
            return;
        };
        let mut value = String::new();
        for child in node.children() {
            let mut nested = Emitter {
                reach: self.reach.clone(),
                hoisted: self.hoisted.clone(),
                module: self.module,
                managed: self.managed.clone(),
                imported: self.imported.clone(),
                interfaces: self.interfaces.clone(),
                globals: self.globals.clone(),
                foreign: self.foreign.clone(),
                leave_helpers: std::collections::BTreeSet::new(),
                program: self.program,
                substitutions: self.substitutions.clone(),
                uses_runtime: self.uses_runtime,
                out: String::new(),
                line_map: Vec::new(),
                options: self.options.clone(),
                pending_skip_space: false,
                frame: self.frame.clone(),
                next_local: self.next_local,
                next_temp: self.next_temp,
                implementing: self.implementing.clone(),
                current_function: self.current_function.clone(),
                record_instances: String::new(),
                exported: self.exported.clone(),
                locals: self.locals.clone(),
            };
            nested.write_node(&child);
            self.next_temp = nested.next_temp;
            value.push_str(&nested.out);
        }
        self.out
            .push_str(&managed_emit::return_statement(&plan, value.trim()));
    }

    /// Writes a loop, with a safepoint poll at the top of its body.
    fn write_loop(&mut self, node: &SyntaxNode) {
        let mut body_seen = false;
        for element in node.children_with_tokens() {
            match element {
                NodeOrToken::Node(child) if !body_seen && is_loop_body(node, &child) => {
                    body_seen = true;
                    self.write_loop_body(&child);
                }
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }
    }

    /// Returns the `for` initializer when it declares a managed local.
    fn managed_for_init(&self, node: &SyntaxNode) -> Option<SyntaxNode> {
        let init = node.children().find(|child| child.kind() == DECL_STMT)?;
        let declaration = init.children().find(|child| child.kind() == DECLARATION)?;
        let names = self.interface_names();
        if frame::declaration_is_managed(&declaration)
            || frame::interface_name(&declaration, &names).is_some()
        {
            Some(init)
        } else {
            None
        }
    }

    /// Writes a `for` whose initializer declares a managed local.
    ///
    /// The declaration and its registration move ahead of the loop, inside a
    /// block that keeps the name local to the loop. Rule M-11 needs the slot
    /// to join the frame after the local has a value, and both statements sit
    /// in statement position here.
    fn write_for_hoisted(&mut self, node: &SyntaxNode) {
        let Some(init) = self.managed_for_init(node) else {
            return;
        };
        let poll = self.uses_runtime && self.reach.needs_poll(self.current_function.as_deref());

        self.out.push_str("{ ");
        self.write_decl_stmt(&init);
        self.out.push(' ');

        let mut body_seen = false;
        for element in node.children_with_tokens() {
            match element {
                // The initializer already went ahead of the loop, so the
                // header keeps an empty clause in its place.
                NodeOrToken::Node(child) if child.text_range() == init.text_range() => {
                    self.out.push(';');
                }
                NodeOrToken::Node(child) if !body_seen && is_loop_body(node, &child) => {
                    body_seen = true;
                    if poll {
                        self.write_loop_body(&child);
                    } else {
                        self.write_node(&child);
                    }
                }
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }
        self.out.push_str(" }");
    }

    /// Writes the body of a loop with the poll that rule M-16 requires.
    fn write_loop_body(&mut self, body: &SyntaxNode) {
        if body.kind() != BLOCK_STMT {
            self.out.push_str("{ LARK_POLL(); ");
            self.write_node(body);
            self.out.push_str(" }");
            return;
        }
        let mut opened = false;
        for element in body.children_with_tokens() {
            match element {
                NodeOrToken::Token(token) if token.kind() == L_CURLY && !opened => {
                    opened = true;
                    self.out.push_str("{ LARK_POLL();");
                }
                NodeOrToken::Node(child) => self.write_node(&child),
                NodeOrToken::Token(token) => self.write_token(&token),
            }
        }
    }

    /// Turns `@import m` into `#include "m.h"`.
    fn write_include(&mut self, item: &SyntaxNode) {
        let name = item
            .children()
            .find(|child| child.kind() == NAME)
            .and_then(|node| node.first_token())
            .map_or_else(String::new, |token| token.text().to_owned());
        let _ = write!(self.out, "#include \"{}\"", names::header_file(&name));
    }

    /// Writes one token, or drops it when it marks Lark machinery.
    fn write_token(&mut self, token: &SyntaxToken) {
        if self.pending_skip_space && token.kind() == WHITESPACE && !token.text().contains('\n') {
            self.pending_skip_space = false;
            return;
        }
        self.pending_skip_space = false;

        // Rule G-1. Inside an instantiation, a parameter names its argument.
        if token.kind() == IDENT
            && let Some(text) = self.substitutions.get(token.text())
        {
            let text = text.clone();
            self.out.push_str(&text);
            return;
        }
        // Rule O-16. Inside an implementation, `Self` names the target type.
        if token.kind() == IDENT
            && token.text() == "Self"
            && let Some((_, target)) = &self.implementing
        {
            self.out.push_str(target);
            return;
        }
        if names::is_dropped_marker(token) {
            self.pending_skip_space = true;
            return;
        }
        if let Some(text) = names::module_path_text(token) {
            self.out.push_str(&text);
            return;
        }
        if names::is_dropped_path_part(token) {
            return;
        }
        self.out.push_str(token.text());
    }

    /// Writes a `#line` directive for an item. See rule X-3.
    fn write_line_directive(&mut self, item: &SyntaxNode) {
        if !self.options.line_directives {
            return;
        }
        let start = u32::from(item.text_range().start());
        let source_line = self.source_line(start);
        let emitted_line = self.emitted_line();
        if !self.out.ends_with('\n') && !self.out.is_empty() {
            self.out.push('\n');
        }
        let path = self
            .options
            .source_name
            .clone()
            .unwrap_or_else(|| self.module.path.display().to_string());
        let _ = writeln!(self.out, "#line {source_line} \"{path}\"");
        self.line_map.push(LineEntry {
            emitted: emitted_line + 1,
            source: source_line,
        });
    }

    /// Returns the one based source line for a byte offset.
    fn source_line(&self, offset: u32) -> u32 {
        let text = self.module.parse.text();
        let end = (offset as usize).min(text.len());
        u32::try_from(text[..end].matches('\n').count() + 1).unwrap_or(1)
    }

    /// Returns the one based line that the emitter is about to write.
    fn emitted_line(&self) -> u32 {
        u32::try_from(self.out.matches('\n').count() + 1).unwrap_or(1)
    }
}

/// Returns the name that a call names, through a path or a plain name.
fn callee_name(call: &SyntaxNode) -> Option<String> {
    let callee = call.children().find(|child| child.kind() != ARG_LIST)?;
    let names: Vec<String> = lark_syntax::all_tokens(&callee)
        .filter(|token| token.kind() == IDENT)
        .map(|token| token.text().to_owned())
        .collect();
    names.last().cloned()
}

/// Returns the type of every parameter and local of a function.
///
/// The map holds the type name and whether the declaration is a pointer, which
/// rule O-18 needs to adapt a receiver.
fn local_types(item: &SyntaxNode) -> std::collections::BTreeMap<String, (String, bool)> {
    let mut found = std::collections::BTreeMap::new();
    for node in item.descendants() {
        let declaration = match node.kind() {
            PARAM => Some(node.clone()),
            DECL_STMT => node.children().find(|child| child.kind() == DECLARATION),
            _ => None,
        };
        let Some(declaration) = declaration else {
            continue;
        };
        let Some(specifiers) = declaration
            .children()
            .find(|child| child.kind() == DECL_SPECIFIERS)
        else {
            continue;
        };
        let named = specifiers
            .children()
            .find(|child| child.kind() == NAME_REF)
            .and_then(|child| child.first_token())
            .map(|token| token.text().to_owned());

        // Rule T-10. An `auto` declaration takes its type from the initializer,
        // and rule O-17 needs that type to resolve a method call.
        let (type_name, is_pointer) = match named {
            Some(name) => (
                name,
                declaration.descendants().any(|item| item.kind() == POINTER),
            ),
            None => match inferred_local_type(&declaration) {
                Some(pair) => pair,
                None => continue,
            },
        };
        for name in frame::declared_names(&declaration) {
            found.insert(name, (type_name.clone(), is_pointer));
        }
    }
    found
}

/// Returns the type name and pointer state that `auto` infers.
fn inferred_local_type(declaration: &SyntaxNode) -> Option<(String, bool)> {
    let value = declaration
        .children()
        .find(|child| child.kind() == INIT_DECLARATOR)
        .and_then(|item| item.children().find(|child| is_expression(child.kind())))?;

    let mut store = lark_types::TypeStore::new();
    let common = store.common();
    let mut infer = lark_types::Infer {
        lowering: lark_types::Lowering {
            store: &mut store,
            common,
        },
    };
    let inferred = infer.inferred(&value);
    if infer.lowering.store.is_error(inferred) {
        return None;
    }
    let text = infer.lowering.store.display(inferred).replace("gc ", "");
    let is_pointer = text.trim_end().ends_with('*');
    let name = text.trim_end_matches(['*', ' ']).trim().to_owned();
    if name.is_empty() {
        None
    } else {
        Some((name, is_pointer))
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

/// Reports whether the last statement of a block is a `return`.
fn ends_with_return(body: &SyntaxNode) -> bool {
    body.children()
        .last()
        .is_some_and(|last| last.kind() == RETURN_STMT)
}

/// Reports whether a node is the body of a loop rather than its head.
fn is_loop_body(loop_node: &SyntaxNode, child: &SyntaxNode) -> bool {
    let statements = [
        BLOCK_STMT,
        EXPR_STMT,
        IF_STMT,
        WHILE_STMT,
        DO_STMT,
        FOR_STMT,
        SWITCH_STMT,
        RETURN_STMT,
        EMPTY_STMT,
        DECL_STMT,
        BREAK_STMT,
        CONTINUE_STMT,
        GOTO_STMT,
        LABELED_STMT,
        CASE_STMT,
        DEFAULT_STMT,
        INIT_STMT,
    ];
    if !statements.contains(&child.kind()) {
        return false;
    }
    // A `do` loop puts its body first, and every other loop puts it last.
    if loop_node.kind() == DO_STMT {
        return true;
    }
    loop_node
        .children()
        .filter(|node| statements.contains(&node.kind()))
        .last()
        .is_some_and(|last| last.text_range() == child.text_range())
}

/// Returns every name that a module exports.
///
/// Rule N-6 marks a symbol, not one declaration of it. A prototype that carries
/// `export` therefore exports the definition that follows.
fn exported_names(root: &SyntaxNode) -> std::collections::BTreeSet<String> {
    root.children()
        .filter(names::is_exported)
        .filter_map(|item| names::declared_name(&item))
        .collect()
}

/// Reports whether an item needs the `static` marker. See rule X-5b.
fn needs_static(item: &SyntaxNode, exported: &std::collections::BTreeSet<String>) -> bool {
    if !matches!(item.kind(), FN_DEF | DECLARATION) {
        return false;
    }
    if names::is_exported(item) {
        return false;
    }
    if names::declared_name(item).is_some_and(|name| exported.contains(&name)) {
        return false;
    }
    // A prototype declares a symbol that lives elsewhere.
    let has_body = item.children().any(|child| child.kind() == BLOCK_STMT);
    if item.kind() == DECLARATION && !has_body && !names::declares_a_variable(item) {
        return false;
    }
    if names::has_storage_class(item) {
        return false;
    }
    // Rule X-5b. The entry point stays external.
    if names::declared_name(item).as_deref() == Some("main") {
        return false;
    }
    has_body || names::declares_a_variable(item)
}
/// Reports whether an item is a top level `typedef` declaration.
fn is_local_typedef(item: &SyntaxNode) -> bool {
    if item.kind() != DECLARATION {
        return false;
    }
    item.children()
        .filter(|child| child.kind() == DECL_SPECIFIERS)
        .any(|specifiers| {
            specifiers
                .children_with_tokens()
                .any(|part| part.kind() == TYPEDEF_KW)
        })
}

/// Returns a key for one node, unique within a tree.
///
/// A node has no hash of its own, and its byte range names it exactly.
fn range_key(node: &SyntaxNode) -> (u32, u32) {
    let range = node.text_range();
    (u32::from(range.start()), u32::from(range.end()))
}

/// Removes the module part of every qualified name in a type. See rule X-5.
fn strip_module_prefix(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(index) = rest.find("::") {
        let head = &rest[..index];
        let start = head
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(0, |position| position + 1);
        out.push_str(&head[..start]);
        rest = &rest[index + 2..];
    }
    out.push_str(rest);
    out
}

/// Returns the last name that an expression reads.
///
/// `a->next` gives `next`, and `items[i].left` gives `left`. A form with no
/// name gives nothing.
fn last_name_of(node: &SyntaxNode) -> Option<String> {
    node.descendants()
        .filter(|child| matches!(child.kind(), NAME_REF | NAME))
        .filter_map(|child| {
            child_tokens(&child)
                .find(|token| token.kind() == IDENT)
                .map(|token| token.text().to_owned())
        })
        .last()
}

/// Reports whether a token is an `#include` directive.
///
/// The lexer marks a whole directive line as one trivia token, so the text
/// holds the word and the header name together.
fn is_include_directive(token: &SyntaxToken) -> bool {
    if token.kind() != PP_DIRECTIVE {
        return false;
    }
    let Some(rest) = token.text().trim_start().strip_prefix('#') else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix("include") else {
        return false;
    };
    !rest
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}
