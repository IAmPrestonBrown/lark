//! The front end interface, and the front end that the harness drives.
//!
//! [`FrontEnd`] runs the real parts of the compiler and stands in for the
//! parts that later phases deliver. Phase 1 gives it a real lexer and parser.
//! The emitted C is still a stand in, driven by directives in the fixture.

use std::path::PathBuf;

use lark_codegen::Options;
use lark_diag::{Code, Diagnostic, Diagnostics};
use lark_resolve::{FileLoader, resolve_with};
use lark_span::{SourceId, SourceMap, Span};

use crate::annotation;

/// The stack root mechanism, from `gc.roots` in `lark.toml`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Roots {
    /// Precise roots from a shadow stack.
    ShadowStack,
    /// Roots from a scan of the machine stack.
    Conservative,
}

impl Roots {
    /// Returns the value that `lark.toml` uses.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ShadowStack => "shadow-stack",
            Self::Conservative => "conservative",
        }
    }
}

/// The collector that a run links. See chapter 10 section 4.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Collector {
    /// Precise, non moving, mark and sweep.
    PreciseMarkSweep,
    /// Allocate only. It frees nothing.
    Arena,
    /// Moving, copying, two space.
    Semispace,
    /// Moving, generational, with a write barrier.
    Generational,
}

impl Collector {
    /// Reports whether the collector follows an interior pointer. Rule M-8.
    pub const fn interior_pointers(self) -> bool {
        !self.moving()
    }

    /// Reports whether a collection frees what nothing reaches.
    pub const fn reclaims(self) -> bool {
        !matches!(self, Self::Arena)
    }

    /// Reports whether the collector meets what a fixture asks for.
    ///
    /// A fixture states a need in its header, as `// needs: interior-pointers`.
    /// A collector that lacks the capability skips the fixture, exactly as the
    /// runtime tests skip a case that does not apply. Rule R-1 gives the same
    /// answer to the transpiler at build time.
    pub fn meets(self, needs: &[String]) -> bool {
        needs.iter().all(|need| match need.as_str() {
            "interior-pointers" => self.interior_pointers(),
            "reclaims" => self.reclaims(),
            "moving" => self.moving(),
            // A need that no capability answers to holds back nothing. The
            // fixture harness reports an unknown need as a failure instead.
            _ => true,
        })
    }

    /// Returns the value that `gc.strategy` uses.
    pub const fn name(self) -> &'static str {
        match self {
            Self::PreciseMarkSweep => "precise-marksweep",
            Self::Arena => "arena",
            Self::Semispace => "semispace",
            Self::Generational => "generational",
        }
    }

    /// Reports whether the collector moves an object during a collection.
    ///
    /// A moving collector must write a new address into every root, and a
    /// rule M-13 conservative scan cannot say which words are roots. So it
    /// accepts rule M-10 shadow stack roots alone.
    pub const fn moving(self) -> bool {
        matches!(self, Self::Semispace | Self::Generational)
    }

    /// Reports whether the collector accepts a root mechanism.
    pub const fn accepts(self, roots: Roots) -> bool {
        match roots {
            Roots::ShadowStack => true,
            Roots::Conservative => !self.moving(),
        }
    }
}

/// The build settings for one compiler run.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Config {
    /// The stack root mechanism.
    pub roots: Roots,
    /// Whether every safepoint runs a full collection. See rule F-3.
    pub torture: bool,
    /// The collector that the run links.
    pub collector: Collector,
}

impl Config {
    /// Returns the four combinations that principles P-3 and P-4 require.
    ///
    /// Each one links the default collector. `full_matrix` adds the others.
    pub const fn matrix() -> [Self; 4] {
        [
            Self {
                roots: Roots::ShadowStack,
                torture: false,
                collector: Collector::PreciseMarkSweep,
            },
            Self {
                roots: Roots::ShadowStack,
                torture: true,
                collector: Collector::PreciseMarkSweep,
            },
            Self {
                roots: Roots::Conservative,
                torture: false,
                collector: Collector::PreciseMarkSweep,
            },
            Self {
                roots: Roots::Conservative,
                torture: true,
                collector: Collector::PreciseMarkSweep,
            },
        ]
    }

    /// Returns every combination of collector, root mechanism, and torture.
    ///
    /// A combination that the collector refuses is left out, so every entry
    /// names a build that can run. Principle P-3 asks for the whole matrix,
    /// and a collector is one more axis of it.
    pub fn full_matrix() -> Vec<Self> {
        let mut all = Vec::new();
        for collector in [
            Collector::PreciseMarkSweep,
            Collector::Arena,
            Collector::Semispace,
            Collector::Generational,
        ] {
            for roots in [Roots::ShadowStack, Roots::Conservative] {
                if !collector.accepts(roots) {
                    continue;
                }
                for torture in [false, true] {
                    all.push(Self {
                        roots,
                        torture,
                        collector,
                    });
                }
            }
        }
        all
    }

