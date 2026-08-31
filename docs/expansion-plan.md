# The expansion plan

The language, the toolchain, and the release path all work. This plan covers
what comes after: a module system that scales, Windows, a concurrent
collector, a standard library, and a registry.

Each phase states its goal, its work, the rules it adds, and what proves it
done. A phase ends with the gate green.

## 1. The order, and why

The requested order was rough edges, concurrent collector, standard library,
repositories, registry. Three additions change it.

| Addition | Why it moves earlier |
|---|---|
| Windows | The standard library wraps threads, files, and sockets. A port after that rewrites every one of those modules. |
| Namespaces and features | The standard library is `std::collections::Vector`. It cannot exist before the module system that names it. |
| Generic interfaces | `Iterator<T>` and `Eq<T>` do not parse today. Collections need them. |

The platform seam also moves ahead of the concurrent collector. That collector
needs atomics and thread primitives. Written against the seam, it ports for
free. Written against pthread, it needs a second pass.

```
E1  rough edges
E2  language: namespaces, re-export, features, generic interfaces
E3  platform seam and Windows
E4  concurrent collector
E5  standard library
E6  registry and library repositories
E7  the official registry
```

E2 and E3 do not depend on each other. E4 depends on E3. E5 depends on E2 and
E3. E6 depends on E5. E7 depends on E6.

---

## E1 - Rough edges

Small, known, and each one blocks nothing else.

| Item | State |
|---|---|
| Every compilation runs through clang | **Done.** Rule F-7 |
| A generic return type is not mangled in the frame | **Done.** Decision D168 |
| A generic interface does not parse | Gap, found while planning |
| `auto` infers nothing from a call, a name, or a field | Gap, found while fixing the above. Moved to E2. |
| Generational collector is slowest on three of five benchmarks | **Done.** Now fastest on three. D173, D174, D175 |
| `lark-driver` tests write into the crate directory | **Not a defect.** The directory was stale, and no current test writes there. |
| `lark fmt` never ran on the project sources | **Done.** 40 files formatted, and the gate checks them. D171, D172 |

### The return type defect

`gc Vec<int>* make(void)` emits the right signature and the wrong temporary.

```c
static lk_c__Vec__i* make(void) {          /* correct */
    { Vec* _lk_result = ...; }             /* wrong, `Vec` is no C type */
}
```

Decision D144 sent the declaration path and the `auto` path through
`lark_mono::resolve`. `frame::return_type_of` never went through it. The fix is
one call. A test in `tests/exec` covers a generic return under every collector.

### Generational tuning

The collector reports 192 ms on `trees` where marksweep reports 117 ms, and
207 ms on `walk` where marksweep reports 130 ms. Three candidates, measured in
this order:

1. **The card table is scanned whole.** Every minor collection walks every card
   of the old generation. A program with a large old set pays for it on every
   nursery fill. Keep a high water mark of dirty cards, or a two level table.
2. **The nursery share is a guess.** `NURSERY_SHARE 4` came from one run. Test
   2, 4, 8, and 16 against all five benchmarks.
3. **Every minor collection promotes everything that survives.** One survival
   count per object, with promotion at two, keeps a short lived object in the
   nursery. This is the largest change and it goes last.

The benchmark suite already measures all five workloads against all four
collectors, so each candidate is one run.

### The other three

The `lark-driver` tests use a relative path, and the working directory of a
test is the crate directory. They move to `std::env::temp_dir`, which
`incremental.rs` already does.

`lark fmt` runs over every `.lark` file in the repository. The five golden
`.expected.c` files carry `#line` directives, so they change and get blessed. A
gate step then runs `lark fmt --check`, so the sources cannot drift again.

**Done when:** every item above is fixed, and the gate checks formatting.

The first exit test asked for the generational collector to come within 20
percent of the mark and sweep collector on `trees` and `walk`. That was the
wrong test. Both benchmarks retain everything they allocate, so a collector
that copies pays for the whole live set and reclaims nothing. The right test is
the shape of the result: the collector wins where objects die young, and stays
in the same class where they do not. It now does both. See decision D175.

