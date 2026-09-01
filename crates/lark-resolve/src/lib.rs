//! The Lark module graph, symbol tables, and name resolution.
//!
//! The resolver runs the two passes that rule L-8 describes.
//!
//! 1. Parse every module with no name information, and record every top level
//!    name. Rule N-3 finds the file for each `@import`.
//! 2. Parse every module again, with an oracle built from pass one. Rule L-6
//!    then decides a generic argument list from a comparison.
//!
//! The checks report only what the resolver can decide. Delivery phase A does
//! not read headers, so a name that no module declares can still come from
//! `#include`.

pub mod check;
pub mod collect;
pub mod headers;
pub mod module;
pub mod oracle;
pub mod symbol;

use std::path::{Path, PathBuf};

use lark_diag::Diagnostics;
use lark_span::SourceMap;
use lark_syntax::{NoNames, parse};

pub use collect::{Collected, Import, collect};
pub use headers::{HeaderIndex, HeaderReader, NoHeaders};
pub use module::{
    FileLoader, MemoryLoader, Module, ModuleGraph, ModuleId, ResolvedImport, SourceLoader,
};
pub use oracle::ModuleNames;
pub use symbol::{Symbol, SymbolKind, SymbolTable, Visibility};

/// The result of a resolver run.
#[derive(Debug)]
pub struct Resolution {
    /// Every module that the run read.
    pub graph: ModuleGraph,
    /// The text of every module.
    pub sources: SourceMap,
    /// Every problem that the run found.
    pub diagnostics: Diagnostics,
    /// The handle for the module that the run started from.
    pub root: Option<ModuleId>,
}

/// Resolves a root module and everything it imports.
///
/// `name` is the module name of the root file, and `text` is its content.
/// `path` is the file that the text came from, which fixes the directory that
/// rule N-3 searches first.
pub fn resolve(loader: &dyn SourceLoader, name: &str, path: &Path, text: &str) -> Resolution {
    resolve_with(loader, &NoHeaders, name, path, text)
}

/// Resolves a root module, with a reader for the C headers it includes.
///
/// `headers` reads every `#include`. Rule N-12 adds the names it finds to the
/// module, and rule L-15 makes the resulting table complete.
pub fn resolve_with(
    loader: &dyn SourceLoader,
    headers: &dyn HeaderReader,
    name: &str,
    path: &Path,
    text: &str,
) -> Resolution {
    let mut builder = Builder {
        loader,
        headers,
        graph: ModuleGraph::new(),
        sources: SourceMap::new(),
        diagnostics: Diagnostics::new(),
        pending: Vec::new(),
    };

    let root = builder.load_module(name, path, text);
    while let Some(id) = builder.pending.pop() {
        builder.load_imports_of(id);
    }
    builder.second_pass();

    let Builder {
        graph,
        sources,
        mut diagnostics,
        ..
    } = builder;

    // The lexer and the parser report first, then the resolver.
    for module in graph.modules() {
        for error in module.parse.errors() {
            diagnostics.push(lark_diag::Diagnostic::new(
                error.code,
                module.source,
                error.span,
            ));
        }
    }
    for id in 0..graph.len() {
        check::check(&graph, id, &mut diagnostics);
    }
    diagnostics.sort_by_position();

    Resolution {
        graph,
        sources,
        diagnostics,
        root,
    }
}

/// Resolves a file on disk, with a search path from the configuration.
///
/// # Errors
///
/// Returns an error when the file cannot be read.
pub fn resolve_path(path: &Path, search: &[PathBuf]) -> std::io::Result<Resolution> {
    resolve_path_with(path, search, &NoHeaders)
}

/// Resolves a file on disk, with a reader for the C headers it includes.
///
/// # Errors
///
/// Returns an error when the file cannot be read.
pub fn resolve_path_with(
    path: &Path,
    search: &[PathBuf],
    headers: &dyn HeaderReader,
) -> std::io::Result<Resolution> {
    let text = std::fs::read_to_string(path)?;
    let name = path.file_stem().map_or_else(
        || "main".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let loader = FileLoader::new(search.to_vec());
    Ok(resolve_with(&loader, headers, &name, path, &text))
}

/// The state that one resolver run carries.
struct Builder<'a> {
    loader: &'a dyn SourceLoader,
    headers: &'a dyn HeaderReader,
    graph: ModuleGraph,
    sources: SourceMap,
    diagnostics: Diagnostics,
    pending: Vec<ModuleId>,
}

impl Builder<'_> {
    /// Adds one module from its text, and runs pass one over it.
    fn load_module(&mut self, name: &str, path: &Path, text: &str) -> Option<ModuleId> {
        if let Some(existing) = self.graph.id_of_path(path) {
            return Some(existing);
        }
        let source = self.sources.add(path.to_path_buf(), text.to_owned()).ok()?;
        let parsed = parse(text, &NoNames);
        let found = collect(&parsed.syntax());
        // Rule C-1. The headers of a module are part of pass one, so the
        // oracle of pass two knows every name that a header declares.
        let headers = self.headers.read(text, path);

        let id = self.graph.len();
        let imports = found
            .imports
            .iter()
            .map(|import| ResolvedImport {
                import: import.clone(),
                target: None,
            })
            .collect();
        self.graph.push(Module {
            id,
            name: name.to_owned(),
            path: path.to_path_buf(),
            source,
            parse: parsed,
            table: found.table,
            namespaces: found.namespaces,
            imports,
            has_unread_include: found.has_unread_include && !headers.is_complete(),
            headers,
            macros: found.macros,
        });
        self.pending.push(id);
        Some(id)
    }

    /// Loads every module that one module imports.
    ///
    /// Rule N-4 allows a cycle, so a module that the graph already holds is
    /// linked rather than loaded again.
    fn load_imports_of(&mut self, id: ModuleId) {
        let Some(module) = self.graph.get(id) else {
            return;
        };
        let directory = module.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let requests: Vec<String> = module
            .imports
            .iter()
            .map(|entry| entry.import.name.clone())
            .collect();

        let mut targets = Vec::with_capacity(requests.len());
        for name in &requests {
            let target = match self.loader.load(name, &directory) {
                Some((path, text)) => self.load_module(name, &path, &text),
                None => None,
            };
            targets.push(target);
        }

        if let Some(module) = self.graph.get_mut(id) {
            for (entry, target) in module.imports.iter_mut().zip(targets) {
                entry.target = target;
            }
        }
    }

    /// Parses every module again, with the oracle that pass one produced.
    fn second_pass(&mut self) {
        for id in 0..self.graph.len() {
            let Some(module) = self.graph.get(id) else {
                continue;
            };
            let complete = !module.has_unread_include;
            let names = ModuleNames::from_table(&module.table, complete)
                .with_headers(&module.headers)
                .with_macros(module.macros.iter().map(String::as_str));
            let text = self.sources.file(module.source).text().to_owned();
            let parsed = parse(&text, &names);
            let found = collect(&parsed.syntax());
            if let Some(module) = self.graph.get_mut(id) {
                module.parse = parsed;
                module.table = found.table;
                module.macros = found.macros;
            }
        }
    }
}
