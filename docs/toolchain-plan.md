# Toolchain Plan

The language is finished. Phases 0 through 13 of [build-plan.md](build-plan.md)
are done, and the gate passes. This plan covers the tools around the language.

Read [conventions.md](conventions.md) and [test-strategy.md](test-strategy.md)
first. Every rule in them binds every phase here. The harness comes before the
tool, and a phase ends when its tests pass and the uncovered rule list from
principle P-6 does not grow.

## 1. What each phase changes

| Phase | Name | Adds | Risk |
|---|---|---|---|
| T1 | Output naming | Nothing new. A fix. | Low |
| T2 | Package manager | `lark-pkg`, a git index format | Medium |
| T3 | Incremental build | `lark-cache`, a fingerprint | High |
| T4 | Formatter and debugger | `lark-fmt`, debug metadata | Medium |
| T5 | Generational collector | `gc-generational`, a write barrier | High |

The order is not arbitrary. T1 is a bug that T2 makes worse, because a package
brings foreign file names into a build. T2 gives T3 the dependency graph that an
incremental build needs. T3 gives T4 the fast rebuild that a formatter and a
debugger both want during a session. T5 stands alone and comes last, because the
seam already holds it and nothing else waits on it.

## 2. Phases

### Phase T1 - Output naming [DONE]

A module emitted `<name>.h`, and the emitted `.c` included it by that name. A
source file `attribute.c` with its own `attribute.h` collided: the generated
header shadowed the real one, and every type in it disappeared. The collision
was silent, because the compiler reported a missing type rather than a shadowed
file.

**Result.** Rule X-4b. A generated file name carries `.lark.`, so a module
`attribute` emits `attribute.lark.h` and a neighbouring `attribute.h` keeps its
own name. `lark_codegen::names::header_file` builds the name once, and the five
places that built it by hand now call it.

The compiler receives `-iquote` for the build directory and then for each
directory that holds a source module, so a header that a programmer wrote is
found under its own name whatever the module is called. The header reader had
the same gap: a module named by a bare relative path gave an empty parent, and
the reader then found no neighbouring header at all.

The execution harness reused a scratch directory and never cleared it. The
rename left the old generated header behind, and `-iquote` on that directory
found it first. A passing test hid a real failure, so the harness now starts
from an empty directory. See decision D118.

The gumbo library, 32,979 lines, now builds under its own file names with no
renaming, links, and prints what `cc` prints.

### Phase T2 - Package manager [DONE]

A program that wants a library today copies the source into its own tree. The
manager fetches it instead, and records exactly what it fetched.

Everything resolves through GitHub. There is no upload step and no server of our
own to run.

**Two ways to name a dependency.** A project uses either one, or both.

```toml
# An index gives the versions. The project names the index once.
[registry]
main = { git = "https://github.com/preston/lark-index" }

[dependencies]
json = "1.2.0"                    # through the index
http = { version = "^0.4", registry = "main" }

# Direct, with no index at all.
zlib = { git = "https://github.com/preston/lark-zlib", tag = "v2.1.0" }
local = { path = "../lark-http" }
```

**The index is the source of truth.** An index is a git repository holding one
TOML file per package. The file names the source repository and every published
version, and each version pins a commit hash.

```toml
# lark-index/j/js/json.toml
name = "json"
repository = "https://github.com/preston/lark-json"

[[version]]
version = "1.2.0"
commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"
yanked = false

[[version]]
version = "1.1.0"
commit = "3f7a1c9d2e8b4a6057913f2e8d4c6b0a97531e42"
yanked = true
reason = "the parser accepted a trailing comma"
```

**Rule.** An index entry must pin a commit hash. A tag moves and a branch moves,
so neither is a version. The manager refuses an entry that names anything else,
and it verifies that the hash exists in the repository the entry names.

That rule is what makes the index worth having. A direct dependency trusts
whoever controls the tag. An index dependency trusts a hash, and a hash cannot
change under it.

**Versions.** Semantic versions, with the usual ranges: `1.2.0` means `^1.2.0`,
and `=1.2.0` pins exactly. A range resolves against the index alone, because a
range needs a list of versions and only an index has one. A direct dependency
names a tag or a commit and gets no range.

**Resolution.** The graph is flat. One version of one package per build.

1. Read `lark.lock` when it exists. It names a commit per package. Fetch those
   and stop.
2. Otherwise, collect every requirement across the whole dependency graph.
3. For each package, pick the highest version that satisfies every requirement.
4. If no version satisfies them all, report an error naming each requirement and
   the path that asked for it.
5. Write `lark.lock`.

A yanked version never resolves, unless the lock file already names it, so an
existing build keeps working.

**The lock file** records the resolved commit and the tree hash for every
package, direct and transitive. A build with a lock file fetches by hash and
consults no index. That is what makes a build reproducible, and rule F-2 already
asks for the property.