---

## E2 - Language: modules, features, generic interfaces

The largest language change since the superset work. It changes every emitted
symbol, so it lands before the collector adds more emitter surface.

### Namespaces

A namespace comes from the directory path, and a block nests further inside a
file.

```
std/
  collections.lark      -> namespace std::collections
  io/
    file.lark           -> namespace std::io::file
```

```c
// std/collections.lark, implicitly namespace std::collections

namespace detail {
    int grow_capacity(int n) { ... }        // std::collections::detail
}

export managed struct Vector<T> { ... }     // std::collections::Vector
```

| Rule | Statement |
|---|---|
| N-10 | A directory contributes one namespace segment. The file stem contributes the last one. |
| N-11 | A `namespace` block nests inside the namespace that holds it. It takes no `export` of its own. |
| N-12 | A name is visible unqualified inside its own namespace and every namespace nested in it. |
| N-13 | A qualified name names any depth: `a::b::c::Name`. |

### Re-export

Both forms. A facade module passes a namespace through. A curated module lifts
one name.

```c
// std.lark
export @import std::collections;      // the whole namespace passes through
export use std::io::print;            // one name moves up to std::print
```

| Rule | Statement |
|---|---|
| N-14 | `export @import p;` makes `p` visible to every module that imports this one, under the same path. |
| N-15 | `export use p::n;` makes `n` visible under this module's namespace. |
| N-16 | A re-export cycle is an error, and the report names the cycle. |

### Features

Two levels. A feature names modules. An attribute gates one declaration.

```toml
# the package that offers them
[features]
default = ["collections", "io"]
collections = ["std::collections"]
crypto = ["std::crypto"]

# the consumer
[dependencies]
std = { version = "0.1", features = ["collections"] }
```

```c
@feature("async")
export void print_async(gc char* s) { ... }
```

| Rule | Statement |
|---|---|
| K-11 | A package declares its features and the modules each one enables. |
| K-12 | A dependency entry selects features. The set is the union over the whole graph, so two consumers never disable each other's feature. |
| K-13 | An import of a module that no enabled feature covers is an error. The report names the feature that supplies it. |
| K-14 | `@feature("x")` removes a declaration when `x` is off. A use of a removed name reports the feature that restores it. |

### The C name

This changes rule X-5. Today an export links under its last segment. That
cannot survive two namespaces exporting `Vector`.

```c
export managed struct Vector<T> { }   // links as lk_std_collections__Vector

@abi("C")
export int lark_version(void) { }     // links as lark_version
```

| Rule | Statement |
|---|---|
| X-5 | **Changed.** An exported name links under its mangled full path. |
| X-9 | `@abi("C")` makes a declaration link under its written name. Two of them that collide is an error, reported with both positions. |

A migration note goes in the specification, because this breaks any C caller
that links against a Lark export today.

### Inference from a name and a call

`auto` reads a literal, a cast, a `new`, and an operator. It reads nothing from
a name, a call, a field, or a method.

```c
auto a = plain();      // no type, emitted as `auto` and rejected by C
auto b = other;        // the same
auto c = value.field;  // the same
```

Every one of these needs a name to type environment, which the type crate does
not carry. The resolver builds one for namespaces in this same phase, so the
two land together.

| Rule | Statement |
|---|---|
| T-12 | `auto` reads the type of a call from the signature of the function it names. |
| T-13 | `auto` reads the type of a name from its declaration, and of a field from its record. |

Collections make this urgent. `auto v = Vector::with_capacity(8);` is the shape
a standard library uses everywhere.

### Generic interfaces

`iface Seq<T>` does not parse. Collections need it.

```c
export iface Iterator<T> {
    bool next(Self this, gc T* out);
}

impl Iterator<int> for Range { ... }
```

| Rule | Statement |
|---|---|
| O-25 | An interface takes generic parameters. Its method table is per instantiation, the same way rule G-1 handles a generic record. |
| O-26 | An implementation names the instantiation it satisfies. |

### Reach

