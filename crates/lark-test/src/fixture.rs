//! Fixture discovery on disk.
//!
//! Each [`Kind`] maps to one test type in `docs/test-strategy.md`, and to one
//! directory under `tests/`.

use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The kind of a fixture test.
///
/// Each kind maps to one test type in `docs/test-strategy.md`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Kind {
    /// T2. Parse a file and compare the tree to a snapshot.
    Parse,
    /// T3. Parse a valid C11 file and report no diagnostic.
    C11,
    /// T4. Compile a file and match the diagnostics to inline annotations.
    Ui,
    /// T5. Compile a file and compare the emitted C to a snapshot.
    Golden,
    /// T6. Compile, build, run, and compare the output.
    Exec,
    /// T7. Check that the emitted C maps back to the Lark source.
    DebugMap,
    /// T9. Run a collector stress program.
    Gc,
    /// T10. Check a language server answer at a cursor marker.
    Lsp,
}

/// Every kind, in the order that a report lists them.
pub const KINDS: &[Kind] = &[
    Kind::Parse,
    Kind::C11,
    Kind::Ui,
    Kind::Golden,
    Kind::Exec,
    Kind::DebugMap,
    Kind::Gc,
    Kind::Lsp,
];

impl Kind {
    /// Returns the short name that a test report prints.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::C11 => "c11",
            Self::Ui => "ui",
            Self::Golden => "golden",
            Self::Exec => "exec",
            Self::DebugMap => "debugmap",
            Self::Gc => "gc",
            Self::Lsp => "lsp",
        }
    }

    /// Returns the directory that holds the fixtures, relative to the repository root.
    #[must_use]
    pub const fn directory(self) -> &'static str {
        match self {
            Self::Parse => "tests/corpus/parse",
            Self::C11 => "tests/corpus/c11",
            Self::Ui => "tests/ui",
            Self::Golden => "tests/golden",
            Self::Exec => "tests/exec",
            Self::DebugMap => "tests/debugmap",
            Self::Gc => "tests/gc",
            Self::Lsp => "tests/lsp",
        }
    }

    /// Returns the extension of an input file for this kind.
    #[must_use]
    pub const fn input_extension(self) -> &'static str {
        match self {
            Self::C11 => "c",
            _ => "lark",
        }
    }

    /// Returns the extension of the expected output file, when the kind has one.
    ///
    /// A [`Kind::Ui`] fixture carries its expectations inline, so it has none.
    /// A [`Kind::C11`] fixture expects no diagnostic, so it has none.
    #[must_use]
    pub const fn expected_extension(self) -> Option<&'static str> {
        match self {
            Self::Parse => Some("tree"),
            Self::Golden => Some("expected.c"),
            Self::Exec | Self::Gc => Some("expected.out"),
            Self::DebugMap => Some("expected.map"),
            Self::Lsp => Some("expected.lsp"),
            Self::C11 | Self::Ui => None,
        }
    }

    /// Reports whether the kind builds and runs a binary.
    #[must_use]
    pub const fn runs_a_binary(self) -> bool {
        matches!(self, Self::Exec | Self::Gc)
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One fixture on disk.
#[derive(Clone, Debug)]
pub struct Fixture {
    /// The kind of test to run.
    pub kind: Kind,
    /// The input file.
    pub input: PathBuf,
    /// The expected output file, when the kind has one. It does not have to exist.
    pub expected: Option<PathBuf>,
    /// The name that a test report prints, such as `ui/gc_cast`.
    pub name: String,
    /// The collector capabilities that the fixture needs. See rule R-1.
    ///
    /// A header line `// needs: interior-pointers` puts one here. A collector
    /// that lacks it does not run the fixture.
    pub needs: Vec<String>,
}

/// Reads the `needs:` line from the head of a fixture.
///
/// The scan stops at the first line that is not a comment, so a `needs:` line
/// deep inside a program is text rather than a directive.
#[must_use]
pub fn needs_of(text: &str) -> Vec<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix("//").or_else(|| line.strip_prefix("/*")) else {
            break;
        };
        let rest = rest.trim().trim_start_matches('*').trim();
        if let Some(list) = rest.strip_prefix("needs:") {
            return list
                .split(',')
                .map(|item| item.trim().to_owned())
                .filter(|item| !item.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Finds every fixture of one kind under the repository root.
///
/// The search walks subdirectories, so a fixture can sit in a group folder.
///
/// # Errors
///
/// Returns an error when a directory exists but cannot be read.
pub fn discover(root: &Path, kind: Kind) -> io::Result<Vec<Fixture>> {
    let directory = root.join(kind.directory());
    let mut fixtures = Vec::new();
    if !directory.is_dir() {
        return Ok(fixtures);
    }
    walk(&directory, &mut |path| {
        if path.extension() != Some(OsStr::new(kind.input_extension())) {
            return;
        }
        // A file under `modules/` is a library that a fixture imports, not a
        // fixture of its own. The runner puts that folder on the search path.
        //
        // Rule N-16 puts a nested namespace in a subdirectory, so the check
        // reads every ancestor rather than the immediate parent alone.
        if path
            .ancestors()
            .filter_map(Path::file_name)
            .any(|name| name == OsStr::new("modules"))
        {
            return;
        }
        let relative = path.strip_prefix(&directory).unwrap_or(path);
        let stem = relative.with_extension("");
        let name = format!("{kind}/{}", stem.display());
        let expected = kind
            .expected_extension()
            .map(|extension| path.with_extension("").with_extension(extension));
        let needs = std::fs::read_to_string(path)
            .map(|text| needs_of(&text))
            .unwrap_or_default();
        fixtures.push(Fixture {
            kind,
            input: path.to_path_buf(),
            expected,
            name,
            needs,
        });
    })?;
    fixtures.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(fixtures)
}

/// Finds every fixture of every kind.
///
/// # Errors
///
/// Returns an error when a directory exists but cannot be read.
pub fn discover_all(root: &Path) -> io::Result<Vec<Fixture>> {
    let mut all = Vec::new();
    for kind in KINDS {
        all.extend(discover(root, *kind)?);
    }
    Ok(all)
}

/// Calls `visit` for every file under `directory`, including subdirectories.
fn walk(directory: &Path, visit: &mut dyn FnMut(&Path)) -> io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<io::Result<Vec<_>>>()?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(&path, visit)?;
        } else {
            visit(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{KINDS, Kind};

    #[test]
    fn every_kind_has_a_distinct_name_and_directory() {
        let mut names: Vec<&str> = KINDS.iter().map(|kind| kind.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KINDS.len());

        let mut directories: Vec<&str> = KINDS.iter().map(|kind| kind.directory()).collect();
        directories.sort_unstable();
        directories.dedup();
        assert_eq!(directories.len(), KINDS.len());
    }

    #[test]
    fn the_kinds_that_carry_expectations_inline_have_no_expected_file() {
        assert_eq!(Kind::Ui.expected_extension(), None);
        assert_eq!(Kind::C11.expected_extension(), None);
        assert_eq!(Kind::Golden.expected_extension(), Some("expected.c"));
    }

    #[test]
    fn only_the_running_kinds_build_a_binary() {
        for kind in KINDS {
            let expected = matches!(kind, Kind::Exec | Kind::Gc);
            assert_eq!(kind.runs_a_binary(), expected, "{kind}");
        }
    }
}