**The store.** A fetched package lives read only in
`~/.lark/store/<host>/<owner>/<repo>/<commit>`, shared between projects. A
project holds no copy. An index clone lives in `~/.lark/index/<host>/<owner>/<repo>`.
`LARK_HOME` overrides the location, and the test suite sets it, so no test
touches a real home directory.

**Namespacing.** A dependency is a module search root, so `@import json` finds
`json.lark` in the package. Rule N-3 already searches a path, and a dependency
adds one entry. Two packages that export the same name collide at link time,
which rule X-5c already states for two modules.

**Publishing** is a pull request against the index repository: one file, one new
`[[version]]` entry. The manager writes the entry with `lark publish --index main`,
which reads the local tag, resolves it to a commit, and prints the diff to apply.
It pushes nothing on its own.

**Design decisions to settle before the code.**

| Question | Recommendation |
|---|---|
| Two versions of one package | An error. The message names every requirement and its path. |
| A direct dependency beside an index one | Allowed. A direct entry wins, and the build warns once. |
| Checksums | The lock file holds the tree hash. A store entry that does not match it is refetched. |
| Native libraries | A package states `[build] link = ["z"]`, and the driver passes `-lz`. |
| Build scripts | No. A package that needs to run code at build time is out of scope. |
| Private repositories | Whatever `git` already does. The manager runs `git` and adds no credential handling. |

**Crate.** `lark-pkg`. It depends on `lark-driver` for the configuration types
and on nothing else in the workspace. It shells out to `git`, as decision D005
shells out to `cc`, because a git implementation in Rust is a dependency far
larger than the tool.

**Commands.**

```
lark add json@1.2.0                  resolve through the index, write the lock
lark add <git-url> [--tag v1.0.0]    add a direct dependency
lark update [<name>]                 refetch and rewrite the lock
lark tree                            print the dependency graph
lark vendor                          copy every dependency into ./vendor
lark publish --index main            print the index entry to submit
```

**Exit.**
- A T1 unit test set for the lock file and for version resolution: read, write,
  round trip, a range that resolves, a range that cannot, and a yank.
- A T1 test that an index entry naming a tag rather than a commit is an error.
- A T6 execution fixture that depends on a package in a local git repository
  that the harness creates, builds, and runs.
- A fixture with a local index repository, proving that a range resolves to the
  commit the index pins.
- A fixture proving that a build with a lock file fetches nothing and reads no
  index.
- A fixture proving that two incompatible requirements report an error naming
  both paths.
- `lark tree` output is a T5 golden snapshot.

**Result.** `lark-pkg`, five modules and 29 tests. Chapter 13 of the
specification states rules K-1 through K-10.

The resolver reads an index through a trait, so its ten tests build an index in
memory and need no git at all. The end to end tests create local git
repositories, commit to them, and fetch from them, so nothing reaches the
network and `LARK_HOME` keeps every test out of a real home directory.

A build now fetches before it resolves modules, and a package directory joins
the search path that rule N-3 already walks. A program that writes
`@import mathx` compiles against a package that an index pinned to a commit.

Deleting the index and building again works, because the lock file names the
commit. That is rule K-7 proved rather than promised.


### Phase T3 - Incremental build [DONE]

A build recompiles every module every time. For a program of any size, most of
that work repeats.

**Design.** The unit of caching is a module. A module's output is valid when its
fingerprint has not changed.

The fingerprint holds:

1. The hash of the module source.
2. The hash of every module it imports, transitively, but only of their
   **interfaces** and not their bodies.
3. The hash of the header set that `#include` produced. Rule C-1a already reads
   it, so the hash costs nothing extra.
4. The effective build settings, which rule F-2 already records.

The second item is what makes this work. A module's `.h` is its interface. A
change to a body that leaves the header identical invalidates that module alone.

**Generics are the hard part, and the design admits it.**

Rule G-1 says a generic has no C form, and the instantiation lives in the module
that declares the generic. So a module that instantiates `Box<int>` causes a
module it did not write to emit a new function. The dependency runs backwards
from the usual direction, and a naive fingerprint misses it.

Three ways to resolve it. The recommendation is the third.

| Approach | How | Cost |
|---|---|---|
| Recompile every generic owner | A module that declares a generic recompiles when any importer changes. | Simple, and it gives up most of the benefit for a library of generics. |
| Emit an instantiation where it is used | The user's module emits `Box<int>`, with `static` linkage. | Duplicate code across modules, and rule X-4a forbids a definition in two places. |
| A separate instantiation unit | Every instantiation of the whole program lands in one generated module, `lark_instances.c`. Its fingerprint is the set of instantiations, not the source of any module. | One extra unit. A new instantiation rebuilds that one unit and nothing else. |