    /// Returns the suffix that a test name carries for this configuration.
    pub fn suffix(self) -> String {
        let torture = if self.torture { "torture" } else { "normal" };
        if self.collector == Collector::PreciseMarkSweep {
            return format!("{}+{torture}", self.roots.name());
        }
        format!("{}+{}+{torture}", self.collector.name(), self.roots.name())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: Roots::ShadowStack,
            torture: false,
            collector: Collector::PreciseMarkSweep,
        }
    }
}

/// Adds the two files that one module emits.
///
/// Rule X-4b names the header, so every caller asks for the name rather than
/// building it.
fn push_files(files: &mut Vec<(String, String)>, module: &str, emitted: &lark_codegen::Emitted) {
    files.push((
        lark_codegen::names::header_file(module),
        emitted.header.clone(),
    ));
    files.push((format!("{module}.c"), emitted.c.clone()));
}

/// One compiler run.
#[derive(Clone, Debug)]
pub struct Input {
    /// The file to compile.
    pub path: PathBuf,
    /// The text of the file.
    pub text: String,
    /// The build settings.
    pub config: Config,
    /// Directories that `@import` searches after the file's own. See rule N-3.
    pub search: Vec<PathBuf>,
    /// Whether the fixture becomes a program. Rule I-1 needs one.
    pub is_program: bool,
    /// The cursor that a language server question asks about.
    pub cursor: Option<Cursor>,
}

/// Where a language server question points, and what it asks.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    /// The byte offset, after the marker is removed.
    pub offset: u32,
    /// What the fixture asks for.
    pub query: lark_lsp::Query,
}

/// What one compiler run produced.
#[derive(Debug)]
pub struct Output {
    /// Every source file that the run read.
    pub sources: SourceMap,
    /// Every diagnostic that the run reported.
    pub diagnostics: Diagnostics,
    /// The syntax tree, printed for a snapshot. See test type T2.
    pub tree: Option<String>,
    /// The source text printed back from the tree. See invariant R.
    pub roundtrip: Option<String>,
    /// The emitted C for the root module. See test types T5 and T6.
    pub c: Option<String>,
    /// Every emitted file, as a name and its text. See rule X-4.
    pub files: Vec<(String, String)>,
    /// Whether the program needs the runtime library.
    pub uses_runtime: bool,
    /// The line map from the emitted C back to the source. See test type T7.
    pub debug_map: Option<String>,
    /// The language server answer at the cursor. See test type T10.
    pub lsp: Option<String>,
}

/// A front end that the harness can drive.
///
/// Phase 0 ships [`StubCompiler`]. Later phases replace it with the driver.
pub trait Compile: Send + Sync {
    /// Runs the front end over one input.
    fn compile(&self, input: &Input) -> Output;
}

/// Every diagnostic code that the real front end can produce today.
///
/// A fixture that annotates one of these gets the real diagnostic. A fixture
/// that annotates any other code gets a stand in, until the phase that
/// produces it lands. The list grows as phases land.
const REAL_CODES: &[Code] = &[
    // Phase 1. The lexer and the parser.
    lark_diag::LK0102,
    lark_diag::LK0103,
    lark_diag::LK0104,
    lark_diag::LK0105,
    lark_diag::LK0110,
    // Phase 2. Names and modules.
    lark_diag::LK0100,
    lark_diag::LK0600,
    lark_diag::LK0610,
    lark_diag::LK0611,
    lark_diag::LK0612,
    lark_diag::LK0613,
    // Phase 3. Types.
    lark_diag::LK0200,
    lark_diag::LK0210,
    lark_diag::LK0211,
    // Phase 6. Managed memory.
    lark_diag::LK0301,
    lark_diag::LK0310,
    lark_diag::LK0311,
    lark_diag::LK0340,
    lark_diag::LK0400,
    // Phase 7. Interfaces.
    lark_diag::LK0410,
    lark_diag::LK0411,
    lark_diag::LK0412,
    lark_diag::LK0413,
    lark_diag::LK0420,
    lark_diag::LK0421,
    lark_diag::LK0430,
    // Phase 8. Generics.
    lark_diag::LK0500,
    lark_diag::LK0501,
    lark_diag::LK0502,
    // Phase 9. Initialization.
    lark_diag::LK0700,
    lark_diag::LK0701,
    lark_diag::LK0710,
    lark_diag::LK0711,
];