| Crate | Change |
|---|---|
| `lark-syntax` | `namespace` blocks, `export @import`, `export use`, `@feature`, `@abi`, generic `iface` |
| `lark-resolve` | Namespace tree, re-export edges, feature gating, cycle detection |
| `lark-mono` | Interface instantiation beside record and function |
| `lark-codegen` | Full path mangling, `@abi` opt out, collision check |
| `lark-driver` | Feature resolution from the manifest |
| `lark-pkg` | Feature declaration and selection |
| `lark-lsp`, `lark-fmt` | New syntax |

**Done when:** `examples/tour.lark` covers every construct above, the gumbo
corpus still builds, and the specification carries every new rule with a test.

---

## E3 - The platform seam and Windows

Full support, runtime included. Managed programs run on Windows.

### The seam

Every platform primitive moves behind `runtime/core/lark_plat.h`, with one
implementation per family.

| Primitive | Unix | Windows |
|---|---|---|
| Mutex | `pthread_mutex_t` | `SRWLOCK` |
| Condition | `pthread_cond_t` | `CONDITION_VARIABLE` |
| Thread id | `pthread_self` | `GetCurrentThreadId` |
| Stack bounds | `pthread_getattr_np`, `pthread_get_stackaddr_np` | `NtCurrentTeb`, or `VirtualQuery` |
| Register capture | `setjmp` | `RtlCaptureContext` |
| Aligned allocation | `aligned_alloc` | `_aligned_malloc` with `_aligned_free` |

The paired free matters. `_aligned_malloc` needs `_aligned_free`, so the seam
owns both sides.

### The compiler

**Settled in E1.** Every compilation runs through clang, on every platform.
Rule F-7 states it. One compiler means one flag dialect, so the Windows port
needs no second set of flags and no profile field.

`build.cc` still names another compiler for a project that needs one. A
compiler that rejects the flags is the caller's problem, and the build reports
what it ran.

The runtime suite runs a second time under gcc on Linux, because gcc reported a
frame overrun that clang accepted. See decision D164.

| Rule | Statement |
|---|---|
| R-9 | The runtime names every platform primitive through one seam. A collector calls the seam, never the platform. |

### Continuous integration

A `windows-latest` job joins the gate. It runs the Rust tests, the runtime
suite, and the fixture suite. AddressSanitizer on Windows comes later, because
its support is thinner.

**Done when:** the gate passes on Windows, and a release archive for
`x86_64-pc-windows-msvc` builds a managed program on a clean machine.

---

## E4 - The concurrent collector

Concurrent mark, stop the world sweep. Non moving, so interior pointers keep
working and rule R-1 rejects nothing new.

### The design

The collector marks while the program runs. Rule R-2 already gives the write
barrier. The barrier gains a second job: it records a pointer that the program
overwrites, so the marker cannot miss an object that moved out of reach.

| Rule | Statement |
|---|---|
| R-10 | A concurrent collector holds the tricolour invariant. No black object holds a pointer to a white one at the end of the mark. |
| R-11 | The barrier shades the old value of a slot grey before the store. This is a deletion barrier, and it makes the mark complete without a rescan. |
| R-12 | An object allocated during a mark starts black. It survives the cycle it was born in. |
| R-13 | The mark ends with a short stop the world pass that drains what the barrier recorded. The sweep runs stopped. |

### The cost

The barrier already exists for the generational collector, so the emitter needs
no new work. Rule M-18 already decides which functions can allocate.

The capability table gains `concurrent`, which rule R-1 reads.

### The risk

This is the hardest piece in the plan. The failure mode is a missed root that
appears once in a million cycles. Three defences:

1. Torture mode already runs a collection at every safepoint. It extends to
   running the marker at every safepoint.
2. The benchmark suite already checks that every collector returns the same
   checksum for the same work.
3. A randomized graph test builds, mutates, and drops object graphs while the
   marker runs, comparing the survivor set against a stop the world reference.

**Done when:** the collector passes the whole runtime suite, the fixture suite
under torture, and the benchmark checksums, on Linux, macOS, and Windows.

---

## E5 - The standard library

Written in Lark. C appears only where a syscall does.