The third approach also simplifies the emitter, because the instantiation logic
leaves the per-module path entirely. It changes rule G-1 and rule X-4a, and both
changes are stated rather than implied.

**Parallelism.** The module graph is a directed acyclic graph, which rule N-5
already guarantees. A build walks it in dependency order and compiles every
module of one level at the same time. Rayon is the obvious dependency, and the
decision record must state why the standard library thread pool is not enough.
Parallelism comes second, after incremental is correct, because a race in a
cache is far harder to find than a slow build.

**The cache.** `build/.lark-cache/<fingerprint>` holds the emitted `.c`, the
`.h`, the line map, and the object file. A hit copies. A miss compiles. The
cache is content addressed, so two branches that build the same module share the
entry, and nothing ever needs invalidating.

**Crate.** `lark-cache`. The driver asks it for a module and gets either the
cached output or a miss.

**Exit.**
- A T1 test set proving the fingerprint changes for each input that must change
  it, and does not change for a comment edit in an unrelated module.
- A T6 fixture that builds twice and proves the second build compiles nothing.
- A fixture that edits a body without touching the interface and proves that one
  module recompiles.
- A fixture that adds an instantiation and proves that only the instance unit
  recompiles.
- A stress test that builds a generated program of 200 modules under a parallel
  build, ten times, and compares every output byte for byte. A cache race
  appears as one differing run.

**Result.** `lark-cache`, and chapter 14 of the specification. Rules Y-1
through Y-6.

The design changed on one point, and for the better. The plan proposed a
fingerprint over the source, the interfaces of every import, the header set,
and the settings, plus a separate instantiation unit so that generics did not
make the graph run backwards. The implementation needs none of that. The key
holds the **emitted C**, which is the exact input to the compiler, so a new
instantiation changes the emitted C of the module that owns the generic and
nothing else needs saying. The cache models no dependency graph, so it cannot
model one wrongly. See decision D126.

Two measurements settled the shape. In release form the front end costs 2.59
seconds over the gumbo corpus and `cc` costs 0.85. So both halves are cached:
the header read under rule Y-4, and the object file under rule Y-5.

| Build | Before | After |
|---|---|---|
| Cold | 0.32s | 0.48s |
| Warm | 0.32s | 0.15s |

A cold build costs half again as long, because every header that a compile
reads is hashed. Rule Y-1 says which way that trade goes.

**What the stress test caught.** A build that edited a header from
`#define VALUE 7` to `#define VALUE 9` reused the old object. The length was
the same and the second was the same, so a witness of length and time held. The
witness now holds a hash of the content. That failure is the reason the test
exists, and it appeared on round nine of twelve.

### Phase T4 - Formatter and debugger [DONE]

Two tools, one phase, because both read the same two things the compiler already
produces: the lossless tree and the line map.

**The formatter.** Invariant R says the text of the tree equals the source, byte
for byte. A formatter is a second printer over the same tree. It is the smallest
tool here, because the hard work is already done.

Design: one canonical style with no options, as `gofmt` has. An option set turns
every project into an argument. `lark fmt` rewrites in place, `lark fmt --check`
exits non zero on a difference, and the gate runs the second form.

Formatting a file that does not parse rewrites the parts that do and leaves the
rest, because the tree keeps the broken text.

**Exit.**
- A T2 property test: formatting is idempotent. Formatting twice equals
  formatting once, for every file in the corpus.
- A T2 property test: formatting preserves meaning. The token stream of the
  output equals the token stream of the input, trivia aside.
- A T5 golden fixture per construct.

**The debugger.** Rule X-3 already emits `#line`, so `lldb` and `gdb` already
show Lark source and Lark line numbers today. What they do not show is a Lark
value: a `gc Person*` prints as an address.

Design: no debugger of our own. Ship a formatter script for each of the two
debuggers, in Python, that reads the object header at a negative offset and
prints the type name and the fields. Rule M-4 puts the header there, and rule
M-5 puts the field map in the descriptor, so the script needs no extra metadata
from the compiler.

`lark build` writes `build/lark-lldb.py` and `build/lark-gdb.py`, and prints one
line saying how to load them.

**Exit.**
- A T6 fixture that builds a program, runs it under the debugger in batch mode
  with a breakpoint, and compares the printed value to a snapshot.
- The fixture skips with a loud message when no debugger is on the path, the way
  `check-asan` skips.

**Result.** `lark-fmt`, and chapter 15 of the specification. Rules Z-1 through
Z-6.

The formatter is a printer over the tree that invariant R already guarantees.
Two of its decisions need the tree rather than the tokens: a `*` is a pointer
when its parent is a `POINTER` node, and a `<` opens a generic list when its
parent is a `GENERIC_ARGS` node. Rule L-6 already made both decisions once.

Two property tests found real defects on their first run.