/// The front end that the harness drives.
///
/// It runs the real lexer, the real parser, the real resolver, and the real
/// type checks. It stands in for the parts that later phases deliver, using
/// directives in the fixture.
///
/// | Directive | Effect |
/// |---|---|
/// | `//~ ERROR LK0800` | A stand in diagnostic, for a code no phase produces yet. |
/// | `// lsp: completion` | The fixture asks the language server at `<|>`. |
#[derive(Clone, Copy, Debug, Default)]
pub struct FrontEnd;

impl Compile for FrontEnd {
    fn compile(&self, input: &Input) -> Output {
        let name = input.path.file_stem().map_or_else(
            || "main".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );

        // The loader searches the directory of the fixture first, then the
        // shared modules. See rule N-3.
        let loader = FileLoader::new(input.search.clone());
        // Rule C-1. A fixture that includes a system header needs the real
        // preprocessor, the same one that a build uses.
        let reader = lark_cpp::Reader::new(lark_cpp::Options {
            include_dirs: input.search.clone(),
            ..lark_cpp::Options::default()
        });
        let resolution = resolve_with(&loader, &reader, &name, &input.path, &input.text);

        let Some(root) = resolution.root.and_then(|id| resolution.graph.get(id)) else {
            return Output {
                sources: resolution.sources,
                diagnostics: resolution.diagnostics,
                tree: None,
                roundtrip: None,
                c: None,
                files: Vec::new(),
                uses_runtime: false,
                debug_map: None,
                lsp: None,
            };
        };

        let mut types = lark_types::check_resolution(&resolution);
        if input.is_program {
            types.extend(lark_types::check_program(&resolution));
        }
        let mut mono_diagnostics = Diagnostics::new();
        let program = lark_mono::collect(&resolution.graph, &mut mono_diagnostics);

        // A fixture states what its own file reports. A diagnostic in an
        // imported file belongs to that file's fixture.
        let mut diagnostics = Diagnostics::new();
        for item in resolution
            .diagnostics
            .items()
            .iter()
            .chain(types.items())
            .chain(mono_diagnostics.items())
        {
            if item.primary.file == root.source {
                diagnostics.push(item.clone());
            }
        }
        stand_in_diagnostics(
            &mut diagnostics,
            &resolution.sources,
            root.source,
            &input.text,
        );
        diagnostics.sort_by_position();

        let tree = root.parse.tree_text();
        let roundtrip = root.parse.text();
        let root_id = root.id;
        let source_name = input
            .path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());

        // The emitter runs only when the earlier passes accepted the program.
        let mut files = Vec::new();
        let mut c = None;
        let mut debug_map = None;
        let mut uses_runtime = false;
        if !diagnostics.has_errors() {
            for module in resolution.graph.modules() {
                // A snapshot must hold no machine path, so a `#line` directive
                // names the file rather than its location.
                let options = Options {
                    source_name: Some(format!("{}.lark", module.name)),
                    roots: match input.config.roots {
                        Roots::ShadowStack => lark_codegen::Roots::ShadowStack,
                        Roots::Conservative => lark_codegen::Roots::Conservative,
                    },
                    torture: input.config.torture,
                    ..Options::default()
                };
                let Some(emitted) =
                    lark_codegen::emit(&resolution.graph, module.id, &options, &program)
                else {
                    continue;
                };
                uses_runtime |= emitted.uses_runtime;
                push_files(&mut files, &module.name, &emitted);
                if module.id == root_id {
                    debug_map = Some(emitted.line_map_text(&source_name));
                    c = Some(emitted.c.clone());
                }
            }
        }

        Output {
            sources: resolution.sources,
            diagnostics,
            tree: Some(tree),
            roundtrip: Some(roundtrip),
            c,
            files,
            uses_runtime,
            debug_map,
            lsp: input.cursor.map(|cursor| {
                // The language server answers about a position, and it works on
                // broken code because the parser always produces a tree.
                let analysis =
                    lark_lsp::Analysis::new(&name, &input.path, &input.text, &input.search);
                analysis.report(cursor.query, cursor.offset)
            }),
        }
    }
}

