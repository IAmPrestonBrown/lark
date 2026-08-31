//! Properties that hold for every input, valid or not.
//!
//! A fixture proves one case. A property test builds many inputs from a seeded
//! generator and asserts the same thing about each one. The seed is written
//! out, so a failure reproduces on any machine.
//!
//! Test type T2 in docs/test-strategy.md.
//! covers: L-13, S-1

// A helper in a test file proves a failure by panicking. Rule C-2.3 bans a
// panic in library code, not in a test.
#![allow(clippy::panic)]

use lark_syntax::{NoNames, parse, tokenize};

/// A linear congruential generator, so the sequence is the same everywhere.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: usize) -> usize {
        // The value is a bucket index, so truncation on a small target picks a
        // different bucket rather than a wrong one.
        let bound = u64::try_from(bound).unwrap_or(1);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// The pieces that a generated input is built from.
///
/// The set mixes Lark keywords, C keywords, punctuation, and text that no
/// grammar accepts, so most inputs are invalid. The parser must still hold its
/// invariants for every one.
const PIECES: &[&str] = &[
    "int",
    "char",
    "void",
    "struct",
    "union",
    "enum",
    "typedef",
    "static",
    "const",
    "return",
    "if",
    "else",
    "while",
    "for",
    "switch",
    "case",
    "break",
    "goto",
    "sizeof",
    "_Generic",
    "_Static_assert",
    "managed",
    "gc",
    "iface",
    "impl",
    "export",
    "new",
    "auto",
    "init",
    "gc_leaf",
    "gc_safe",
    "@import",
    "@global",
    "Person",
    "value",
    "x",
    "y",
    "f",
    "T",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    ";",
    ",",
    ".",
    "->",
    "::",
    "<",
    ">",
    "*",
    "&",
    "=",
    "==",
    "+",
    "-",
    "/",
    "%",
    "?",
    ":",
    "...",
    "#include <stdio.h>",
    "\n",
    " ",
    "\t",
    "\"text\"",
    "'c'",
    "0",
    "42",
    "3.5",
    "0x1f",
    "/* comment */",
    "// line\n",
    "\\",
];

/// Builds one input from a seed.
fn generate(seed: u64, pieces: usize) -> String {
    let mut random = Random(seed);
    let mut out = String::new();
    for _ in 0..pieces {
        out.push_str(random.pick(PIECES));
        if random.below(3) == 0 {
            out.push(' ');
        }
    }
    out
}

/// Invariant R. The text of the tree equals the source, byte for byte.
///
/// The invariant is what makes the tree usable for a language server and for
/// a formatter. It must hold for an input that no grammar accepts, because a
/// file under an editor is invalid most of the time.
#[test]
fn the_tree_text_equals_the_source_for_every_input() {
    for seed in 0..400u64 {
        let source = generate(seed, 40);
        let parsed = parse(&source, &NoNames);
        assert_eq!(
            parsed.text(),
            source,
            "invariant R failed for seed {seed}\n{source:?}"
        );
    }
}

/// Rule L-13. Joining every token gives the source back.
#[test]
fn the_tokens_join_back_into_the_source() {
    for seed in 1_000..1_400u64 {
        let source = generate(seed, 40);
        let lexed = tokenize(&source);
        assert_eq!(
            lexed.join(&source),
            source,
            "rule L-13 failed for seed {seed}\n{source:?}"
        );
    }
}

/// The parser always finishes. A grammar with a loop that consumes nothing
/// hangs instead, and a fixture rarely finds one.
#[test]
fn the_parser_finishes_for_every_input() {
    for seed in 2_000..2_400u64 {
        let source = generate(seed, 60);
        // The call returns, or the test never ends and the harness times out.
        let parsed = parse(&source, &NoNames);
        assert_eq!(parsed.text(), source);
    }
}