`a++ + ++a` became `a++ ++a`, which lexes as `a ++ ++ a`. The style bound a
step operator to its value, and the program changed. A space now goes wherever
writing two tokens together would lex differently, and the check lexes the pair
rather than consulting a table.

`}else{` and `} else {` formatted differently, because the rule that keeps
`else` on the closing line read the next token and a space sat in the way. The
second pass then differed from the first. Every lookahead now skips trivia.

**The debugger.** No debugger, as planned. Two Python scripts, one per
debugger, that read the object header at a negative offset. A build writes both
beside the program, and rule Z-6 turns on debug information so that a script
can name a local.

```text
(lldb) frame variable team
(Person *) team = 0x986810150 Person[3] at 0x986810150

(lldb) gc-stats
collector          precise-marksweep
total_allocations  2
```

The count in `Person[3]` comes from the header, so an array prints its length
with no cast. The test runs the real `lldb` and skips with a loud message where
none is installed, the way `check-asan` skips.

### Phase T5 - Generational collector [DONE]

The seam holds it already. Chapter 10 rule R-2 names the write barrier, and the
capability table has room. `arena` and `semispace` proved that the seam is real,
and this one proves that it holds a collector with a barrier.

**Design.** A nursery and one old generation. Allocation is a bump pointer in
the nursery. A minor collection copies the survivors of the nursery into the old
generation, which makes it a moving collector with the constraints that
`semispace` already established: precise roots only, and no interior pointers.

The remembered set is a card table. A write of a managed pointer into an old
object marks the card that holds it, and a minor collection scans the marked
cards as roots.

**The write barrier is the new work, and it reaches the transpiler.**

Rule R-2 says a collector that needs no barrier supplies a trivial one that the
transpiler removes. Today every collector supplies the trivial one, so the path
has never run. Phase T5 makes the transpiler emit `lark_write_barrier(&slot,
value)` for a store of a managed pointer into a managed object, and remove the
call when the capability says the barrier is trivial.

The emitter needs to find those stores, which is the same kind of analysis that
rule M-8 enforcement needed in `lark_types::interior`.

**Concurrency is a separate phase, and it is not this one.** A concurrent marker
needs the barrier from this phase plus a tricolour invariant, and it changes the
stop the world protocol that rule M-26 states. Doing both at once makes a
failure impossible to attribute. This phase ends with a generational collector
that stops the world.

**Exit.**
- The whole runtime suite passes against `gc-generational`, which principle P-7
  already requires of every collector.
- A new runtime suite for the barrier: a store into an old object marks a card,
  a minor collection scans a marked card, and an unmarked card is not scanned.
- A T9 fixture with an old object that points at a young one, which is exactly
  the shape a missing barrier loses.
- A T5 golden fixture showing that a store of a managed pointer emits a barrier
  under this collector and no call under the others.
- The `tour` example gives identical output under all four collectors.

**Result.** `gc-generational`, a fourth collector, and the write barrier that
rule R-2 named and nothing had ever used.

The collector was the smaller half, as expected. A nursery, an old generation
of two halves, and a card table. A minor collection walks the nursery and the
marked cards. A major one copies the whole live set between the halves of the
old generation.

**The barrier reached three places.** `lark_gc_caps` gained a `write_barrier`
flag. `lark_collector` gained a function pointer, empty for every collector
that needs none. The emitter reads the capability and emits a call for a store
whose target names a `gc` field.

```c
a->next = b;                                 /* three collectors */
lark_write_barrier((void **)&a->next, b);    /* generational */
a->value = 9;                                /* never: not a managed field */
```

**What the new collector found.** `test_a_deep_chain_survives` builds a ten
thousand link chain with the tail in a plain local. Every earlier collector
had a space large enough that no collection ran during the loop. The nursery
does not, so the collection moved the tail and the chain broke at the join. The
test had assumed something it never stated, and only a collector that collects
often could show it.

The tour does not build under either moving collector, and that is rule R-1
working: line 99 takes an interior pointer into a managed buffer.

## 3. Order and what waits on what

```
T1 -- T2 -- T3 -- T4
                        T5
```

T5 shares no code with the others and runs whenever there is time for it. T4 is
listed after T3 for convenience rather than necessity: a formatter needs no
cache, but a person editing with a formatter wants a fast rebuild.

## 4. What this plan does not cover

- **A concurrent collector.** It follows T5 and needs its barrier.
- **A hosted index service.** T2 defines the index and reads it, and the index
  is a git repository that anyone hosts. Nothing here runs a server.
- **Cross compilation.** The driver passes flags to `cc`, so a target triple is
  a configuration entry rather than a phase. It becomes a phase when a program
  needs a runtime built for another platform.
- **A REPL.** The language compiles to C, so a REPL means compiling and linking
  per line. The value does not carry the cost.
