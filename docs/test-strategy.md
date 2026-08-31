# Test Strategy

The harness comes before the compiler. Phase 0 of the build plan delivers every
test type below, each with one trivial fixture, against a stub that does
nothing. A test type that does not exist before the code exists never gets
written afterward.

## 1. Principles

**P-1.** A test asserts a diagnostic **code**, never a message text. A message
improves without a change to a test.

**P-2.** Every generated artifact test supports bless mode, which rewrites the
expected file from the actual output. Review of a blessed diff is the review
step, not manual transcription.

```
LARK_BLESS=1 cargo test
```

**P-3.** Every execution test runs twice, once under `roots = "shadow-stack"`
and once under `roots = "conservative"`. The two runs must produce identical
output. This one property finds most root set defects.

**P-4.** Every execution test that allocates also runs under `gc.torture`. A
correct program gives the same output. This finds a missing root, a missing
safepoint poll, and a missing write barrier.

**P-5.** Every execution test runs under AddressSanitizer and
UndefinedBehaviorSanitizer in continuous integration.

**P-7.** Every runtime test runs against every collector. A program links
exactly one, and each one satisfies the same seam, so the same suite proves all
of them. A collector supplies a different set of capabilities, and a test that
asks for one the collector lacks skips with the reason rather than failing. Rule
R-1 gives the transpiler the same answer at build time.

**P-8.** A collector test runs against every collector as well. A fixture under
`tests/gc` names what it needs in its header, and the harness leaves out every
configuration whose collector lacks it.

```c
// needs: interior-pointers
```

The whole matrix is ten runs: three collectors, two root mechanisms where the
collector accepts both, and torture on and off.

**P-9.** A property holds for every input, not for one. A generator builds
inputs from a seed, and the seed is written out, so a failure reproduces on any
machine. The corpus tests apply the same properties to every file that the
project already holds.

## 2. Test types

### T1 - Unit tests
Standard Rust tests inside each crate. They cover the lexer, the span math, the
name table, and the mangling scheme.

### T2 - Parser corpus, with a round trip invariant
Input: a `.lark` file. Output: an indented tree, compared to a snapshot.

**Invariant R.** The parser prints its tree back to text, and the result is byte
identical to the input. This holds for a malformed input as well, because the
parser recovers and keeps every token.

Invariant R is what makes the LSP possible. A parser that fails it cannot serve
a formatter, a rename, or an accurate hover.

```
tests/corpus/parse/generics_vs_comparison.lark
tests/corpus/parse/generics_vs_comparison.tree
```

### T3 - Superset conformance
Input: a valid C11 file. The test asserts that Lark parses it, reports no error,
and emits C with the same behavior.

```
tests/corpus/c11/**.c
```

This is the direct test of rule S-1. It stays small in phase A and grows through
phase C.

### T4 - Diagnostic tests
Input: a `.lark` file with an annotation on the line that must fail.

```c
void f(void) {
    handle_opaque_data(count);   //~ ERROR LK0301
    Bad<gc Person*> x;           //~ ERROR LK0400
}
```

The test asserts the code and the line. It asserts no message text. An
unannotated diagnostic fails the test, and a missing diagnostic fails the test.

### T5 - Golden C output
Input: a `.lark` file. Expected: a `.expected.c` file. The comparison
normalizes whitespace and absolute paths, and keeps everything else exact.

These tests are the fastest way to see the cost of a construct change. A diff
that adds a shadow stack push to a function that had none is a design
regression, and the diff shows it.

### T6 - Execution tests
Input: a `.lark` file and an expected standard output and exit code. The harness
transpiles, compiles with the configured C compiler, runs, and compares.

Every such test runs in the four combinations from P-3 and P-4.

### T7 - Debug mapping
The harness injects a C level error into the emitted output, compiles, and
asserts that the C compiler reports the original `.lark` file and line. This
tests rule X-3 directly.

### T8 - Runtime and collector tests
Plain C tests against the runtime library, with no transpiler involved. They
cover allocation, the block map, interior pointer resolution, the mark phase,
the sweep phase, thread attach and detach, and the stop the world protocol.

The runtime is testable before the transpiler emits a single line.

Every case runs against every collector, and three times for each: plain, under
UndefinedBehaviorSanitizer, and under AddressSanitizer. A sandbox can stop AddressSanitizer from mapping its shadow
memory, and the process then hangs before `main`. The `check-asan` target
therefore probes first, with a time limit, and skips with a loud message.
Continuous integration sets `LARK_REQUIRE_ASAN=1`, which turns the skip into a
failure.

### T9 - Collector stress
Lark programs that build large object graphs, drop them, and check liveness by
count. They run under torture mode, under both root mechanisms, and against
every collector that supports what the fixture needs. See principle P-8.

The shapes matter more than the size. A chain finds a marker that recurses. A
diamond finds a copy that runs twice. A ring finds a walk that does not stop. An
array finds a trace that reads the first element alone.


### T10 - LSP tests
A fixture with a cursor marker and a query directive.

```c
// lsp: completion
auto p = new Person { .name = "x", .age = 1 };
p.<|>
```

The harness removes the marker, records the offset, and compares the rendered
answer to a snapshot. The query is `completion`, `hover`, or `definition`.

One fixture holds code that does not parse, because that is the case a language
server meets most often.

### T11 - Property and corpus tests
A generator builds inputs from a seed, and each one must satisfy the invariants:
the text of the tree equals the source, joining the tokens gives the source
back, and the parser finishes. The same properties then run over every `.lark`,
`.c`, and `.h` file in the repository.

Two shapes matter beyond random text. Every prefix of a valid program is what an
editor holds while a person types. Every single byte deletion from a valid
program is the other common editor state.

## 3. Directory layout

```
tests/
  corpus/parse/      T2, with .tree snapshots
  corpus/c11/        T3, valid C11 inputs
  ui/                T4, with //~ ERROR annotations
  golden/            T5, with .expected.c
  exec/              T6, with .expected.out
  debugmap/          T7
  gc/                T9
  lsp/               T10
runtime/tests/       T8, plain C
```

## 4. Coverage rule

**P-6.** Every numbered rule in `docs/spec` maps to at least one test. A rule
with no test is an open task. The harness prints the uncovered rule list, and
continuous integration fails when the list grows.

A test claims a rule with a `covers:` marker. The marker sits in a comment, in
a Rust test file or in a fixture. It holds one rule, or several separated by
commas.

```rust
/// covers: M-11, M-12
#[test]
fn zeroes_every_slot_before_the_push() { ... }
```

`tests/rule-coverage-baseline.txt` lists the rules with no test. The list must
equal the scan exactly. A rule that gains a test leaves the list. A new rule
with no test joins it, and the build fails until someone accepts the change with
`LARK_BLESS=1`.

This is the mechanism that keeps the specification and the implementation
together. A rule that no test enforces is a rule that the implementation is free
to break.
