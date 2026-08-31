# Build Plan

Read [conventions.md](conventions.md) and [test-strategy.md](test-strategy.md)
first. The conventions bind every change. The harness comes before the compiler.

Every phase here is done. The tools around the language live in
[toolchain-plan.md](toolchain-plan.md).

## 1. Workspace layout

```
lark/
  Cargo.toml                 workspace root
  lark.toml                  configuration for the example programs
  scripts/                   the gate from conventions.md section 6
  .github/workflows/ci.yml   the gate, on every push
  crates/
    lark-span/               source files, byte spans, line maps. No dependencies.
    lark-diag/               the LK#### catalogue, rendering, the --bless writer
    lark-syntax/             tokens, lossless tree, error recovering parser
    lark-cpp/                cc -E driver, position mapping back to the source
    lark-resolve/            two pass symbol table, module graph, name oracle
    lark-types/              type representation, lowering, inference, checking
    lark-mono/               monomorphization, layout selection for generics
    lark-codegen/            the C emitter, mangling, #line
    lark-driver/             lark.toml, the pipeline, the cc invocation
    lark-cli/                the `lark` binary: build, check, emit
    lark-lsp/                the language server
    lark-test/               the harness for T2 through T7 and T10
  runtime/
    include/lark_rt.h        the public runtime header
    core/                    shadow stack, safepoints, threads, transitions
    gc-marksweep/            the version 1 collector
    gc-conservative/         later
    tests/                   T8, plain C
  tests/                     the fixture tree from the test strategy
  examples/
    tour.lark                the reference example
```

**Crate rule.** `lark-syntax` never depends on `lark-types`. The parser must run
on a file with unresolved names, because the LSP needs a tree from broken code.
Rule L-6 needs the name table, so the parser takes a resolver callback instead of
a dependency. The callback is the `NameOracle` trait, and phase 1 already uses
it.

**Crate note.** The lexer lives inside `lark-syntax`, not in a crate of its own.
See decision D046.

## 2. Phases

Each phase states its exit tests. A phase ends when its tests pass and the
uncovered rule list from P-6 does not grow.

### Phase 0 - Harness and gates [DONE]
Deliver the workspace, `lark-span`, `lark-diag`, and `lark-test`. Every test type
runs against a stub, with one trivial fixture each. `--bless` works.

Deliver the gate from `docs/conventions.md` section 6, and the continuous
integration job that runs it.

**Exit:** `./scripts/check.sh` passes. `cargo test` runs all ten test types and
reports. The LK catalogue in `lark-diag` matches chapter 12, checked by a test.

**Result.** The gate passes. 82 tests run. The catalogue holds 35 codes and
matches chapter 12. The rule coverage scan finds 150 rules, of which 2 have a
test. `tests/rule-coverage-baseline.txt` records the rest.

### Phase 1 - Lexer and parser [DONE]
Deliver `lark-syntax`. Cover the whole Lark surface and the C subset that Lark
needs. Error recovery always produces a tree.

**Exit:** T2 green, including invariant R on every fixture, malformed inputs
included. Rules L-1 through L-5 covered.

**Result.** The gate passes. 149 tests run. `examples/tour.lark` parses with no
error. Invariant R holds for every source file in the repository, at the token
level and at the tree level, and for every malformed sample. Rules L-1 through
L-7, L-13, L-14, and O-25 have tests. The uncovered rule list fell from 148 to
139.

### Phase 2 - Names and modules [DONE]
Deliver `lark-resolve`. Two pass resolution, the module graph, `@import`,
`export`, and rule L-6.

**Exit:** T4 green for LK0100, LK0600, LK0610, LK0611, LK0612. A parse corpus
that separates `a<b>(c)` from `swap<T>(&a,&b)` passes.

**Result.** The gate passes. 188 tests run. The resolver reads `examples/tour.lark`
and its import with no diagnostic. Rules L-8, L-15, N-1 through N-11, and I-6
have tests. The uncovered rule list fell from 139 to 127.