### The tree

| Module | Language | Holds |
|---|---|---|
| `std::core` | Lark | `Eq`, `Ord`, `Hash`, `Show`, `Clone`, `Iterator` |
| `std::collections` | Lark | `Vector<T>`, `HashMap<K,V>`, `Set<T>`, `Deque<T>` |
| `std::string` | Lark | `String`, slices, formatting, UTF-8 |
| `std::io` | Lark over C | Console, files, paths, directories |
| `std::thread` | Lark over C | Spawn, join, mutex, channel |
| `std::net` | Lark over C | TCP, UDP, address resolution |
| `std::random` | Lark | A named generator, seeded explicitly |
| `std::crypto` | Lark over C | Hashing and random bytes. Off by default. |
| `std::time` | Lark over C | Monotonic and wall clock |

`std::core` comes first. Every other module implements its interfaces.

### The rule for the boundary

A module is Lark unless it needs a syscall, a platform header, or a primitive
the language does not express. That rule goes in the conventions, so the
boundary does not drift.

### What the library will find

A standard library written in the language is the hardest test the language
gets. Expect defects in generics, interfaces, and the collector, at a rate
higher than the guide or the benchmarks produced. That is the point of writing
it in Lark.

**Done when:** every module has tests, the gate runs them, and a sample program
uses collections, strings, files, and threads together.

---

## E6 - The repositories

Two new repositories, then the links.

| Repository | Holds |
|---|---|
| `lark-index` | One TOML file per package, per rule K-1. The source of truth for versions. |
| `lark-std` | The standard library, released by tag, listed in the index. |

The index format already exists in specification chapter 13. `lark publish`
already writes an entry. Neither has run against a real repository.

**Done when:** `lark add std@0.1.0` in a fresh project resolves through the
index, fetches the package, and builds.

---

## E7 - The official registry

`lark` searches one index with no configuration. Any other index is added
first, which keeps the default trustworthy.

| Rule | Statement |
|---|---|
| K-15 | The official index is built in. A project reads it with no `[registry]` entry. |
| K-16 | Another index needs a `[registry]` entry. A package resolves through exactly one index, and an ambiguity is an error. |
| K-17 | `registry.official = false` turns the built in index off, for a project that reads only its own. |

**Done when:** a fresh project with no `[registry]` section resolves a package
from the official index.

---

## Later - A concurrency first runtime

Sketched, not planned. It sits above the language rather than inside it.

The idea is an opt in runtime with tasks, an executor, and asynchronous IO,
closer to what a Rust user gets from Tokio. It is a much higher abstraction
than the Lark runtime otherwise allows, so it stays a package rather than a
language feature.

Three questions decide whether it is possible, and they need answers before it
becomes a plan:

1. A task that parks mid function needs its stack saved. Lark has no coroutine
   transform. Does the runtime use one stack per task, or does the compiler
   gain a transform?
2. A shadow stack frame is per thread. A task that moves between threads
   carries its roots with it. Rule M-10 needs an answer for that.
3. The collector stops the world. A task runtime with thousands of tasks needs
   a safepoint protocol that scales past the current thread list.

The concurrent collector in E4 answers part of the third. The first two are
open.

---

## Risks

| Risk | Mitigation |
|---|---|
| The E2 mangling change breaks every existing C caller | A migration note, and `@abi("C")` to keep any name that matters |
| Windows runtime work is larger than it looks | The seam lands first and is testable on Unix alone |
| The concurrent collector hides a rare missed root | Torture mode, checksum agreement, and a randomized graph test |
| The standard library exposes many language defects at once | `std::core` first, one module at a time, gate green between each |
| Three platforms and five collectors is a large matrix | The matrix already runs. Each addition costs runner time, not new machinery. |

## Open decisions

| Decision | Recommendation |
|---|---|
| Windows compiler | clang first, `cl` as a later flag profile |
| Nursery survival counting in E1 | Measure the two cheaper candidates first, and skip it if they suffice |
| `std::crypto` scope | Hashing and secure random bytes only. No ciphers, no TLS. |
