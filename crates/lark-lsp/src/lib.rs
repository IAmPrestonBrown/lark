//! The Lark language server.
//!
//! The crate has two parts. [`Analysis`] answers a question about a position in
//! a file, and knows nothing about the protocol. The `server` module speaks the
//! protocol and asks [`Analysis`].
//!
//! Every answer works on broken code, because the parser recovers and always
//! produces a tree. Invariant R keeps every token, so a position always lands
//! on something.

// A tree walk matches on kinds constantly. Naming the enum on every arm hides
// the shape of the walk behind noise, so this module imports the variants.
#![allow(clippy::enum_glob_use)]

pub mod position;
pub mod scope;
pub mod server;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use lark_diag::Diagnostics;
use lark_resolve::{FileLoader, Resolution, SymbolKind, resolve};
use lark_span::Span;
use lark_syntax::SyntaxKind::*;
use lark_syntax::{SyntaxNode, SyntaxToken};
use lark_types::iface::Interfaces;
use lark_types::managed::Managed;

/// What one completion offers.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum CompletionKind {
    /// A module that the file imports.
    Module,
    /// A struct, a union, an enum, or a type alias.
    Type,
    /// An interface.
    Interface,
    /// A function.
    Function,
    /// A variable at file scope.
    Global,
    /// A field of a record.
    Field,
    /// A method that an interface declares.
    Method,
    /// A local variable or a parameter.
    Local,
    /// A keyword of the language.
    Keyword,
}

impl CompletionKind {
    /// Returns the word that a report prints.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Type => "type",
            Self::Interface => "iface",
            Self::Function => "fn",
            Self::Global => "global",
            Self::Field => "field",
            Self::Method => "method",
            Self::Local => "local",
            Self::Keyword => "keyword",
        }
    }
}

/// One item that the server offers at a position.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Completion {
    /// What the kind is.
    pub kind: CompletionKind,
    /// The text to insert.
    pub label: String,
    /// The signature or the type, for the detail column.
    pub detail: String,
}

/// What the server says about the symbol under the cursor.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hover {
    /// The kind of the symbol.
    pub kind: CompletionKind,
    /// The name.
    pub label: String,
    /// The signature or the type.
    pub detail: String,
}

/// Where a definition lives.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Location {
    /// The file that holds the definition.
    pub path: PathBuf,
    /// The region that the name covers.
    pub span: Span,
}

/// Everything that the server knows about one file and its imports.
pub struct Analysis {
    resolution: Resolution,
    diagnostics: Diagnostics,
    root: Option<lark_resolve::ModuleId>,
}

/// The Lark words that a completion offers at statement level.
const KEYWORDS: &[(&str, &str)] = &[
    ("auto", "infer the type from the initializer"),
    (
        "export",
        "make the declaration visible to an importing module",
    ),
    ("gc", "a managed pointer"),
    ("gc_leaf", "a foreign call with no safepoint"),
    ("gc_safe", "a foreign call that can collect"),
    ("iface", "declare an interface"),
    ("impl", "implement an interface for a type"),
    ("init", "mark where the runtime starts"),
    ("managed", "give the struct an object header"),
    ("new", "allocate in the collector heap"),
];

impl Analysis {
    /// Reads a file and everything it imports.
    pub fn new(name: &str, path: &Path, text: &str, search: &[PathBuf]) -> Self {
        let loader = FileLoader::new(search.to_vec());
        let resolution = resolve(&loader, name, path, text);
        let mut diagnostics = resolution.diagnostics.clone();
        diagnostics.extend(lark_types::check_resolution(&resolution));
        diagnostics.sort_by_position();
        let root = resolution.root;
        Self {
            resolution,
            diagnostics,
            root,
        }
    }

    /// Returns every problem that the passes found.
    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Returns the tree of the file that the server opened.
    fn tree(&self) -> Option<SyntaxNode> {
        let module = self.resolution.graph.get(self.root?)?;
        Some(module.parse.syntax())
    }

    /// Returns the module that the server opened.
    fn module(&self) -> Option<&lark_resolve::Module> {
        self.resolution.graph.get(self.root?)
    }

    /// Returns what the server offers at an offset.
    pub fn completions(&self, offset: u32) -> Vec<Completion> {
        let Some(root) = self.tree() else {
            return Vec::new();
        };
        let mut found = match Self::trigger(&root, offset) {
            Some(Trigger::Member(receiver)) => Self::member_completions(&root, offset, &receiver),
            Some(Trigger::Module(name)) => self.module_completions(&name),
            None => self.scope_completions(&root, offset),
        };
        found.sort();
        found.dedup();
        found
    }