**Left for phase 3.** Rule L-16. Version 1 resolves rule L-6 at module scope. A
local variable that shadows a type name needs the scope tree that type checking
builds.

### Phase 3 - Types, no GC [DONE]
Deliver `lark-types` for the C type system, plain structs, functions, and `auto`.

**Exit:** T4 green for LK02xx. Rules T-9 through T-11 covered.

**Result.** The gate passes. 228 tests run. The tour example type checks with no
diagnostic. Rules T-1, T-1a, T-2, T-3, T-9, T-10, T-11, L-5, L-16, and DQ-4 have
tests. The uncovered rule list fell from 127 to 120.

**Also closed.** Rule L-16. The parser now carries a local scope stack, so a
local declaration hides an outer type of the same name.

### Phase 4 - Emitter, unmanaged subset [DONE]
Deliver `lark-codegen`, `lark-driver`, and `lark-cli`. A Lark program with no
managed type compiles and runs.

**Exit:** T5 and T6 green for the unmanaged corpus. T7 green. This is the first
milestone where a program runs.

**Result.** The gate passes. 248 tests run. `lark build` turns a `.lark` file
into a binary. The exec fixtures build and run four times each. The debug map
fixture proves rule X-3 end to end: a C compiler error names the `.lark` file
and line. Rules X-1 through X-5b, O-25a, and F-1 have tests. The uncovered rule
list fell from 120 to 116.

### Phase 5 - Runtime and collector [DONE]
Deliver `runtime/core` and `runtime/gc-marksweep`, in C, with no transpiler
involved.

**This phase runs in parallel with phases 1 through 4.** It shares no code with
them.

**Exit:** T8 green under AddressSanitizer. Allocation, the block map, interior
pointer resolution, mark, sweep, thread attach and detach, and the stop the
world protocol all covered.

**Result.** 31 cases and 288 checks pass, plain and under
UndefinedBehaviorSanitizer, with the warning set from rule C-3.3 and `-Werror`.
The collector is `precise-marksweep`. Both root modes work, and a test proves
that a program behaves the same under each. Rules M-4 through M-26, F-3, O-4,
and O-6 have tests. The uncovered rule list fell from 116 to 99.

**One environment limit.** AddressSanitizer cannot map its shadow memory in the
sandbox that development runs in, and a trivial program hangs before `main`. The
`check-asan` target probes first, with a time limit, and skips with a loud
message. Continuous integration sets `LARK_REQUIRE_ASAN=1`, which turns the skip
into a failure.

### Phase 6 - Managed memory [DONE]
Deliver the `gc` qualifier, `managed struct`, `new`, the placement rules, the
object header, the field map, the shadow stack codegen, and the safepoint polls.

**Exit:** T4 green for LK03xx and LK0400. T6 green in all four combinations from
P-3 and P-4. T9 green. This is the milestone where the language does its job.

**Result.** The gate passes. 278 Rust tests and 32 runtime cases run. A managed
Lark program compiles, links the runtime, allocates, collects, and prints the
right answer under all four configurations. Rules M-1 through M-3, M-5, M-10 to
M-12, M-16, M-22, M-27, O-2, O-4, O-6, T-5, T-8, X-8, and I-3 have tests. The
uncovered rule list fell from 99 to 90.

**Pulled forward from phase 9.** Rule I-3 puts the runtime startup in the `init`
function. A program that allocates cannot run without it, so phase 6 delivers
it. Phase 9 still delivers `@global` and `@init` ordering.

**Left for later.** LK0320 needs a collector that forbids an interior pointer,
and the version 1 collector allows them. LK0330 needs proof that a `longjmp`
crosses a frame with a managed local, which needs a control flow pass.

### Phase 7 - Interfaces [DONE]
Deliver `iface`, `impl`, method tables, static dispatch, dynamic dispatch, fat
pointers, and the receiver adaptation table.

