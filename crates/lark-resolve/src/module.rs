//! The module graph and the loaders that fill it.
//!
//! One file is one module. Rule N-1 gives the module its name, and rule N-3
//! gives the search order that finds its file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lark_span::SourceId;
use lark_syntax::Parse;

use std::collections::BTreeSet;

use crate::collect::Import;
use crate::headers::HeaderIndex;
use crate::symbol::SymbolTable;

/// A handle to one module in a [`ModuleGraph`].
pub type ModuleId = usize;

/// One module, after both passes.
#[derive(Debug)]
pub struct Module {
    /// The handle for this module.
    pub id: ModuleId,
    /// The file name without the `.lark` extension. See rule N-1.
    pub name: String,
    /// The file that the module came from.
    pub path: PathBuf,
    /// The handle for the text in the source map.
    pub source: SourceId,
    /// The tree from the second pass.
    pub parse: Parse,
    /// Every top level name that the module declares.
    pub table: SymbolTable,
    /// Every `@import` directive, with the module it names.
    pub imports: Vec<ResolvedImport>,
    /// Whether the module holds an `#include` that the front end cannot read.
    pub has_unread_include: bool,
    /// Every name that the headers of the module declare. See rule N-12.
    pub headers: HeaderIndex,
    /// Every macro that the module itself defines. See rule C-2a.
    pub macros: BTreeSet<String>,
}

/// One `@import` directive, with the module it found.
#[derive(Clone, Debug)]
pub struct ResolvedImport {
    /// The directive as written.
    pub import: Import,
    /// The module that the name found, or `None` when the search failed.
    pub target: Option<ModuleId>,
}

/// Every module that one compiler run reads.
#[derive(Debug, Default)]
pub struct ModuleGraph {
    modules: Vec<Module>,
    by_path: BTreeMap<PathBuf, ModuleId>,
}

impl ModuleGraph {
    /// Builds an empty graph.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module and returns its handle.
    pub fn push(&mut self, module: Module) -> ModuleId {
        let id = module.id;
        self.by_path.insert(module.path.clone(), id);
        self.modules.push(module);
        id
    }

    /// Returns the handle for a file that the graph already holds.
    #[must_use]
    pub fn id_of_path(&self, path: &Path) -> Option<ModuleId> {
        self.by_path.get(path).copied()
    }

    /// Returns the module for a handle.
    #[must_use]
    pub fn get(&self, id: ModuleId) -> Option<&Module> {
        self.modules.get(id)
    }

    /// Returns the module for a handle, for a change.
    pub fn get_mut(&mut self, id: ModuleId) -> Option<&mut Module> {
        self.modules.get_mut(id)
    }

    /// Returns every module, in load order.
    #[must_use]
    pub fn modules(&self) -> &[Module] {
        &self.modules
    }

    /// Returns the number of modules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Reports whether the graph holds no module.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Returns the module that one module imports under a name.
    #[must_use]
    pub fn import_target(&self, from: ModuleId, name: &str) -> Option<ModuleId> {
        let module = self.get(from)?;
        module
            .imports
            .iter()
            .find(|entry| entry.import.name == name)
            .and_then(|entry| entry.target)
    }
}

/// Finds the file for a module name.
///
/// Rule N-3 gives the search order: the directory of the importing file, then
/// each directory in `paths.search`.
pub trait SourceLoader {
    /// Returns the path and the text for a module name.
    ///
    /// `from` is the directory of the file that holds the `@import`.
    fn load(&self, name: &str, from: &Path) -> Option<(PathBuf, String)>;
}

/// A loader that reads from the file system.
#[derive(Clone, Debug, Default)]
pub struct FileLoader {
    /// Directories that the search visits after the importing directory.
    pub search: Vec<PathBuf>,
}

impl FileLoader {
    /// Builds a loader with a search path.
    pub fn new<I, P>(search: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        Self {
            search: search.into_iter().map(Into::into).collect(),
        }
    }
}

impl SourceLoader for FileLoader {
    fn load(&self, name: &str, from: &Path) -> Option<(PathBuf, String)> {
        let file = format!("{name}.lark");
        let mut directories = vec![from.to_path_buf()];
        directories.extend(self.search.iter().cloned());
        for directory in directories {
            let candidate = directory.join(&file);
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                let path = candidate.canonicalize().unwrap_or(candidate);
                return Some((path, text));
            }
        }
        None
    }
}

/// A loader that holds files in memory. Tests use it.
#[derive(Clone, Debug, Default)]
pub struct MemoryLoader {
    files: BTreeMap<String, String>,
}

impl MemoryLoader {
    /// Builds a loader from module names and their text.
    pub fn new<I, N, T>(files: I) -> Self
    where
        I: IntoIterator<Item = (N, T)>,
        N: Into<String>,
        T: Into<String>,
    {
        Self {
            files: files
                .into_iter()
                .map(|(name, text)| (name.into(), text.into()))
                .collect(),
        }
    }
}

impl SourceLoader for MemoryLoader {
    fn load(&self, name: &str, _from: &Path) -> Option<(PathBuf, String)> {
        let text = self.files.get(name)?;
        Some((PathBuf::from(format!("{name}.lark")), text.clone()))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{MemoryLoader, SourceLoader};

    #[test]
    fn a_memory_loader_finds_a_module_it_holds() {
        let loader = MemoryLoader::new([("stdio", "int printf(const char* f, ...);")]);
        let found = loader.load("stdio", Path::new("."));
        assert!(found.is_some());
        assert!(loader.load("missing", Path::new(".")).is_none());
    }

    #[test]
    fn a_loaded_module_carries_the_lark_extension() {
        let loader = MemoryLoader::new([("stdio", "int x;")]);
        let Some((path, _)) = loader.load("stdio", Path::new(".")) else {
            panic!("the loader must find stdio");
        };
        assert_eq!(path.to_string_lossy(), "stdio.lark");
    }
}