    /// Returns what the cursor sits on.
    pub fn hover(&self, offset: u32) -> Option<Hover> {
        let root = self.tree()?;
        let token = ident_at_or_before(&root, offset)?;
        let name = token.text().to_owned();

        let locals = scope::locals_at(&root, offset);
        if let Some(local) = locals.get(&name) {
            return Some(Hover {
                kind: CompletionKind::Local,
                label: name,
                detail: local.type_text.clone(),
            });
        }

        let module = self.module()?;
        if let Some(symbol) = module.table.get(&name) {
            return Some(Hover {
                kind: kind_of(symbol.kind),
                label: name,
                detail: format!("{} in module `{}`", symbol.kind.word(), module.name),
            });
        }

        // A method or a field of the receiver before the cursor.
        let interfaces = lark_types::iface::collect(&root);
        for item in &interfaces.implementations {
            if let Some(method) = item.methods.iter().find(|entry| entry.name == name) {
                return Some(Hover {
                    kind: CompletionKind::Method,
                    label: name,
                    detail: format!(
                        "{} in `impl {} for {}`",
                        method.result, item.iface, item.target
                    ),
                });
            }
        }
        None
    }

    /// Returns where the symbol under the cursor is declared.
    pub fn definition(&self, offset: u32) -> Option<Location> {
        let root = self.tree()?;
        let token = ident_at_or_before(&root, offset)?;
        let name = token.text().to_owned();
        let module = self.module()?;

        // A qualified name resolves in the module that the prefix names.
        if let Some(prefix) = module_prefix(&token)
            && let Some(target) = self.resolution.graph.import_target(module.id, &prefix)
            && let Some(other) = self.resolution.graph.get(target)
            && let Some(symbol) = other.table.get(&name)
        {
            return Some(Location {
                path: other.path.clone(),
                span: symbol.span,
            });
        }

        let locals = scope::locals_at(&root, offset);
        if let Some(local) = locals.get(&name) {
            return Some(Location {
                path: module.path.clone(),
                span: Span::new(local.offset, local.offset + text_len(&name)),
            });
        }
        let symbol = module.table.get(&name)?;
        Some(Location {
            path: module.path.clone(),
            span: symbol.span,
        })
    }

    /// Returns what triggered a completion, when something did.
    fn trigger(root: &SyntaxNode, offset: u32) -> Option<Trigger> {
        let token = token_before(root, offset)?;
        match token.kind() {
            DOT | ARROW => {
                let receiver = previous_name(&token)?;
                Some(Trigger::Member(receiver))
            }
            COLON2 => {
                let name = previous_name(&token)?;
                Some(Trigger::Module(name))
            }
            _ => None,
        }
    }

    /// Returns the fields and the methods of the type before a dot.
    fn member_completions(root: &SyntaxNode, offset: u32, receiver: &str) -> Vec<Completion> {
        let locals = scope::locals_at(root, offset);
        let Some(local) = locals.get(receiver) else {
            return Vec::new();
        };
        let target = local.type_name.clone();

        let mut found = Vec::new();
        let managed: Managed = lark_types::managed::collect(root);
        if let Some(record) = managed.records.get(&target) {
            for field in &record.fields {
                found.push(Completion {
                    kind: CompletionKind::Field,
                    label: field.name.clone(),
                    detail: format!("field of `{target}`"),
                });
            }
        }

        // Rule O-17 resolves a method across every interface that the type
        // implements.
        let interfaces: Interfaces = lark_types::iface::collect(root);
        for item in interfaces.interfaces_of(&target) {
            for method in &item.methods {
                found.push(Completion {
                    kind: CompletionKind::Method,
                    label: method.name.clone(),
                    detail: format!("{} from `{}`", method.result, item.iface),
                });
            }
        }
        found
    }

    /// Returns the exported names of a module. See rules N-2 and N-6.
    fn module_completions(&self, name: &str) -> Vec<Completion> {
        let Some(module) = self.module() else {
            return Vec::new();
        };
        let Some(target) = self.resolution.graph.import_target(module.id, name) else {
            return Vec::new();
        };
        let Some(other) = self.resolution.graph.get(target) else {
            return Vec::new();
        };
        other
            .table
            .iter()
            .filter(|symbol| symbol.visibility.is_exported())
            .map(|symbol| Completion {
                kind: kind_of(symbol.kind),
                label: symbol.name.clone(),
                detail: format!("{} in module `{}`", symbol.kind.word(), other.name),
            })
            .collect()
    }