**Exit:** T4 green for LK04xx. T5 shows a direct call for a concrete receiver,
which proves rule O-19.

**Result.** The gate passes. 301 Rust tests and 33 runtime cases run. An
interface program builds, dispatches directly on a concrete type and through a
method table on a fat pointer, and runs under all four configurations. The
golden fixture shows `lk_m__Greet__Person__by_value(*p)`, which is rule O-19 and
rule O-18 together. Rules O-9 through O-24, T-12, and O-23 have tests. The
uncovered rule list fell from 90 to 75.

**Left for later.** Rule O-23, the checked cast from an interface value back to
a concrete type. The runtime carries `lark_itable_find` and a test for it, and
the syntax lands with the rest of the cast work.

### Phase 8 - Generics [DONE]
Deliver `lark-mono`, monomorphization, mangling, and the conditional `managed`
rule.

**Exit:** T4 green for LK05xx. A layout test proves that `Box<int>` carries no
header and `Box<gc Person*>` carries one.

**Result.** The gate passes. 323 Rust tests and 33 runtime cases run. A generic
program builds and runs under all four configurations. The golden fixture shows
`lk_generics__Box__G6Person__ti` with a field map, and `lk_generics__Box__i`
with none, which is rule G-10. Rules G-1 through G-13 and X-5a have tests. The
uncovered rule list fell from 75 to 65.

**Scope change.** Rule G-6 promised inference of a type argument. Version 1 asks
for the list instead, and rule G-6a states it. Inference needs the type of every
call argument, which needs the full expression type checker. Diagnostic LK0501
names the list to write, so the cost is one edit.

### Phase 9 - Initialization [DONE]
Deliver `init`, `@global`, `@init`, and the ordering rules.

**Exit:** T4 green for LK07xx. Execution tests prove the order from chapter 07
section 4.

**Result.** The gate passes. 334 Rust tests and 33 runtime cases run.
`examples/tour.lark` compiles, links, and runs under every configuration. That
is the author's original demo, end to end. Rules I-1 through I-17 have tests.
The uncovered rule list fell from 65 to 52.

**Also delivered.** The emitter now writes the type that `auto` infers, because
C11 has no inference. Rule T-10 gives the type, and the method call resolver
uses it too.

### Phase 10 - Foreign calls [DONE]
Deliver `gc_leaf`, `gc_safe`, the state transitions, and the thread rules.

**Exit:** T4 green for LK0340. A multithread execution test with a foreign call
passes under torture mode.

**Result.** The gate passes. 344 Rust tests and 34 runtime cases run. Two worker
threads allocate while the main thread collects, in all four configurations. The
golden fixture shows a transition around an unmarked extern and around a
`gc_safe` call, and none around a `gc_leaf` call. Rules M-19 through M-26 have
tests. The uncovered rule list fell from 52 to 51.

**Also delivered.** `examples/threads.lark` and `examples/pthread.lark` declare
the host facilities that a thread needs. Rule M-23 gives Lark no thread syntax,
and these show that none is needed.

### Phase 11 - Language server [DONE]
Deliver `lark-lsp`. Completion, hover, go to definition, and diagnostics.

**Exit:** T10 green.

**Result.** The gate passes. 363 Rust tests and 34 runtime cases run. The server
offers the fields and the methods of a receiver, the exported names of an
imported module, and the names in scope. It reports what a name is and where it
is declared. Every answer works on code that does not parse, which the crate
rule from phase 0 promised and invariant R makes true.

**The crate has two parts.** `Analysis` answers a question about a position and
knows nothing about the protocol, so a test asks it directly. The `server`
module speaks the protocol and asks `Analysis`.

At this point Lark is a working language. The superset promise is not yet
complete.

### Phase 12 - Superset phase B [DONE]
Parse full C11 declarations from preprocessed headers. `#include <stdio.h>`
type checks.

**Exit:** T3 green for a declaration corpus. `printf` resolves through the
header rather than through `stdio.lark`.

