# 14 - Caching

## 1. The rule that shapes everything else

**Rule Y-1.** A wrong cache produces a program that builds and misbehaves,
which is the worst failure a build tool has. Every doubt therefore resolves
toward a miss. A cache that does too little costs time. A cache that does too
much costs correctness.

That rule decides three design choices that a faster design would make
differently.

| Choice | The faster way | What Lark does |
|---|---|---|
| What names an entry | A timestamp | The content of every input |
| What a witness records | A length and a time | A hash of the content |
| What an unreadable input gives | The value of an empty file | A value of its own |

## 2. What names an entry

A key names every input to one step. It is a hash, and it is also the file
name of the entry, so a directory of entries needs no index.

Two properties matter, and both are tested.

1. Every value carries a label, so two inputs of the same bytes under
   different names give different keys. Without the label, swapping two
   arguments would leave the key unchanged.
2. Every value carries its length, so two values never run together into a
   third. Without the length, `ab` then `c` would hash the same as `a` then
   `bc`.

A build that compares timestamps asks whether a file is newer than its output.
That answer is wrong whenever a clock moves, a file is restored, or two
branches share a directory. A key built from content is right in all three
cases, and it needs no invalidation step: an entry that no key names is never
read.

## 3. What a witness records

Some inputs are files that a subprocess reads on its own. The C preprocessor is
the example: the include lines name a header set indirectly, and the build
learns the names only after the run.

**Rule Y-2.** A cache entry records every file that the step read and that its
key did not name. A later run checks each one before it trusts the entry.

A witness records a hash of the content, not a timestamp and not a length.
Both of those miss an ordinary edit.

```c
#define VALUE 7      /* before */
#define VALUE 9      /* after: the same length, the same second */
```

A build that ran twice inside one second would reuse the first object. The
stress test in `crates/lark-driver/tests/incremental.rs` found exactly that,
and it is why the record is the content itself.

## 4. What a cache is for

**Rule Y-3.** A cache is a saving, never a source of truth. Every entry can be
deleted at any moment, and the build then does the work again and produces the
same program. `LARK_NO_CACHE=1` turns it off, and a build with it off must
produce byte for byte what a build with it on produced.

## 5. What is cached

**Rule Y-4.** The header read of a module is cached. The preprocessor is the
slowest step of the front end, and its answer depends on the include lines, the
build settings, and the header files. The first two go in the key. The third
goes in the witness list, so a header that changes on disk makes the entry a
miss.

**Rule Y-5.** Each module compiles to its own object file, and the object is
cached. The key holds the emitted C, every generated header of the build, the
compile flags, and the identity of the C compiler. Every other file that the
compile read joins the witness list.

A generated file never joins the witness list. The build rewrites it every
time, so its timestamp always differs, and its content is already in the key. A
witness on one would turn every entry into a miss.

**Rule Y-6.** Several units compile at the same time. One translation unit
reads no output of another, so nothing orders them. The link waits, because it
reads every object.

The object file name carries the key, so two units never write one file. A race
would appear as a program that differs between runs, so one test builds ten
modules from an empty cache ten times and compares every output.

## 6. Where a generic fits

A generic has no C form of its own. Rule G-1 emits one copy per instantiation,
in the module that declares the generic. So the output of that module depends
on what other modules ask for, and the dependency runs backwards from the usual
direction.

The key needs no special case for it. The key holds the emitted C, and a new
instantiation changes the emitted C of the module that owns the generic. A
module that gains an unrelated edit does not change it, and its object is
reused.

That is the whole answer. The cache never models the dependency graph, so it
cannot model it wrongly.

## 7. What is not cached

| Not cached | Why |
|---|---|
| The parse and the resolve | Rule L-8 makes a module order independent, and rule G-1 needs every module, so the front end reads them all anyway. |
| The link | It reads every object, so it repeats whenever any one changes. |
| A system header upgrade | A witness catches a change to a header file. A compiler upgrade changes the identity in the key. |