/// Adds a stand in diagnostic for every annotation that names a future code.
///
/// A fixture can therefore test a rule before the phase that enforces it. When
/// that phase lands, its code joins [`REAL_CODES`] and the stand in stops.
fn stand_in_diagnostics(
    diagnostics: &mut Diagnostics,
    sources: &SourceMap,
    id: SourceId,
    text: &str,
) {
    let file = sources.file(id);
    for expectation in annotation::parse(text).expected {
        if REAL_CODES.contains(&expectation.code) {
            continue;
        }
        let Some(span) = file.line_span(expectation.line) else {
            continue;
        };
        // Drop the line ending, so a caret does not cover it.
        let trimmed = if span.len() > 1 {
            Span::new(span.start, span.end - 1)
        } else {
            span
        };
        diagnostics.push(Diagnostic::new(expectation.code, id, trimmed));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Collector, Compile, Config, FrontEnd, Input, Roots};

    fn run(text: &str) -> super::Output {
        FrontEnd.compile(&Input {
            path: PathBuf::from("fixture.lark"),
            text: text.to_owned(),
            config: Config::default(),
            search: Vec::new(),
            is_program: false,
            cursor: None,
        })
    }

    #[test]
    fn the_matrix_holds_four_distinct_configurations() {
        let matrix = Config::matrix();
        let mut names: Vec<String> = matrix.iter().map(|config| config.suffix()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn a_configuration_suffix_names_both_settings() {
        let config = Config {
            roots: Roots::Conservative,
            torture: true,
            collector: Collector::PreciseMarkSweep,
        };
        assert_eq!(config.suffix(), "conservative+torture");
    }

    #[test]
    fn a_suffix_names_a_collector_that_is_not_the_default() {
        let config = Config {
            roots: Roots::ShadowStack,
            torture: false,
            collector: Collector::Semispace,
        };
        assert_eq!(config.suffix(), "semispace+shadow-stack+normal");
    }

    #[test]
    fn the_full_matrix_leaves_out_a_combination_that_cannot_run() {
        let all = Config::full_matrix();
        // A moving collector accepts shadow stack roots alone, so it appears
        // twice rather than four times. Two of the four move.
        assert_eq!(all.len(), 12);
        assert!(
            !all.iter()
                .any(|item| item.collector.moving() && item.roots == Roots::Conservative)
        );
    }

    #[test]
    fn a_collector_meets_only_what_it_supports() {
        let needs = vec!["interior-pointers".to_owned()];
        assert!(Collector::PreciseMarkSweep.meets(&needs));
        assert!(Collector::Arena.meets(&needs));
        assert!(!Collector::Semispace.meets(&needs));

        let reclaims = vec!["reclaims".to_owned()];
        assert!(Collector::PreciseMarkSweep.meets(&reclaims));
        assert!(!Collector::Arena.meets(&reclaims));
        assert!(Collector::Semispace.meets(&reclaims));
        assert!(Collector::Generational.meets(&reclaims));
    }

    #[test]
    fn a_stand_in_diagnostic_lands_on_the_annotated_line() {
        // LK0800 belongs to phase 12, so the stand in still supplies it.
        let output = run("init void f(void) {\n    x;   //~ ERROR LK0800\n}\n");
        let Some(diagnostic) = output
            .diagnostics
            .items()
            .iter()
            .find(|item| item.code == lark_diag::LK0800)
        else {
            panic!(
                "the stand in must supply LK0800: {:?}",
                output.diagnostics.items()
            );
        };
        let file = output.sources.file(diagnostic.primary.file);
        assert_eq!(file.line_col(diagnostic.primary.span.start).line, 2);
    }

    /// covers: L-13
    #[test]
    fn the_front_end_holds_invariant_r() {
        let text = "int main(void) { return 0; }\n";
        let output = run(text);
        assert_eq!(output.roundtrip.as_deref(), Some(text));
    }

    #[test]
    fn a_real_parser_diagnostic_reaches_the_output() {
        let output = run("int x = ;\n");
        assert!(
            output
                .diagnostics
                .items()
                .iter()
                .any(|item| item.code == lark_diag::LK0110),
            "{:?}",
            output.diagnostics.items()
        );
    }

    #[test]
    fn the_emitter_keeps_the_source_shape() {
        let output = run("// a comment\nint main(void) { return 0; }\n");
        let c = output.c.unwrap_or_default();
        assert!(c.contains("// a comment"), "{c}");
        assert!(c.contains("int main(void) { return 0; }"), "{c}");
    }

    /// covers: X-3
    #[test]
    fn the_emitter_writes_a_line_directive_for_each_item() {
        let output = run("int a;\nint b;\n");
        let map = output.debug_map.unwrap_or_default();
        assert!(map.contains("fixture.lark:1 -> c:"), "{map}");
        assert!(map.contains("fixture.lark:2 -> c:"), "{map}");
    }

    /// covers: X-4
    #[test]
    fn the_emitter_writes_one_c_file_and_one_header_per_module() {
        let output = run("export int f(void) { return 0; }\n");
        let names: Vec<&str> = output.files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["fixture.lark.h", "fixture.c"]);
    }
}