**Result:** `lark-cpp` runs `cc -E -dD` over the `#include` set of a module and
collects the type names, the value names, and the macro names. `lark-resolve`
takes a `HeaderReader`, so the analysis crates still run no process. A module
whose headers all read has a complete name table, which is what rule L-15 tests.

Sixty system headers parse with no error, from `stdio.h` to `pthread.h` and
`regex.h`. Reading them needed rule C-4a for the five places a real header puts
an attribute, rule C-4b for the reserved keyword spellings, rule C-4c for a
block pointer, and the C11 array qualifier that rule S-1 always required.

Six fixtures cover the declaration corpus: `declarations.c`, `typedefs.c`,
`records.c`, `qualifiers.c`, `pointers.c`, and `headers.c`. The proof that a
header really reaches the parser is that `size_t n = (size_t) 0;` parses with
`#include <stddef.h>` and fails without it. Rule L-6 cannot tell a cast from a
product without the name.

### Phase 13 - Superset phase C [DONE]
Parse full C11 statements and expressions.

**Exit:** T3 green for a statement and expression corpus. A `.c` file compiles
unchanged as a `.lark` file. Rule S-1 holds completely.

**Result:** Statements and operators already worked, because phase 1 built them
for Lark. Five standard constructs were missing: `_Generic`, `_Static_assert` as
a declaration, an old style function definition, a type name as a call argument,
and a qualifier inside an array parameter. Seven extensions joined them under
rule C-4, from a statement expression to a computed label.

Four fixtures cover the corpus: `statements.c`, `expressions.c`, `generic.c`,
and `oldstyle.c`. The exec fixture `plain_c.lark` is a C11 program with a macro,
an old style definition, a generic selection, and a range designator. It builds
and runs in all four configurations.

The measure that mattered was real code. The Lark C runtime, 2,366 lines, checks
clean as Lark. So does the gumbo HTML parser, 32,979 lines of third party C11.
Every one of its seventeen files emits C that compiles, and the library built
that way links against an unchanged driver and prints what the same library
built by `cc` prints.

Getting there needed four fixes beyond the parser. A `#define` with a carriage
return before its newline ended early. A macro whose replacement is a type, such
as `bool`, bound to a value. Rule X-5b made every public function `static`. The
forward declarations came before the `#include` lines and before the local
typedefs that they name.

## 3. Order and parallelism

```
Phase 0
   |
   +-- Phase 1 -- Phase 2 -- Phase 3 -- Phase 4 -- Phase 6 -- Phase 7 -- Phase 8 -- Phase 9 -- Phase 10 -- Phase 11 -- Phase 12 -- Phase 13
   |                                                  |
   +-- Phase 5 ---------------------------------------+
```

Phase 6 needs both branches. Everything after phase 6 is a single line of work.

## 4. Milestones

| Milestone | After |
|---|---|
| The parser holds invariant R | Phase 1 |
| Names and modules resolve | Phase 2 |
| Declarations have types | Phase 3 |
| A Lark program runs | Phase 4 |
| The collector works standalone | Phase 5 |
| A garbage collected program runs | Phase 6 |
| The tour example runs | Phase 9 |
| The language is complete | Phase 11 |
| The superset promise holds | Phase 13 |

## 5. Risks

**R1. The C11 front end is the largest component.** Phases 12 and 13 carry most
of the total work. The phase order puts them last on purpose, so the language
proves itself before the compatibility work starts.

**R2. Header extensions are an open ended chase.** Rule C-6 makes an unknown
extension a warning, not an error. A header never stops a build.

**R3. Shadow stack correctness is easy to get wrong and hard to see.** Torture
mode and the two mode agreement property from P-3 and P-4 are the defense. Both
exist from phase 0.

**R4. The parser serves two masters, batch compilation and the LSP.** The crate
rule in section 1 and invariant R in the test strategy hold the design in place.