/// A truncation of a valid file is what an editor holds most of the time.
/// Every prefix must parse without a panic and keep invariant R.
#[test]
fn every_prefix_of_a_program_keeps_the_invariant() {
    let program = "\
managed struct Person { gc char* name; int age; }
iface Greet { void say_hi(Self this); }
impl Greet for Person {
    void say_hi(Person this) { stdio::printf(\"%s\\n\", this.name); }
}
init int main(void) {
    auto p = new Person { .name = \"Ada\", .age = 36 };
    p.say_hi();
    for (int i = 0; i < 3; i++) { p.age = p.age + 1; }
    return 0;
}
";
    for end in 0..program.len() {
        if !program.is_char_boundary(end) {
            continue;
        }
        let source = &program[..end];
        let parsed = parse(source, &NoNames);
        assert_eq!(parsed.text(), source, "invariant R failed at length {end}");
    }
}

/// Deleting one byte from a valid file is the other common editor state.
#[test]
fn every_single_byte_deletion_keeps_the_invariant() {
    let program = "int f(int a) { return a + 1; }\nmanaged struct P { gc P* next; }\n";
    for index in 0..program.len() {
        if !program.is_char_boundary(index) || !program.is_char_boundary(index + 1) {
            continue;
        }
        let mut source = program.to_owned();
        source.remove(index);
        let parsed = parse(&source, &NoNames);
        assert_eq!(
            parsed.text(),
            source,
            "invariant R failed at deletion {index}"
        );
    }
}

/// A run of one character must not make the parser loop or overflow.
#[test]
fn a_long_run_of_one_token_terminates() {
    for piece in ["{", "}", "(", ")", "[", "]", "<", ">", "*", ";", "#", "\\"] {
        let source = piece.repeat(2_000);
        let parsed = parse(&source, &NoNames);
        assert_eq!(
            parsed.text(),
            source,
            "invariant R failed for a run of {piece}"
        );
    }
}

/// Deep nesting must not overflow the machine stack.
#[test]
fn deep_nesting_terminates() {
    for (open, close) in [("(", ")"), ("{", "}"), ("[", "]")] {
        let source = format!(
            "int f(void) {{ return {}1{}; }}",
            open.repeat(300),
            close.repeat(300)
        );
        let parsed = parse(&source, &NoNames);
        assert_eq!(parsed.text(), source);
    }
}

// ---------------------------------------------------------------------------
// The superset contract, over the whole keyword set.
// ---------------------------------------------------------------------------

/// Every Lark keyword is contextual, so a C program that uses one as a name
/// keeps its meaning. Rule S-2 promises that Lark adds no reserved word.
/// covers: S-2, L-3
#[test]
fn every_lark_keyword_works_as_an_ordinary_name() {
    // Every word that Lark gives a meaning somewhere.
    const WORDS: &[&str] = &[
        "gc", "managed", "iface", "impl", "export", "new", "init", "gc_leaf", "gc_safe", "Self",
    ];
    for word in WORDS {
        // As a variable, as a function, as a parameter, and as a field.
        let source = format!(
            "int {word} = 1;\n\
             int f_{word}(int {word}) {{ return {word}; }}\n\
             struct S_{word} {{ int {word}; }};\n\
             int g_{word}(void) {{ struct S_{word} s; s.{word} = {word}; return s.{word}; }}\n"
        );
        let parsed = parse(&source, &NoNames);
        assert!(
            parsed.errors().is_empty(),
            "`{word}` is not usable as a name\n{source}\n{:?}",
            parsed.errors()
        );
        assert_eq!(parsed.text(), source);
    }
}

/// `new` is a keyword only in expression position. A declaration that names a
/// type `new` still reads as a declaration.
/// covers: O-7, L-3
#[test]
fn new_is_a_keyword_only_in_expression_position() {
    // As a name, `new` declares a variable.
    let ordinary = parse("int new = 3;\nint f(void) { return new; }\n", &NoNames);
    assert!(ordinary.errors().is_empty(), "{:?}", ordinary.errors());

    // Before a type name in expression position, `new` allocates.
    let allocation = parse(
        "managed struct P { int a; }\nint f(void) { auto p = new P { .a = 1 }; return p->a; }\n",
        &NoNames,
    );
    assert!(allocation.errors().is_empty(), "{:?}", allocation.errors());
    assert!(allocation.tree_text().contains("NEW_EXPR"));
}

/// Lark has no `delete` and no finalizer, so both words stay ordinary names.
/// covers: O-8
#[test]
fn delete_and_finalizer_are_ordinary_names() {
    for word in ["delete", "finalize", "free", "weak"] {
        let source =
            format!("int {word}(int a) {{ return a; }}\nint f(void) {{ return {word}(1); }}\n");
        let parsed = parse(&source, &NoNames);
        assert!(
            parsed.errors().is_empty(),
            "`{word}` is not usable as a name\n{:?}",
            parsed.errors()
        );
    }
}

/// A `__builtin_*` name is an ordinary function name, not an extension that
/// the parser skips.
/// covers: C-5
#[test]
fn a_builtin_name_is_an_ordinary_function_name() {
    let source = "int f(double x) { return __builtin_isnan(x) + __builtin_expect(1, 0); }\n";
    let parsed = parse(source, &NoNames);
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
    // It reads as a call, so the name reaches the C compiler unchanged.
    assert!(parsed.tree_text().contains("__builtin_isnan"));
    assert!(parsed.tree_text().contains("CALL_EXPR"));
}

/// Pass one respects C scoping for the C subset, so a program that depends on
/// declaration order keeps its meaning.
/// covers: L-9
#[test]
fn a_local_name_hides_a_file_scope_name_of_the_same_kind() {
    // `count` is a type at file scope and a variable inside the function. The
    // cast inside the function must read the local, so `(count)` is not a type
    // name there and the file does not parse as a cast.
    let source = "typedef int count;\n\
                  int f(void) { int count = 3; return count + 1; }\n\
                  count global_value = 1;\n";
    let parsed = parse(source, &NoNames);
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());

    // A parameter name hides a file scope type in the same way.
    let parameter = "typedef int width;\nint g(int width) { return width * 2; }\n";
    let parsed = parse(parameter, &NoNames);
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
}