    /// Returns everything that a bare name can be at an offset.
    fn scope_completions(&self, root: &SyntaxNode, offset: u32) -> Vec<Completion> {
        let mut found = Vec::new();

        for local in scope::locals_at(root, offset).values() {
            found.push(Completion {
                kind: CompletionKind::Local,
                label: local.name.clone(),
                detail: local.type_text.clone(),
            });
        }

        if let Some(module) = self.module() {
            for symbol in module.table.iter() {
                found.push(Completion {
                    kind: kind_of(symbol.kind),
                    label: symbol.name.clone(),
                    detail: format!("{} in module `{}`", symbol.kind.word(), module.name),
                });
            }
            // Rule N-2 needs the prefix, so an imported module is a completion.
            for entry in &module.imports {
                found.push(Completion {
                    kind: CompletionKind::Module,
                    label: entry.import.name.clone(),
                    detail: "imported module".to_owned(),
                });
            }
        }

        for (word, detail) in KEYWORDS {
            found.push(Completion {
                kind: CompletionKind::Keyword,
                label: (*word).to_owned(),
                detail: (*detail).to_owned(),
            });
        }
        found
    }

    /// Returns the answer in the form that a snapshot test compares.
    pub fn report(&self, query: Query, offset: u32) -> String {
        let mut out = String::new();
        match query {
            Query::Completion => {
                for item in self.completions(offset) {
                    let _ = writeln!(
                        out,
                        "{} {} -- {}",
                        item.kind.word(),
                        item.label,
                        item.detail
                    );
                }
            }
            Query::Hover => match self.hover(offset) {
                Some(item) => {
                    let _ = writeln!(
                        out,
                        "{} {} -- {}",
                        item.kind.word(),
                        item.label,
                        item.detail
                    );
                }
                None => out.push_str("nothing\n"),
            },
            Query::Definition => match self.definition(offset) {
                Some(item) => {
                    let name = item
                        .path
                        .file_name()
                        .map_or_else(String::new, |value| value.to_string_lossy().into_owned());
                    let _ = writeln!(out, "{name}:{}..{}", item.span.start, item.span.end);
                }
                None => out.push_str("nothing\n"),
            },
        }
        out
    }
}

/// What the server is asked for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Query {
    /// What can go here.
    Completion,
    /// What is here.
    Hover,
    /// Where is this declared.
    Definition,
}

impl Query {
    /// Reads a query from the word that a fixture writes.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "completion" => Some(Self::Completion),
            "hover" => Some(Self::Hover),
            "definition" => Some(Self::Definition),
            _ => None,
        }
    }
}

/// What triggered a completion.
enum Trigger {
    /// A dot or an arrow, with the receiver name.
    Member(String),
    /// A scope operator, with the module name.
    Module(String),
}

/// Returns the completion kind for a symbol kind.
fn kind_of(kind: SymbolKind) -> CompletionKind {
    match kind {
        SymbolKind::Type => CompletionKind::Type,
        SymbolKind::Iface => CompletionKind::Interface,
        SymbolKind::Function => CompletionKind::Function,
        SymbolKind::Global => CompletionKind::Global,
    }
}

/// Returns the identifier that a cursor touches, or the one just before it.
///
/// A cursor sits between two characters. A reader who puts it at the end of a
/// name means that name.
fn ident_at_or_before(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    if let Some(token) = token_at(root, offset)
        && token.kind() == IDENT
    {
        return Some(token);
    }
    let token = token_before(root, offset)?;
    if token.kind() == IDENT {
        Some(token)
    } else {
        None
    }
}

/// Returns the token that covers an offset.
fn token_at(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    lark_syntax::all_tokens(root).find(|token| {
        let range = token.text_range();
        u32::from(range.start()) <= offset && offset < u32::from(range.end())
    })
}

/// Returns the last token that ends at or before an offset, trivia skipped.
fn token_before(root: &SyntaxNode, offset: u32) -> Option<SyntaxToken> {
    lark_syntax::all_tokens(root)
        .filter(|token| !token.kind().is_trivia())
        .filter(|token| u32::from(token.text_range().end()) <= offset)
        .last()
}

/// Returns the identifier before a token, when one is there.
fn previous_name(token: &SyntaxToken) -> Option<String> {
    let mut current = token.prev_token();
    while let Some(item) = current {
        if item.kind().is_trivia() {
            current = item.prev_token();
            continue;
        }
        if item.kind() == IDENT {
            return Some(item.text().to_owned());
        }
        return None;
    }
    None
}

/// Returns the module prefix of a qualified name, when it has one.
fn module_prefix(token: &SyntaxToken) -> Option<String> {
    let parent = token.parent()?;
    if parent.kind() != PATH {
        return None;
    }
    let names: Vec<SyntaxToken> = lark_syntax::child_tokens(&parent)
        .filter(|item| item.kind() == IDENT)
        .collect();
    match names.as_slice() {
        [prefix, name] if name.text_range() == token.text_range() => Some(prefix.text().to_owned()),
        _ => None,
    }
}

/// Returns the byte length of a name.
fn text_len(name: &str) -> u32 {
    u32::try_from(name.len()).unwrap_or(0)
}
