# Decision Log

Every entry records a decision, the reason, and the alternatives. An entry with
status **settled** does not reopen without new information.

---

## D001 - The transpiler is written in Rust
**Status:** settled.
**Reason:** A good standard library, and memory safety that removes a class of
defect from the tool itself.
**Rejected:** C, for self-hosting later. Self-hosting stays a long term option.

## D002 - The output is C11
**Status:** settled.
**Reason:** Wide support, `_Atomic` and `_Alignof` available, no extension
needed.

## D003 - The emitted C stays readable, with `#line` directives
**Status:** settled.
**Reason:** Constraint D-2. A programmer who wants to see the cost of a
construct reads the output. It also makes golden file tests possible.

## D004 - Lark is a strict superset of C11
**Status:** settled.
**Reason:** The project goal. C interoperation is fundamental, and a near
superset creates a permanent porting tax.
**Cost:** A full C11 front end. This is the largest component of the project.
**Mitigation:** See D005, D006, D007.

## D005 - Lark does not implement the C preprocessor
**Status:** settled.
**Reason:** The system compiler already does it correctly. Writing one adds no
value and adds a large surface for defects.
**Method:** Invoke `cc -E`.

## D006 - `#include` passes through to the emitted C
**Status:** settled.
**Reason:** The output keeps the original include, so the linker resolves libc
with no work. Lark never re-emits a libc prototype.

## D007 - The parser tolerates compiler extensions but does not model them
**Status:** settled.
**Reason:** Real system headers are dense with GCC and Clang extensions. A
parser that rejects them cannot read `stdio.h` on any real machine. Modeling
their semantics has no benefit for Lark.

## D008 - Delivery runs in three phases
**Status:** settled.
**Reason:** Phase A gives a working language with tests early. Phase C completes
the superset. The end state is identical. Only the order changes.
**Constraint from the user:** an LSP must give correct completions at every
phase, for the code that phase supports.

## D009 - Every Lark keyword is contextual
**Status:** settled.
**Reason:** D004 requires it. Reserving `var`, `let`, or `func` breaks any C
program that uses the name as a typedef. Only the `@` sigil space and the
`_Upper` reserved space are free, and both are ugly for common syntax.
**Method:** Each keyword sits where valid C11 fails to parse, by the constraint
in C11 6.7.2p2.

## D010 - `auto` is kept, and means type inference
**Status:** settled.
**Reason:** C11 6.7.2p2 makes `auto x = 5;` invalid C11, so the spelling is
free. C23 gives it the same meaning that Lark wants. Lark agrees with the
standard rather than diverging.
**Rejected:** `var` and `let`, both of which break D004.

## D011 - Generics keep `<T>` syntax
**Status:** settled.
**Reason:** An identifier that names a type cannot begin a valid C expression.
Two pass resolution knows every type name before it parses any body, so the
decision is deterministic. C++ cannot do this, because its lookup depends on
declaration order.
**Rejected:** A turbofish `::<T>` and a sigil form `@<T>`. Neither is needed.

## D012 - `gc` qualifies the pointer, not the pointee
**Status:** settled.
**Reason:** It gives a clean rule for the object header. A `gc T*` is an address
with a header before it.
**Consequence:** A struct with a `gc` field needs the `managed` marker, because
the collector needs a field map. This is rule T-4, and it is the load bearing
rule of the memory model.

## D013 - Version 1 ships a precise, non moving, mark and sweep collector
**Status:** settled.
**Reason:** Precise tracing from the start forces the field map and the shadow
stack design to be correct. Retrofitting precision is much harder.

## D014 - The root mechanism is configurable
**Status:** settled.
**Reason:** Constraint D-4. A shadow stack costs a push and a pop per function
with a managed local. A program that does not want the cost selects
conservative mode, with no source change.
**Setting:** `gc.roots = "shadow-stack" | "conservative"`.

## D015 - Interior pointers are legal, and a block map supports them
**Status:** settled.
**Reason:** A `gc` buffer is a normal thing to want. `&buf[3]` must work.
**Method:** Size classed pages plus a block map. Any interior address resolves
to its payload base in constant time.
**Cost:** A moving collector becomes much harder.
**Resolution:** The collector declares `interior_pointers` in its capability
record. The transpiler enforces the matching source rule. A future moving
collector sets the flag to false. The language does not change.

## D016 - A managed pointer cannot live in `malloc` memory
**Status:** settled.
**Reason:** Nothing traces it. The four legal places are the stack, a managed
struct, a `@global` block, and an array a managed struct owns.

## D017 - A managed struct can live on the stack
**Status:** settled.
**Reason:** The shadow stack traces it. It has no cost for the common case.

## D018 - The runtime saves and restores the shadow stack head across `longjmp`
**Status:** settled.
**Reason:** A `longjmp` skips every frame pop and corrupts the root set.
**Method:** `lark_setjmp` and `lark_longjmp`. Plain `setjmp` stays correct for
code with no managed local.

## D019 - Safepoint polls go at every allocation and every loop back edge
**Status:** settled.
**Reason:** These two points bound the time to reach a safepoint. A function
that cannot reach an allocation emits no poll.

## D020 - An unmarked extern defaults to `gc_safe`
**Status:** settled.
**Reason:** The safe default is always correct. Every mainstream implementation
does this: CoreCLR P/Invoke, LLVM statepoints, HotSpot JNI, and Go cgo all
transition by default and offer an opt out.
**Cost:** A call to `strlen` pays a transition unless it is marked `gc_leaf`.
**Accepted:** Correctness first. `gc_leaf` is a one word opt in.

## D021 - One marker covers both the transition and the rooting contract
**Status:** settled.
**Reason:** Constraint D-5. Splitting them into two axes adds a second marker
for little gain.

## D022 - The object header sits before the payload
**Status:** settled.
**Reason:** It keeps C layout compatibility. A `gc Person*` and a `Person*`
point at the same bytes, so C code reads the fields with no adaptation.

## D023 - Interface values exist, as a two word fat pointer
**Status:** settled.
**Reason:** It makes an interface a real type, not only a call site feature.
**Cost:** The type has no C ABI form. A function that takes one is not callable
from C. The type name makes this visible, so constraint D-2 holds.

## D024 - Dispatch is static when the concrete type is known
**Status:** settled.
**Reason:** Constraint D-1. A direct call costs what a C call costs.

## D024b - `managed` on a generic means conditionally managed
**Status:** settled.
**Reason:** Rule O-2 applied naively to generics breaks the generic container,
which is one of the things Lark exists to make pleasant. `Box<int>` and
`Box<gc Person*>` must both work from one declaration.
**Method:** `managed struct Box<T>` carries a header only for an instantiation
that needs one. An instantiation with no `gc` field and no `impl` pays nothing.
**Why not inference:** Constraint D-2. Automatic inference of `managed` hides
the machinery. The marker at the declaration keeps it visible.
**Why not two types:** Constraint D-5 and ordinary usability.
**Consequence:** Two instantiations of one generic can have two layouts. This is
already true of any generic.

## D025 - Generics have no constraints in version 1
**Status:** settled.
**Reason:** Constraint D-5. A constraint system is a large feature. Errors after
substitution are acceptable, provided the diagnostic names the instantiation.
**Revisit:** After the language works end to end.

## D026 - `export` marks a symbol as public. Private is the default
**Status:** settled.
**Reason:** A default of private keeps a module interface small and deliberate.
**Detail:** An `impl` is exported with its type. A type without its methods is
not usable.

## D027 - `init` marks where startup goes, not the entry point
**Status:** settled.
**Reason:** C `main` stays the entry point, so D004 holds. A program that starts
from a host callback puts `init` on that callback.
**Detail:** Exactly one `init` per program. Zero and two are both errors.

## D028 - Runtime startup always runs first
**Status:** settled.
**Reason:** Nothing else works before the collector exists. Its position is not
configurable.

## D029 - A `@global` block initializes at most once, guarded by a flag
**Status:** settled. Confirmed by the user.
**Reason:** A block can get both an implicit `@init` from `@global(f)` and an
explicit `@init`. A runtime guard is safer than a compile error, because the
double initialization is hard to see across files.
**Alternative:** A compile error on the double initialization.

## D030 - Block ordering is explicit, never inferred
**Status:** settled.
**Reason:** Constraint D-2. An automatic topological sort of initializer
dependencies is hidden machinery. An order number is visible in the source.

## D031 - A string literal is assignable to `gc char*`
**Status:** settled.
**Reason:** A literal has static storage duration and lies outside the managed
heap. A collector that identifies managed addresses by range ignores it safely.
It removes friction from the most common case with no cost.
**Companion:** `str::from_cstr` copies into managed memory when a copy is
wanted.
**Rejected:** A `gc"..."` literal form. Constraint D-5. The two mechanisms above
already cover the case.

## D032 - Configuration lives in `lark.toml`, with command line overrides
**Status:** settled.
**Reason:** Familiar from Cargo and from Go. Every field has a flag, and the
flag wins.

## D033 - Diagnostics carry stable codes
**Status:** settled.
**Reason:** A test asserts the code, not the message. A message can improve
without a change to the test suite.

## D034 - The parser produces a full fidelity tree with error recovery
**Status:** settled.
**Reason:** The user requires a working LSP. A parser built for batch
compilation alone cannot be retrofitted for this cheaply.

## D035 - Every tracked file holds only ASCII bytes
**Status:** settled.
**Reason:** An em dash, a curly quote, and an arrow character all break a
terminal, a diff tool, or a C compiler somewhere. ASCII removes the class of
problem.
**Method:** `scripts/check-ascii.sh` in the gate. A test fixture that needs
non-ASCII bytes lists itself in `.ascii-exempt`.

## D036 - Documentation uses simple English, and a script checks it
**Status:** settled.
**Reason:** A reader whose first language is not English must understand the
text on one read. A rule that no check enforces decays.
**Method:** `scripts/check-prose.py` flags a banned word, a contraction, a
semicolon in prose, and a sentence over 25 words.

## D037 - No file names a tool
**Status:** settled.
**Reason:** The author owns the work. A tool name in a comment or a commit
message adds no information and dates the file.
**Method:** `scripts/check-attribution.sh` greps a word list over every tracked
file and over the last 200 commit messages.
**Note:** This overrides any default commit trailer that a tool adds.

## D038 - Lints live in the workspace, and the gate denies every warning
**Status:** settled.
**Reason:** A warning that survives becomes noise, and noise hides a real
defect.
**Method:** `[workspace.lints]` in the root `Cargo.toml`, and
`cargo clippy --workspace --all-targets --all-features -- -D warnings` in the
gate. A narrow `#[allow]` needs a reason comment. A crate level allow needs an
entry in this file.

## D039 - Dependency versions come from the registry, never from memory
**Status:** settled.
**Reason:** A version number written from memory is stale on the day it is
written.
**Method:** Add every dependency with `cargo add`, which resolves the current
version. Run `cargo update` at the start of each phase. `cargo deny check` and
`cargo audit` run in continuous integration.

## D040 - The compiler uses index based data structures
**Status:** settled.
**Reason:** A reference counted graph is expensive to copy, hard to serialize
for the language server, and easy to make cyclic. An index into an arena has
none of those problems.
**Method:** No `Rc<RefCell<T>>` in a compiler crate. A node holds a typed index.

## D041 - Two pedantic lints are allowed at workspace level
**Status:** superseded by D159.
**Reason:** Rule C-2.1 needs an entry here for a broad allow.
**Allowed:** `clippy::module_name_repetitions`, because a crate named
`lark-span` naturally holds a type named `Span` in a module named `span`.
`clippy::must_use_candidate`, because it fires on almost every getter and adds
noise without finding a defect.
**Not allowed:** everything else in `clippy::pedantic`.

## D042 - The diagnostic renderer is written here, not taken from a crate
**Status:** settled.
**Reason:** Chapter 12 of the specification fixes the output format, down to
the caret and the suggestion markers. A crate that renders a different format
turns the specification into a description of that crate.
**Cost:** About 150 lines in `lark-diag`.
**Revisit:** If the format ever stops being specified.

## D043 - The fixture harness uses libtest-mimic
**Status:** settled.
**Reason:** Rule C-4.3 needs an entry for a new dependency. `libtest-mimic`
turns each fixture into one named test inside `cargo test`, so a failure names
the file. The standard test harness cannot discover tests at run time.
**Alternative:** One `#[test]` per kind that loops over fixtures. It works, and
a failure then names the kind rather than the file.

## D044 - The phase 0 front end is a stub driven by fixture directives
**Status:** settled.
**Reason:** The harness must be provably working before the compiler exists. A
stub that returns canned output proves nothing. This stub reads the annotations
and the directives from each fixture, so the discovery, the matching, the
snapshot comparison, the C build, and the run all execute for real.
**Method:** `//~ ERROR LKxxxx` drives the diagnostics. `// stub-print: text`
drives the emitted C. Later phases replace the stub with the driver, and the
harness does not change.

## D045 - Each configuration in the matrix gets its own scratch directory
**Status:** settled.
**Reason:** The four runs from principles P-3 and P-4 happen at the same time.
A shared directory makes them overwrite each other's binary, and the failure
looks like a missing file.

## D046 - The lexer lives inside lark-syntax
**Status:** settled.
**Reason:** The tree library tags a token and a node with one type, so
`SyntaxKind` must cover both. A separate lexer crate then needs either a second
enum with a hand written conversion, or a numeric scheme that loses exhaustive
matching. Neither buys anything.
**Consequence:** The build plan lists one crate where it listed two.
**Kept:** The lexer stays a module with its own tests, so the boundary survives
as a design boundary.

## D047 - The tree uses rowan
**Status:** settled.
**Reason:** Rule C-4.3 needs an entry for a new dependency. Invariant R and the
language server both need a lossless tree with source ranges and cheap subtree
sharing. `rowan` is the library that rust-analyzer uses for exactly this.
**Alternative:** An index based tree of about 400 lines. It matches decision
D040 more literally, and it puts a defect in the most load bearing layer of the
compiler.
**Note:** `rowan` shares immutable green nodes. It has no cycles and it is cheap
to copy, so it meets the intent of D040.

## D048 - A definition that ends with a brace needs no semicolon
**Status:** settled. See rule O-25.
**Reason:** The demo code from the author omits it, and reading it back, the
semicolon adds nothing after a `}`.
**Safety:** C requires the semicolon, so accepting its absence adds programs
rather than removing them. Rule S-1 holds.
**Cost:** The parser must tell `struct S { } x;` from `struct S { }` followed by
the next item. A name is a declarator only when a declaration can continue after
it.

## D049 - The parser splits a shift token that closes two generic lists
**Status:** settled. See rule L-14.
**Reason:** C needs `>>` as one token for a shift. `Box<Data<int>>` needs two
closing angles. The lexer cannot know which, so the parser splits.
**Method:** The parser emits two `>` tokens into the tree for one `>>` token.
Rule L-13 holds, because the two halves join back into `>>`.

## D050 - The front end reports real diagnostics as phases land
**Status:** settled.
**Reason:** A fixture must be able to state a rule before the phase that
enforces it exists.
**Method:** `lark-test` holds a list of the codes that the real front end
produces today. A fixture annotation for any other code gets a stand in. When a
phase lands, its codes join the list and the stand in stops. No fixture changes.

## D051 - The oracle answers with three states, not two
**Status:** settled. See rules L-6 and L-15.
**Reason:** Rule L-6 gives three cases for an identifier before `<`: a type, a
value, and unbound. A boolean answer cannot carry the third.
**Method:** `Binding::Type`, `Binding::Value`, and `Binding::Unbound`, plus a
completeness flag that says whether the oracle knows every name.
**Why the flag:** Delivery phase A does not read headers. Without the flag, an
unbound name before `<` would open a generic argument list for every name that a
header declares.

## D052 - The resolver reports only what it can decide
**Status:** settled.
**Reason:** Phase A passes `#include` through without reading it, so a name that
no module declares can still be a real name. An unknown type error would fire on
every header symbol.
**Reported:** A qualified path, an export violation, a missing module, and a
generic base name in a module that reads no header.
**Deferred to phase 12:** Every other unknown name.

## D053 - The resolver parses each module twice
**Status:** settled. See rule L-8.
**Reason:** Rule L-6 needs the name table, and the name table needs a parse. A
top level declaration parses correctly without the table, so one extra parse
closes the loop.
**Cost:** Two parses per module. The parser is fast and the trees share their
green nodes, so the cost is small.
**Alternative:** A parser that collects names as it goes. It couples the parser
to name resolution, which the crate rule forbids.

## D054 - A module namespace holds no imported name
**Status:** settled. See rules N-2 and N-11.
**Reason:** The oracle for a module holds only the names that the module itself
declares. An imported name always needs its prefix.
**Consequence:** A bare name that an imported module exports is diagnostic
LK0612, and the message names the prefix to write.

## D055 - The parser owns a local scope stack
**Status:** settled. See rule L-16.
**Reason:** Rule L-6 resolves a name in the innermost enclosing scope, and only
the parser knows where it is while it parses.
**Method:** A block, a function, and a `for` statement each push a scope. A
declarator records its name. The lookup checks the local scopes first, then the
module oracle.
**Why not a third pass:** A scope aware oracle needs the parser to report scope
events, which is the same information, delivered less directly.
**Cost:** About sixty lines in the parser, and no new dependency.

## D056 - `gc` in the specifiers marks the outermost pointer
**Status:** settled. See rule T-1a.
**Reason:** The specification said `gc` binds "exactly as `const` does", and
that is wrong. A `const` in the specifiers qualifies the base type, and `gc`
cannot, because rule T-2 allows it only on a pointer.
**Method:** One `gc` marks the outermost pointer level. A second marks the next
level in. A `gc` after a `*` marks that pointer.
**Consequence:** `gc gc int* x;` is diagnostic LK0200, because the type has one
pointer level and the declaration carries two markers.

## D057 - A later pass reports nothing inside an earlier diagnostic
**Status:** settled. See rule DQ-4.
**Reason:** A declaration that the parser could not read has no reliable type.
Reporting a type error there turns one problem into two, and the second one is
noise.
**Method:** The type checks receive the syntax error spans and skip any
construct that overlaps one.

## D058 - The type checks are permissive
**Status:** settled.
**Reason:** The same reason as decision D052. Delivery phase A does not read
headers, so an unknown name yields the error type rather than a diagnostic.
**Consequence:** The error type absorbs every operation, so one unknown name
produces no cascade.

## D059 - A qualifier precedes the type it qualifies
**Status:** settled.
**Reason:** Rule O-25 makes the semicolon optional after `}`. Without this rule
the `gc` in `struct S { } gc Person* p;` joins the struct declaration.
**Method:** The parser reads `gc`, `init`, `gc_leaf`, and `gc_safe` as markers
only before a type specifier appears.

## D060 - The emitter transforms tokens rather than rebuilding source
**Status:** settled.
**Reason:** Rule X-2 wants readable output, and the most readable output is the
programmer's own text. The tree is lossless, so the emitter can keep every
token that C already understands and change only what Lark adds.
**Consequence:** A comment, a blank line, and an unusual indent all survive into
the emitted C. The C compiler reports a column that matches the source.
**Cost:** The emitter cannot reformat. That is a feature here.

## D061 - A user symbol keeps its name, and a generated one carries `lk_`
**Status:** settled. See rules X-5 and X-5a.
**Reason:** The specification mangled every module function. That deletes the
entry point, because `main` becomes `lk_app__main`. It also stops a C file from
calling an exported Lark function, which constraint D-7 requires.
**Method:** A function, a global, and a type keep their names. A private
definition becomes `static`, except `main`, an `extern` declaration, and a
prototype. Only a generated symbol uses the `lk_` prefix.
**Accepted:** Two modules that export the same name collide at link time, as two
C files do. Rule X-5c states it.

## D062 - An exported definition lives in exactly one file
**Status:** settled. See rule X-4a.
**Reason:** The first build failed with "redefinition of Point", because the
type was in both the header and the body.
**Method:** A type definition goes to the header alone. A function leaves its
prototype in the header and its body in the module. A variable gets an `extern`
declaration in the header and its initializer in the module.

## D063 - The declaration specifiers end at a `}` body
**Status:** settled. See rule O-25a.
**Reason:** Rule O-25 makes the semicolon optional, so `struct S { } static x;`
cannot be told from `struct S { }` followed by `static int x;`.
**Method:** A qualifier and a storage class both come before the type. C accepts
both orders, and Lark reads the second word as the next item.
**Related:** Decision D059 made the same choice for `gc`.

## D064 - A `#line` directive can name the file rather than its path
**Status:** settled.
**Reason:** A golden snapshot that holds an absolute path fails on every other
machine.
**Method:** The emitter takes the name as an option. A real build leaves it
empty, so a debugger opens the right file. A test sets it to the file name.

## D065 - The page table maps every aligned page, not every allocation
**Status:** settled. See rule M-7.
**Reason:** A large object spans more than one aligned page. Masking an address
inside its second page gives an address that a table of allocation bases does
not hold, so an interior pointer into a large object would not resolve.
**Method:** The allocator registers every 64 KiB page of an allocation, and all
of them point at the one owner. Rule M-7 then holds for any address inside any
object, whatever its size.

## D066 - The object header carries the element count
**Status:** settled.
**Reason:** Rule O-6 allocates an array, and the mark phase must walk the
managed fields of every element. Without a count it walks the first one.
**Method:** The header is two words: the type descriptor and the count. A single
object carries a count of one, so the mark phase needs no special case.

## D067 - The allocator zeroes every payload
**Status:** settled.
**Reason:** Rule O-5 makes a field with no designator zero. More important, a
collection can start between the allocation and the first field store, and rule
M-11 forbids a managed slot that holds garbage.
**Cost:** One `memset` per allocation. A generational collector can drop it
later by allocating from a pre-zeroed region.

## D068 - A thread records its stack position before it parks
**Status:** settled.
**Reason:** Conservative mode scans the machine stack, and a parked thread sits
inside a condition variable. The collector cannot find that thread's stack
pointer from outside.
**Method:** Every path that stops a thread first takes the address of a local
and calls `setjmp`, which spills the callee saved registers onto the stack. The
scan then reads the range and the spill area.
**Note:** The buffer is a spill area, not a jump target. This is the technique
that every conservative collector uses.

## D069 - The platform gives the high end of the stack
**Status:** settled.
**Reason:** Taking the address of a local inside `lark_thread_attach` marks that
function's frame, not the start of the stack. Every root above it would be
missed, which includes every local of `main`.
**Method:** `pthread_get_stackaddr_np` on macOS, `pthread_getattr_np` with
`pthread_attr_getstack` on Linux, and the caller's frame as a fallback.

## D070 - AddressSanitizer is probed, not assumed
**Status:** settled.
**Reason:** AddressSanitizer needs to map a large shadow region. The sandbox
that development runs in stops it, and a trivial program then hangs before
`main`. A gate that hangs is worse than one that reports the gap.
**Method:** `check-asan` builds and runs a trivial probe with a ten second
limit. It runs the real suite when the probe passes, and prints a loud skip
otherwise. `LARK_REQUIRE_ASAN=1` turns the skip into a failure, and continuous
integration sets it.

## D071 - A shadow stack slot holds the address of a local
**Status:** settled. See rule M-10.
**Reason:** The specification put the local inside the slot, so every use of it
became `_lf.s[0]`. That deletes the name the programmer wrote and breaks rule
X-2, which wants readable output. It also forces the emitter to rewrite every
expression rather than transform a few tokens.
**Method:** The slot holds `&local`, and the collector reads through it. The
local keeps its name and every use of it stays as written.
**Cost:** One more load in the mark phase. A moving collector can still update
the local through the address.

## D072 - Every `new` expression gets a temporary slot
**Status:** settled. See rule M-27.
**Reason:** `f(new A(), new B())` allocates twice. The first result belongs to
no local when the second allocation runs, so a collection there frees it.
**Method:** The result goes into a value slot in the frame, and the expression
reads it back. One slot per `new` site, which the source bounds.
**Rejected:** A ring of recent allocations. It keeps dead objects alive for a
bounded but unpredictable time, and no test can then observe a clean heap.

## D073 - The runtime startup comes before the frame push
**Status:** settled. See rule I-3.
**Reason:** The first emitted program hung. `lark_frame_push` runs on a thread
that no one attached yet, so it does nothing, and the frame never reaches the
shadow stack. Every managed local was then unrooted, and torture mode freed the
live list.
**Method:** The `init` function starts the runtime as its first statement, and
the frame declaration follows.

## D074 - Every record definition also emits a typedef
**Status:** settled. See rule X-8.
**Reason:** Lark names a type without the keyword, as `gc Person* p`. C needs
either the keyword or a `typedef`.
**Method:** A forward `typedef` for every record comes before every definition,
so a field can name its own record and two records can name each other.

## D075 - The emitter walks the tree, not a flat token list
**Status:** settled.
**Reason:** Phase 6 replaces whole nodes: a `new` expression becomes a call, a
`return` becomes a block, a loop body gains a poll. A flat token loop cannot
replace a subtree.
**Kept:** The walk still writes each token verbatim where nothing changes, so
decision D060 holds and the output keeps the programmer's formatting.

## D076 - A method table entry calls a thunk, not the method
**Status:** settled.
**Reason:** A method table must hold one signature for every implementation,
and the written signature names `Self`, which differs per implementation. C
gives no way to call a function pointer through another signature. Casting one
and calling it is undefined.
**Method:** Each method gets a thunk that takes `void *` and calls the real
function with the receiver form that the programmer wrote. The table holds the
thunk.
**Cost:** One call per dynamic dispatch. A direct call under rule O-19 skips it.

## D077 - Every table lives in the module epilogue
**Status:** settled.
**Reason:** A method table names a thunk, an interface table names a method
table, and a type descriptor names an interface table. Each one must follow the
definitions it holds.
**Method:** The epilogue defines them in that order. The prologue declares each
one, so any earlier item can name it.
**Consequence:** A table has external linkage rather than `static`. Rule X-5a
reserves the name space, so nothing collides.

## D078 - An interface value roots its object field
**Status:** settled. See rule O-24.
**Reason:** A fat pointer holds a managed pointer in its `obj` field. A shadow
stack slot that pointed at the whole value would give the collector a pointer to
a two word struct, not to the object.
**Method:** The slot holds `&value.obj`.

## D079 - Version 1 asks for a type argument list rather than inferring it
**Status:** settled. See rules G-6 and G-6a.
**Reason:** Rule G-6 promised inference. Inference needs the type of every call
argument, which needs the full expression type checker. That is a phase of its
own.
**Method:** A call carries the list. Diagnostic LK0501 names the list to write,
so a missing one costs one edit rather than a puzzle.
**Revisit:** After the type checker covers every expression.

## D080 - An instantiation belongs to the module that declares the generic
**Status:** settled. See rule G-7.
**Reason:** Rule G-7 shares one definition across the whole program. Two modules
that both write `Data<int>` must get one C definition, not two.
**Method:** The pass runs over the whole module graph and files each
instantiation under the declaring module. That module emits it.

## D081 - One text function decides every mangle
**Status:** settled. See rule X-5a.
**Reason:** The pass and the emitter both compute a mangled name. The first
build failed because one kept the `gc` marker and the other dropped it, so
`lk_g1__Box__P6Person` never matched `lk_g1__Box__G6Person`.
**Method:** `lark_mono::type_text` is the one function that turns a type node
into text. Both callers use it.

## D082 - A half consumed shift token must never survive a path change
**Status:** settled. See rule L-14.
**Reason:** The parser splits `>>` into two `>` tokens for a nested generic
list. The split sets a flag and leaves the position on the shift token. Any
path that then leaves the generic grammar saw `nth(0)` report a token it never
consumed, and the parser spun forever on `Box<Data<int>> nested;`.
**Method:** `bump` consumes the second half whenever the flag is set, so every
path clears it.
**Test:** Four inputs that leave the generic grammar after the split.

## D083 - Rule I-1 belongs to a build, not to a check
**Status:** settled. See rule I-1.
**Reason:** The first version of the check reported LK0700 for every file with
no `init` marker, which included a valid C11 file. Rule S-1 forbids that.
**Method:** A program that uses managed memory needs the marker. A program that
uses none starts no runtime and needs none. The build runs the check, and a
single file check does not, because one file is not a program.

## D084 - A prototype beside its definition is a forward declaration
**Status:** settled. See rule X-5b.
**Reason:** The tour declared `handle_opaque_data` and then defined it. The
prototype stayed external and the definition became `static`, which C rejects.
**Method:** A declaration with no body whose name the module defines follows the
definition. The emitted C carries a forward declaration for every function
definition, so a call never precedes a declaration.

## D085 - The emitter writes the type that `auto` infers
**Status:** settled. See rules L-5 and T-10.
**Reason:** C11 has no `auto` inference, so the emitted C must name the type.
Phase 3 computed the type and nothing used it.
**Method:** A declaration whose specifiers hold only `auto` emits the inferred
type instead. The local type map uses the same inference, so a method call on an
`auto` local resolves.

## D086 - A record instantiation lives in the header
**Status:** settled. See rules G-1 and X-4.
**Reason:** A generic has no C form, so the header cannot carry the declaration.
An importing module still needs the layout of an instantiation it uses.
**Method:** The header carries every record instantiation, and the module body
carries the function instantiations and the field maps.
**Consequence:** The header carries a forward typedef for a private record that
an instantiation names. A forward typedef gives the name and no layout.

## D087 - A safe transition uses the comma operator and a helper
**Status:** settled. See rule M-19.
**Reason:** The specification writes the transition as three statements. A call
is an expression, and it can sit anywhere in one. C11 has no statement
expression, so a statement form would need the emitter to hoist the call.
**Method:** `(lark_enter_safe(), lk_leave__i(printf(...)))`. The comma operator
sequences the left operand first, and the helper takes the call as its argument,
so the order is enter, call, leave, value.
**Cost:** One `static` helper per result type, which the C compiler inlines.

## D088 - A safe transition counts its depth
**Status:** settled. See rule M-19.
**Reason:** One safe call can hold another, as in `f(g())` where both are
foreign. With a flag, the inner call would return the thread to the running
state while the outer callee still runs.
**Method:** The runtime counts. The thread enters the safe state on the first
transition and leaves it on the last.

## D089 - `export` marks a symbol, not one declaration of it
**Status:** settled. See rule N-6.
**Reason:** `export void attach(void);` followed by `void attach(void) { }`
made the prototype external and the definition `static`, which C rejects.
**Method:** The emitter collects every exported name of a module, and a
definition of one of those names stays external. The resolver already worked
this way, and the emitter now agrees.

## D090 - The emitted headers use `-iquote`, not `-I`
**Status:** settled.
**Reason:** A module named `pthread` emits `pthread.h`, and `-I` put it ahead of
the system header. The runtime then failed to find `pthread_mutex_t`.
**Method:** `-iquote` applies to a quoted include only, so `#include "pthread.h"`
finds the Lark module and `#include <pthread.h>` finds the system one.

## D091 - A configuration file applies to a package, not to a file
**Status:** settled.
**Reason:** `lark build tests/exec/x.lark` read `tests/exec/lark.toml`, so a
relative search path resolved against the wrong directory.
**Method:** The search walks up from the file to the nearest `lark.toml`, as
every build tool does. A relative path in it is relative to that directory.

## D092 - The language server splits analysis from the protocol
**Status:** settled.
**Reason:** A test that goes through JSON-RPC tests the protocol, not the
answer. The answer is the part that can be wrong.
**Method:** `Analysis` takes a file and a byte offset and returns a completion
list, a hover, or a definition. The `server` module converts positions and
speaks the protocol.
**Consequence:** Twelve tests ask `Analysis` directly, and five fixtures compare
its rendered answer.

## D093 - The server uses raw JSON, not a types crate
**Status:** settled.
**Reason:** Rule C-4.3 needs a reason for each dependency. `lsp-types` is a
large surface that changes between versions, and the server needs six methods.
**Method:** `lsp-server` carries the framing and the connection. The six method
bodies build their JSON directly.
**Revisit:** If the server grows past a handful of methods.

## D094 - A cursor at the end of a name means that name
**Status:** settled.
**Reason:** A reader who puts the cursor after `preston` asks about `preston`.
The token at that offset is the one after it.
**Method:** Hover and go to definition take the identifier that the cursor
touches, or the one immediately before it. Completion takes the token before,
because it asks what comes next.

## D095 - Only the include lines go to the preprocessor
**Status:** settled.
**Reason:** Rule C-1 sends a translation unit to `cc -E`. Sending the whole
Lark file does not work, because Lark syntax is not C, and the preprocessor
would carry Lark declarations into the name table. The header names are the
only part that the front end needs.
**Method:** `lark_cpp::read` gathers the `#include` directives, writes them to
one generated C file, and preprocesses that file. The Lark source never reaches
the preprocessor. The emitted C keeps every directive, so the real compile sees
the headers a second time. That is rule C-3.
**Consequence:** A conditional include cannot change with a macro that the Lark
file defines. No Lark program can define a macro, so nothing is lost today.

## D096 - A macro name counts as a name in the table
**Status:** settled.
**Reason:** A header names much of its interface with a macro. `stdout` is a
macro for `__stdoutp`, and `EOF` and `NULL` are macros with no declaration at
all. A table built only from declarations would call `stdout` unknown, and rule
L-15 would then read a complete table that is missing real names.
**Method:** The preprocessor runs with `-dD`, which keeps every `#define` in the
output. The collector scans those lines and records the macro names as values.
**Rejected:** A second run with `-dM`. One run gives both answers.

## D097 - The header reader is a trait, not a dependency
**Status:** settled.
**Reason:** `lark-resolve` must run no process. The language server and the
test harness need name resolution without a compiler on the path, and a crate
that spawns `cc` cannot promise that.
**Method:** `lark_resolve::HeaderReader` is a trait with one method. `NoHeaders`
reads nothing and keeps the table incomplete. `lark_cpp::Reader` implements the
trait, caches by include set, and the build, the test harness, and any tool that
wants real headers pass it to `resolve_with`.
**Consequence:** `lark-cpp` depends on `lark-resolve`, not the other way round.

## D098 - A record body opens a scope
**Status:** settled.
**Reason:** C11 6.2.3 puts a struct member in its own namespace. The parser
declared a field name in the enclosing scope, so `struct node { int value; }`
made `value` a value name for the rest of the file. A later `value v;` then
read as an expression, and a valid program failed to parse.
**Method:** `struct_body` pushes a scope and truncates it at the closing brace.
**Consequence:** A field never hides a type of the same name. The fixture
`tests/corpus/c11/typedefs.c` covers it.

## D099 - The file itself is a scope
**Status:** settled.
**Reason:** The parser opened no scope for a file, so every top level `declare`
was dropped. Nothing showed it, because a build supplies the names through the
oracle instead. A single file with a typedef and a cast to it did not parse.
**Method:** `source_file` pushes a scope before the first item.
**Consequence:** A file that declares a type and casts to it parses on its own,
with no oracle. That is what a preprocessed header does.

## D100 - Every record with a body gets a descriptor
**Status:** settled.
**Reason:** The emitter gave a record with no `gc` field the shared descriptor
`lark_bytes_type`, which reports a size of one byte. `lark_new` copies `size`
bytes from the initializer, so `new Plain { .label = "x", .first = 11 }` copied
one byte and left every other field unset. A record whose first field was a
pointer produced a truncated pointer, and the program crashed on the first use.
A record whose first field was a small integer appeared to work, which is why
the fault survived phase 6.
**Method:** The emitter writes a `lark_typeinfo` for every record with a body.
A record with no `gc` field gets an empty field map and its true `sizeof` and
`_Alignof`. `lark_bytes_type` now serves only an allocation with no named
record behind it.
**Consequence:** Each record costs one static descriptor. The fixture
`tests/exec/record_size.lark` allocates a record with four unmanaged fields,
collects, and reads every field back, in all four configurations.

## D101 - A module macro is a name in its own table
**Status:** settled.
**Reason:** Decision D095 sends only the `#include` lines to the preprocessor,
so a module is never preprocessed. A name that the module defines with
`#define` is then unbound. With a complete table, rule L-15 reads an unbound
name after `<` as the start of a generic argument list, so
`for (size_t i = 0; i < CLASS_COUNT; i += 1)` failed to parse. Five valid lines
of the Lark collector broke this way.
**Method:** The collector scans the module for its own `#define` directives and
records each name as a value in the module name table.
**Consequence:** A macro hides nothing, because a module name is checked first.

## D102 - An include stands at the top of the emitted C
**Status:** settled.
**Reason:** The emitter writes forward declarations before the module body, and
the body held the `#include` lines where the programmer wrote them. A forward
declaration such as `static int sum(const int *values, size_t count);` then
named `size_t` before `<stddef.h>` declared it, and the C compiler rejected it.
Every plain C file with a function that takes a library type hit this.
**Method:** The prologue writes every `#include` of the module, and the body
walk skips those directives. Each one appears exactly once, so a header with no
include guard is still included once.
**Consequence:** The line of an `#include` in the emitted C differs from its
line in the source. Every other line keeps its `#line` mapping.

## D103 - An old style definition gets no prototype
**Status:** settled.
**Reason:** C11 6.9.1 allows `int add(a, b) int a; int b; { ... }`. The emitter
writes a forward declaration for every function definition, and a prototype
built from that declarator reads `int add(a, b);`, which is not valid C.
**Method:** A definition with a `KR_PARAM_LIST` gets no forward declaration.
The definition itself declares the function, as C11 says.
**Consequence:** An old style definition must appear before its first call, as
it must in C.

## D104 - A directive splices over a carriage return
**Status:** settled.
**Reason:** The lexer reads a `#define` that ends in a backslash as one token
across the line break. It consumed two characters for the splice, which is
right for a file with Unix line endings and wrong for a file with Windows line
endings, where the splice is a backslash, a carriage return, and a newline. The
newline then ended the directive, and the continuation line parsed as code. One
libxml source file reported sixty errors from a single macro.
**Method:** The splice consumes the backslash, then a carriage return if one
follows, then a newline if one follows.

## D105 - A header prototype keeps a symbol external
**Status:** settled.
**Reason:** Rule X-5b marks every definition that no `export` names as `static`.
That is right for a Lark module. It is wrong for a plain C file, where the
matching header already declares the symbol without `static`, so the C compiler
reports a static declaration after a non static one. Seventeen of seventeen
files of one real C library failed this way.
**Method:** A definition whose name an included header declares keeps external
linkage. Phase 12 already reads every header, so the emitter has the answer.
**Consequence:** A C file that publishes its interface through its own header
keeps that interface. A Lark module names nothing in a header, so rule X-5b
still applies to it.

## D106 - The emitted C has a fixed order
**Status:** settled.
**Reason:** A forward declaration can name a type from a header, as in
`static int sum(const int *v, size_t n);`, and it can name a type that the
module itself declares, as in a callback typedef. The emitter wrote the forward
declarations first, so both failed.
**Method:** The prologue writes every `#include`, then every local `typedef`,
then the forward declarations. The body walk skips a directive and a typedef
that the prologue already wrote, and marks the place with a comment.
**Consequence:** The order of the emitted C differs from the order of the
source for those two kinds of item. Rule L-8 already makes a module order
independent, so nothing is lost.

## D107 - Three collectors ship, not one
**Status:** settled.
**Reason:** Chapter 10 defines a seam between the core and the collector. One
collector behind a seam proves nothing, because the seam can have grown around
that collector without anyone noticing. A second collector with the opposite
properties tests the seam rather than the collector.
**Method:** `arena` allocates and never frees, which is the smallest plugin the
seam accepts. `semispace` moves every object, which needs the address of each
root and the field map of each object, and so exercises the parts of the seam
that a non moving collector never touches. `gc.strategy` names the one that a
build links, and the runtime test suite runs against all three.
**Consequence:** The suite became capability aware. A test that asserts
reclamation skips under `arena`, and a test that asserts a stable address skips
under `semispace`. Each skip names the capability that it wanted.

## D108 - A capability decides what a test runs
**Status:** settled.
**Reason:** A collector supplies a different set of capabilities, and a test
that asks for one the collector lacks is not a failure. Marking such a test as
expected to fail loses the reason.
**Method:** `lark_gc_caps` gained `reclaims`, so a collector states whether a
collection frees what nothing reaches. The runtime harness gained
`SKIP_UNLESS(condition, reason)`, and it reports the skipped count beside the
failed count. A Lark fixture states a need in its header, as
`// needs: interior-pointers`, and the harness leaves out every configuration
whose collector lacks it.
**Consequence:** Adding a collector needs no edit to a test that does not apply
to it. Rule R-1 gives the transpiler the same answer at build time.

## D109 - The allocation runs before the initializer
**Status:** settled.
**Reason:** The emitter wrote `lark_new(&ti, &(Node){ .next = head })`. The
compound literal is an argument, so it reads `head` before `lark_new` runs. An
allocation is a safepoint, so a collector that moves objects moves the object
that `head` names, and the field then holds the address it had before the move.
Under a non moving collector the address stays valid, which is why the fault
survived until a moving collector existed. The fixture `tests/gc/churn.lark`
printed 80 rather than 2000.
**Method:** Rule M-28. The emitter allocates first and assigns the initializer
afterwards, which is what the worked example in chapter 09 already showed.
**Consequence:** One more comma expression per `new`, and the emitted C states
the order that the language needs.

## D110 - A nested allocation is hoisted
**Status:** settled.
**Reason:** Rule M-28 alone is not enough. `new Pair { .first = new Pair { } }`
allocates the outer object, then allocates the inner one while building the
initializer, then stores through the outer address. C does not fix the order in
which the two sides of an assignment are evaluated, so the emitted C can compute
the destination before the inner allocation moves it. The fixture
`tests/gc/nested_new.lark` crashed under the moving collector.
**Method:** Rule M-28a. The emitter finds every `new` inside an initializer and
emits it first, each into its own temporary slot. The initializer then reads
slots alone, so no safepoint stands between an allocation and its stores.
**Consequence:** A `new` with nested allocations uses one temporary slot per
allocation, which rule M-27 already required.

## D111 - Rule R-1 is a check, not a promise
**Status:** settled.
**Reason:** Chapter 10 said the transpiler reads the collector capabilities at
build time and enforces the source rules that depend on them. Nothing did. A
program that used an interior pointer under the moving collector compiled, and
then read a stale address at run time. The test harness knew the answer through
`// needs:` and the compiler did not.
**Method:** `lark_types::caps::Capabilities` mirrors `lark_gc_caps`, and the
driver reads it from `gc.strategy`. `lark_types::interior` finds every
construction of an interior pointer and reports `LK0320` when the collector
lacks the capability. The driver rejects an unknown collector name and refuses
`gc.roots = "conservative"` under a moving collector, both before any pass runs.
**Consequence:** The capability table exists twice, once for the transpiler and
once for the runtime. A test compares the two, so they cannot drift.

## D112 - A record keeps the keyword that declared it
**Status:** settled.
**Reason:** Rule X-8 emits a `typedef` for every record. The emitter wrote
`typedef struct` for all of them, so a top level `union` produced
`typedef struct Value Value;` and the C compiler reported a tag type that does
not match. An `enum` got no typedef at all. No test covered either, because no
fixture declared a top level union or enum.
**Method:** `Record` carries a `Keyword`, and the typedef repeats it. An enum
joins the record table for its typedef and stays out of the descriptor table,
because nothing allocates one.

## D113 - Rule M-18 is a call graph analysis
**Status:** settled.
**Reason:** Rule M-16 puts a poll at every loop back edge, and rule M-18 takes
it back out of a function that cannot reach an allocation. The emitter polled in
every loop of any module that used the runtime, so a function of pure arithmetic
paid a load and a branch per iteration.
**Method:** `lark_codegen::reach` reads the whole program once. A function
allocates when it holds a `new`, calls a function that allocates, makes an
indirect call, calls a method through an interface, or calls a name that no
module defines and no foreign marker describes. The pass then runs to a fixed
point over the call graph.
**Consequence:** The analysis is conservative in one direction only. A function
that it marks might not allocate. A function that it clears never allocates.

## D115 - An exported signature must have a C form
**Status:** settled.
**Reason:** Rule C-9 said a Lark function that C code calls must not take an
interface value and must not take a `managed struct` by value. Nothing checked
it. An interface value is two words with no C name, and a managed struct by
value copies the payload and leaves the header behind, because rule M-4 puts the
header at a negative offset. The copy is then no longer a managed object, and
nothing said so.
**Method:** `LK0440`. The boundary checker reads every exported declaration at
file scope and reports a parameter whose type names an interface or a marked
record, with no pointer. A pointer to either is an ordinary C pointer, which
rule C-10 already states.
**Consequence:** A private function keeps both forms, because C never calls one.

## D116 - An error inside a generic names the instantiation
**Status:** settled.
**Reason:** Rule G-4 makes the absence of constraint syntax the whole of version
1 constraint reporting: a type error appears after substitution, and the
diagnostic says which use caused it. Rule DQ-2 says the same thing for any
diagnostic from an instantiation. The checker reported the body location alone,
so a reader saw an error in a generic and no reason for it.
**Method:** `lark_driver::generics::attribute` runs after the type checks. A
diagnostic whose span lies inside a generic declaration gains a secondary label
per instantiation, up to three, and a note citing rule G-4. It adds a label
rather than a diagnostic, so rule DQ-4 still holds: one problem, one report.
**Consequence:** The monomorphization pass now runs before the type checks
rather than after them, because the attribution needs the instantiations.
A type error that only the C compiler detects still reports through `cc`, with
the `#line` mapping that rule X-3 gives.

## D117 - A generated file name carries `.lark.`
**Status:** settled.
**Reason:** A module emitted `<name>.h`, and the emitted C included it by that
name. A C file `attribute.c` with its own `attribute.h` then lost its header to
the generated one, and every type in it disappeared. The compiler reported a
missing type rather than a shadowed file, so the cause was invisible. Testing
the gumbo library needed every file renamed to work around it.
**Method:** Rule X-4b. `lark_codegen::names::header_file` builds the name once,
and every caller uses it. The compiler receives `-iquote` for the build
directory and then for each source directory, so a source header keeps its name
whatever the module is called.
**Consequence:** The gumbo library builds under its own file names, with no
renaming. The emitted `.h` of every fixture changed name, so five golden files
changed with it.

## D118 - A scratch directory starts empty
**Status:** settled.
**Reason:** The execution harness reused a scratch directory per fixture and per
configuration, and never cleared it. Decision D117 renamed the emitted header,
so the old name stayed behind, and `-iquote` on the scratch directory found the
stale generated header before the real one. A passing test hid a real failure.
**Method:** The harness removes the directory before it writes anything.
**Consequence:** A run costs one directory removal. A stale file from an earlier
shape can no longer answer a lookup.

## D119 - An index entry pins a commit
**Status:** settled.
**Reason:** A package manager that resolves a version to a tag inherits the
trust of whoever controls the tag. A tag moves, and a moved tag changes what a
build compiles without changing anything the project wrote. An index that pins
a commit removes that: the hash names one tree and cannot name another later.
**Method:** Rule K-3. `lark_pkg::index::Entry::parse` refuses an entry whose
`commit` field is not forty hexadecimal characters. A short hash is refused
too, because it is ambiguous.
**Consequence:** A direct git dependency still names a tag, and rule K-5 warns
once, because that path has no index to trust instead. The lock file records
what the tag pointed at, so the build still repeats.

## D120 - Git is a subprocess, not a library
**Status:** settled.
**Reason:** Rule C-4.3 needs a reason for each dependency. A git
implementation in Rust is a far larger surface than the six operations that the
manager needs: clone, fetch, reset, checkout, ls-remote, and rev-parse.
Decision D005 shells out to `cc` for the same reason.
**Method:** `lark_pkg::store` runs `git` and reads its output. A failure
carries the command and what git wrote, so a reader sees the real message.
**Consequence:** Credential handling, proxies, and ssh keys are whatever the
user already configured for git. The manager adds nothing and breaks nothing.

## D121 - Resolution reads an index through a trait
**Status:** settled.
**Reason:** Version resolution is the part most likely to be wrong, and it is
the part hardest to test if it needs a network. A resolver that clones a
repository to answer one question cannot be tested cheaply.
**Method:** `lark_pkg::resolve::Reader` has two methods: read an entry, and
read the dependencies of a resolved package. The tests supply an index built in
memory, so ten resolution tests run with no git at all. `lark_pkg::store`
supplies the real reader, and the end to end tests use local git repositories
that the test itself creates.
**Consequence:** The resolver holds no path and no url. Every test of it is
fast and reproducible.

## D122 - The resolution loop has a bound
**Status:** settled.
**Reason:** The resolver repeats until the choice for every package stops
changing. Every requirement narrows the choice, so it settles. A defect in that
argument turns into a hang, which is the worst failure a build tool has.
**Method:** A step count with an upper bound. Passing it returns
`Error::DidNotSettle`, which says that the fault is in the resolver rather than
in the project.

## D123 - A cache key names content, never a timestamp
**Status:** settled.
**Reason:** A build that compares timestamps asks whether a file is newer than
its output. That answer is wrong whenever a clock moves, a file is restored
from a backup, or two branches share a directory. A key built from the content
of every input is right in all three cases, and it needs no invalidation step:
an entry that no key names is never read.
**Method:** Rule Y-1. `lark_cache::Fingerprint` takes each input with a label
and a length, so two inputs of the same bytes under different names differ, and
two values never run together into a third. The key is a hash, and it is also
the file name of the entry, so a directory of entries needs no index.

## D124 - A witness records content, not a length and a time
**Status:** settled.
**Reason:** Some inputs are files that a subprocess reads on its own, and the
build learns their names only after the run. A record of the length and the
modification time misses an ordinary edit: `#define VALUE 7` to
`#define VALUE 9` changes neither, and two builds inside one second see the
same time. The stress test found exactly that, and it printed a program built
from a header that no longer existed.
**Method:** Rule Y-2. A witness holds a hash of the content.
**Consequence:** A build hashes every header that a compile read. That costs a
first build about half again as long, and it is what makes a second build
trustworthy. Rule Y-1 says which way that trade goes.

## D125 - A generated file is never a witness
**Status:** settled.
**Reason:** The first object cache never hit. The dependency list that `-MD`
writes names the source file itself, and the build rewrites every emitted `.c`
on every run, so its timestamp always differed and every entry missed.
**Method:** A file under the output directory is left out of the witness list.
Its content is already in the key, which is the stronger check.

## D126 - The cache needs no model of the dependency graph
**Status:** settled.
**Reason:** A generic has no C form of its own, so rule G-1 emits one copy per
instantiation in the module that declares the generic. The output of that
module then depends on what other modules ask for, and the dependency runs
backwards from the usual direction. A cache that models the graph has to model
that too, and a wrong model is a wrong program.
**Method:** The key holds the emitted C. A new instantiation changes the
emitted C of the module that owns the generic, so its object rebuilds. An
unrelated edit elsewhere does not change it, so its object is reused. The cache
never reasons about who depends on whom.
**Consequence:** The front end runs over every module on every build, because
rule L-8 and rule G-1 both need it. The cache saves the emit and the compile,
which is where the time goes once the front end is in release form.

## D127 - Compiles run at the same time, in batches
**Status:** settled.
**Reason:** One translation unit reads no output of another, so nothing orders
them. A build that compiles them in turn leaves most of a machine idle.
**Method:** Rule Y-6. `compile_all` runs a batch per group of processors inside
a scoped thread block. The link waits, because it reads every object. The
object file name carries the key, so two units never write one file.
**Consequence:** A cold build of the tour dropped from 0.60 to 0.48 seconds. A
race in the cache would appear as a differing run, so one test builds ten
modules from an empty cache ten times and compares every output.

## D128 - The formatter has one style and no options
**Status:** settled.
**Reason:** An option set turns every project into an argument about the option
set. The value of a formatter is that the argument stops, and an option is the
one feature that takes that value away.
**Method:** Rule Z-1. `lark fmt` rewrites a file, and `lark fmt --check` names
a file that differs and changes nothing. A gate runs the second form.

## D129 - A space goes wherever two tokens would merge
**Status:** settled.
**Reason:** The style binds a step operator to its value, so `a++` has no
space. Applied to `a++ + ++a` that gives `a++ ++a`, which lexes as
`a ++ ++ a`. The program changed, and the property test found it on the first
run over the corpus.
**Method:** Rule Z-2. Before the formatter writes two tokens together, it lexes
the pair. If the result is not those two tokens, a space goes between them. The
check needs no table of operators, and it covers `/` before `*` as well.

## D130 - A layout choice never reads the whitespace of the source
**Status:** settled.
**Reason:** The rule that keeps `} else {` on one line looked at the next
token. A source written `}else{` had `else` next, and a source written
`} else {` had a space next, so the first pass and the second decided
differently. Formatting was not idempotent.
**Method:** Rule Z-3. Every lookahead skips trivia, so the second pass sees
what the first pass produced.

## D131 - The tree decides a pointer star and a generic angle
**Status:** settled.
**Reason:** A `*` is a pointer or a product, and a `<` is a generic list or a
comparison. A formatter with no types cannot tell them apart from the tokens
alone, and a guess would be wrong in ordinary code.
**Method:** The parent node kind answers both. Rule L-6 already made the
decision once, and the formatter reads the answer.
**Consequence:** The generic list must also bind tight for a second reason. The
parser splits a `>>` into two tokens to close two lists, and a space between
them would leave text that lexes as one token fewer.

## D132 - Lark ships no debugger
**Status:** settled.
**Reason:** Rule X-3 already emits `#line`, so `lldb` and `gdb` both show Lark
source and Lark line numbers with no help. The only thing missing is that a
`gc Person*` prints as an address. That gap is a formatter script, not a
debugger.
**Method:** Rule Z-5. One script for each debugger, in Python, reading the
object header that rule M-4 puts at a negative offset and the descriptor that
rule M-5 fills in. A build writes both beside the program.
**Consequence:** Rule Z-6 turns on debug information by default, because a
script needs it to name a local. The script name carries an underscore, because
a Python module name cannot hold a dash and `lldb` refuses one.

## D133 - The barrier performs the store
**Status:** settled.
**Reason:** A barrier that only records, and leaves the store to the caller,
gives two ways to write a field and one of them is wrong. A collector that
needs no barrier would then need a second path in the emitted C.
**Method:** Rule R-2. `lark_write_barrier(&slot, value)` writes the value and
records what it needs. A collector with no barrier leaves the function pointer
empty, and the core performs the store and returns.
**Consequence:** A call where none is needed is correct and only slower, so the
transpiler can be conservative about which stores get one.

## D134 - The barrier reads the field name, not the type of the base
**Status:** settled.
**Reason:** A store needs a barrier when the field holds a managed pointer.
Deciding that from the type of the base needs the full type of every
expression, which the emitter does not carry.
**Method:** The module knows every record and every field that carries `gc`.
A store whose target names one of those fields gets a barrier. The set is
per module rather than per type, so a field name that two records share gives a
barrier for both.
**Consequence:** A barrier is emitted where none is needed when two records
share a field name and only one is managed. Decision D133 makes that correct
and only slower.

## D135 - Generational comes before concurrent
**Status:** settled.
**Reason:** A concurrent marker needs the barrier that this phase adds, plus a
tricolour invariant, and it changes the stop the world protocol that rule M-26
states. Doing both at once makes a failure impossible to attribute to one of
them.
**Method:** `generational` stops the world. The barrier is the new work, and it
reaches the capability table, the emitter, and the test matrix.
**Revisit:** When a concurrent marker is worth its cost. The seam holds it.

## D136 - A test that holds a raw pointer is a test of one collector
**Status:** settled.
**Reason:** `test_a_deep_chain_survives` built a ten thousand link chain with
the tail in a plain local. It passed under every collector until the nursery of
the generational one filled during the loop, and the collection then moved the
tail. The test had assumed that no collection ran while it built.
**Method:** The tail lives in a root, and the loop reads it back after every
allocation. Generated code needs no such reload, because rule M-10 makes the
shadow stack slot the address of the local itself.
**Consequence:** A runtime test that keeps a pointer across an allocation now
states where the root is. That is what generated code does, so the test is a
closer model of it.

## D137 - A thread parks before it asks for the world lock
**Status:** settled.
**Reason:** `lark_collect` locked the world and only then set its own state. A
second thread that entered `lark_collect` while a collection was starting
blocked on that lock while it still said it was running. The collector waited
for it to park, and it waited for the lock. Both waited for ever.
**Method:** The thread sets its state to parked before it asks for the lock.
The write is outside the lock and the reader holds it, so a stale read costs
one wakeup and nothing else.
**Consequence:** The window closed. Only the generational collector reached it,
because only its allocation calls `lark_collect` often enough to hit a window
that narrow.

## D138 - A blocking call in a test enters the safe state
**Status:** settled.
**Reason:** `test_a_collection_waits_for_every_thread` joined four worker
threads with a plain `pthread_join`. The main thread then sat in that call
saying it was running, and a worker that needed a collection waited for it
until the test timed out. Rule M-19 already answers this: a foreign call that
blocks enters the safe state, and `examples/pthread.lark` marks `pthread_join`
as `gc_safe` for exactly that reason.
**Method:** Every join in the runtime tests is wrapped in `lark_enter_safe` and
`lark_leave_safe`, which is what generated code does.
**Consequence:** The tests model generated code more closely. A test that
blocks outside Lark now says so, and the collector can run while it waits.

## D139 - A collection count is a lower bound
**Status:** settled.
**Reason:** A test asserted that twenty explicit calls gave exactly twenty
collections. That holds only for a collector whose allocation never collects on
its own. A generational collector collects whenever a nursery fills, and the
worker threads fill one throughout the test.
**Method:** The assertion is that every explicit call collected, which is what
the test means.

## D140 - The editor extension is plain JavaScript
**Status:** settled.
**Reason:** A TypeScript extension needs a build step, a compiler
configuration, and a generated file that a reader never opens. The client is
two hundred lines that start a subprocess and register two commands. A build
step for that is a cost with no return.
**Method:** `editors/vscode/src/extension.js`. The file a reader opens is the
file that runs. The one dependency is `vscode-languageclient`, which every
language server client uses.

## D141 - Highlighting does not depend on the compiler
**Status:** settled.
**Reason:** A person who installs an editor extension wants the file to read
correctly at once. An extension that shows plain text until a compiler is on
the path looks broken, and the first impression is the one that decides whether
somebody tries the language.
**Method:** Rule Z-7. A `TextMate` grammar colours the file, and the language
server client is separate. A server that does not start is reported once, and
the editor stays usable.

## D142 - The grammar is generated from the lexer
**Status:** settled.
**Reason:** A grammar is a second list of every keyword, and a second list
drifts. A keyword that the lexer knows and the grammar does not shows as plain
text, which reads as a bug in the language rather than in the extension.
**Method:** Rule Z-8. The grammar was generated from the keyword table in
`lark_syntax::SyntaxKind::c_keyword`, and a test compares the two on every run.
A keyword added to the lexer fails that test until the grammar names it.

## D143 - A fetched dependency is not held to the style rules
**Status:** settled.
**Reason:** The extension brought `node_modules` into the tree, and the prose
check reported twenty nine problems in text that other people wrote. The style
rules in `docs/conventions.md` bind what this project writes.
**Method:** `scripts/lib-files.sh` and `scripts/check-prose.py` both skip
`node_modules`.

## D144 - A generic argument becomes C text before it is mangled
**Status:** settled.
**Reason:** `Box<Box<int>>` put the Lark spelling `Box<int>` in an emitted C
field, and no C compiler reads it. The pass discovered the inner instantiation
but never used its name. The same defect reached `auto held = new Box<int>{}`,
which wrote `Box<int>* held` from the inferred type.
**Method:** Rule G-1. `lark_mono::resolve` replaces every `Name<Args>` in a
type with the name of its instantiation, and it runs on itself, so a nest of
any depth resolves. The pass runs it before it mangles, and both call sites in
the emitter run it too, so a use and a definition agree by construction.

## D145 - An instantiation is emitted after the instantiations it holds
**Status:** settled.
**Reason:** `Pair<int, Box<int>>` holds a `Box<int>` by value. C needs the
complete type before the field, and the name order that the sort gives is not
that order, so the emitted header named an incomplete type.
**Method:** Rule G-1 with rule X-6a. `order_instances` moves each instance
after every instance that its arguments name. Rule G-8 bounds the depth, so a
cycle cannot form.

## D146 - A generic instantiation gets a descriptor even with no managed field
**Status:** settled.
**Reason:** `new Box<int>` used `lark_bytes_type`, which says one byte, so the
allocation was one byte and the initializer wrote four. This is decision D100
again, for an instantiation rather than for a plain record.
**Method:** Rule M-5a with rule G-13. The emitter writes a `lark_typeinfo` for
every record instantiation. An instantiation with no managed field gets an
empty field map, and the size is still `sizeof` the instantiation.

## D147 - A managed parameter is a root
**Status:** settled.
**Reason:** A function that takes a `gc T*` and allocates lost the parameter
under a collector that moves. The frame held only the locals of the body, so
the callee kept the address that the caller passed, and every object it wrote
after the move pointed at free space. A stress program read 9 rather than 22.
**Method:** Rule M-10a. The frame plan reads the parameter list, and the
prologue registers every managed parameter right after the push. Rule M-18
already says which functions can reach an allocation, and a function that
cannot roots no parameter, so constraint D-1 still holds.

## D148 - The `gc` specifier of a function qualifies the return type
**Status:** settled.
**Reason:** `gc Node* make(int n)` was rejected with LK0200, because the
declarator builds a function type and rule T-2 found no pointer. That left no
way to write a function that returns a managed pointer.
**Method:** Rule T-1b. `apply_gc` descends through a function type into the
result, the same way a C qualifier does.

## D149 - A managed record puts the runtime header in its own header
**Status:** settled.
**Reason:** A module that declares `managed struct Circle` but never allocates
had `uses_runtime` false, so its generated header named `lark_typeinfo` with no
declaration for it. Any module that imported it failed to compile.
**Method:** Rule M-5a. `module_uses_runtime` is true when the module declares a
record with the `managed` marker. A plain C module declares none, so constraint
D-1 still keeps it free of the runtime.

## D150 - A `new` reads the descriptor of the module that owns the record
**Status:** settled.
**Reason:** `new geometry::Circle { ... }` read the record name as `geometry`,
so the emitted C named a type that does not exist. The descriptor lookup also
searched only the current module, so a record from another module fell back to
the one byte `lark_bytes_type`.
**Method:** Rules N-4 and X-5. The emitter carries the records of every other
module. A qualified name splits into the owner and the record, the descriptor
comes from the owner, and only the record name reaches the emitted C. The
inferred type of `auto` drops the module part the same way.

## D151 - A test suite can hold its own library modules
**Status:** settled.
**Reason:** A fixture that imports a second module had nowhere to put it. Every
`.lark` file under `tests/exec` is a fixture, so a library file ran as a test
and failed for want of an entry point.
**Method:** A file under `<suite>/modules/` is a library, not a fixture. The
runner puts that folder on the search path beside `examples/`.

## D152 - A build sets its own optimization level
**Status:** settled.
**Reason:** Every `lark build` passed no `-O` flag, so every binary was
unoptimized and no project could produce a release build. The gap appeared as
soon as a benchmark measured anything.
**Method:** Rule F-5. `build.opt` becomes `-O<level>`. The default is `"0"`,
which matches the default `build.debug = true`, so the plain build stays a
debug build. The level joins the settings that rule F-2 records and the key
that rule Y-2 gives an object, so a change to it rebuilds.

## D153 - A configuration field can be set on the command line
**Status:** settled.
**Reason:** Rule F-1 said every field has a command line flag, and no code read
one. Building the same source against four collectors needed it.
**Method:** `--section.field=value` before the file name. The override merges
into the parsed document and the result goes through the same deserialization,
so an unknown path gives the same error as an unknown field in the file.

## D154 - A collector sizes its next collection from the last one
**Status:** settled.
**Reason:** `precise-marksweep` held a fixed one megabyte trigger. A live set
above it collected on every allocation, and every one of those collections
freed nothing. A list of 100,000 cells took 69 seconds and ran 67,519
collections. The generational old space was a fixed one megabyte and aborted
when a program promoted more than that.
**Method:** Rule R-6. Marksweep sets the next limit to twice the heap that the
sweep left. Semispace reserves twice what a collection can copy. The
generational old space grows through a major collection, and the nursery
follows a share of it. The same list now takes 3 milliseconds and 2
collections.

## D155 - A space that holds live objects never moves under the program
**Status:** settled.
**Reason:** The semispace `grow` copied the whole from space to a new address
with `memcpy` and rewrote no pointer. Every root and every field then named an
address that held nothing. A benchmark read wrong values and then hung. The
comment in the function claimed the roots were the only pointers into the
space, which was not true.
**Method:** Rule R-7. The collector enlarges the empty destination and lets the
collection copy into it, because a collection already rewrites every root and
every field. The generational collector does the same through its reserve.

## D156 - An allocator remembers where it stopped
**Status:** settled.
**Reason:** Marksweep searched for a free block from block zero of the first
page on every allocation. After a sweep the free blocks were spread through the
list, so each allocation walked every full page ahead of them. The `churn`
benchmark took 5.7 seconds where the other collectors took 50 milliseconds.
**Method:** A cursor per page for the block, and a cursor per size class for
the page. A sweep clears both, because it frees blocks anywhere and it can free
a whole page. The same benchmark now takes 64 milliseconds.

## D157 - A managed local in a `for` initializer moves into a block
**Status:** settled.
**Reason:** `for (gc Cell* walk = head; ...)` emitted the slot registration as
two statements inside the loop header, which gave the `for` four clauses. No C
compiler read it.
**Method:** Rule M-11. The declaration and its registration move ahead of the
loop, inside a block that keeps the name local to the loop. A comma expression
cannot do the same, because a declaration takes no comma expression, and
registering inside the initializer would put the slot in the frame before the
local has a value.

## D158 - The benchmarks run in the gate
**Status:** settled.
**Reason:** Every defect above was invisible to the test suite. The correctness
tests set a 64 megabyte heap so that each test decides when the collector runs,
so no test ever reached a growth policy.
**Method:** `benchmarks/run.sh --quick` runs in the gate. It compares no
timings, because a shared machine gives no stable number. It checks that every
benchmark builds against every collector and that the four collectors return
the same checksum for the same work. `runtime/tests/test_growth.c` covers the
policies directly, and it starts the runtime at the default size rather than at
the size the other tests ask for.

## D159 - A lint override names one file, not the whole workspace
**Status:** settled.
**Reason:** The workspace allowed `module_name_repetitions` and
`must_use_candidate` for every crate. The first one fired nowhere, so the entry
hid nothing and said nothing. The second one hid 144 real suggestions.
**Method:** Both entries are gone. Every function that returns a value with no
side effect carries `#[must_use]`, so a caller who drops the result gets a
warning. Decision D041 no longer holds.

## D160 - A glob import stays only where the list is unusable
**Status:** settled.
**Reason:** Twenty two files carried `#![allow(clippy::enum_glob_use)]`. Three
of them had no glob import at all, so the suppression was stale. Most of the
rest used fewer than twenty of the 184 kinds, so the list was short enough to
write out.
**Method:** Sixteen files now name the variants they use. Seven keep the glob,
and each one walks 31 kinds or more. The comment on each states the count, so a
reader checks the claim rather than trusts it.

The used set came from the source text, matched against the real variant list.
A compiler round trip cannot find it: a variant in a pattern position binds
silently rather than failing to resolve, which turns a match arm into a catch
all.

## D161 - A public function returns a type that a caller can name
**Status:** settled.
**Reason:** `SourceMap::add` returned `Result<SourceId, FileTooLarge>`, and
`lark-span` exported neither `FileTooLarge` nor `MAX_SOURCE_LEN`. A caller
could not match on the error, store it, or name it. The documentation check
found it as a link to a private item.
**Method:** Both are exported. `cargo doc` with `-D warnings` runs in the gate,
so the next one fails the build rather than reaching a reader.
## D162 - An installed binary finds the runtime beside itself
**Status:** settled.
**Reason:** The runtime search read `build.runtime`, `LARK_RUNTIME`, and two
paths relative to the project. A user who unpacked a release archive had none
of those, so every managed program failed until they set a variable by hand.
**Method:** The search also looks beside the running program: `../runtime`,
`../share/lark/runtime`, and `runtime`. A release archive holds `bin/lark` and
`runtime/`, so it works after one unpack.

## D163 - A release archive proves itself before it ships
**Status:** settled.
**Reason:** A missing runtime file breaks every managed program, and no unit
test finds it, because the tests run inside the source tree where the runtime
is always present.
**Method:** Rule C-7.2. The release workflow unpacks each archive outside the
source tree and builds a managed program with it. The four targets each build
on their own machine, so no build cross compiles.

## D164 - A nested allocation takes one temporary slot, not one per level
**Status:** settled.
**Reason:** `hoist_nested_allocations` collected every nested `new` in one
list, then recursed into each one. The recursion hoisted the deeper entries,
but the list already held them, so the loop hoisted each a second time. Four
allocations took seven slots in a frame sized for four, which wrote 24 bytes
past the array and into the stack canary. The emitted C also allocated three
objects that nothing read.

Clang did not report it and the program printed the right answer. The first
run on Linux reported `stack smashing detected` under every collector.
**Method:** Rule M-27 and rule M-28a. The loop checks the map again for each
child, because the recursion fills it while the loop runs.

Every fixture now compiles with `-fstack-protector-strong`, so the same class
of defect fails on every platform rather than on one. Reverting the fix makes
`gc/nested_new` fail on macOS as well.

## D165 - A POSIX function needs its feature macro
**Status:** settled.
**Reason:** `runtime/tests/test_contract.c` called `setenv` and `unsetenv`
with no feature macro. Apple libc declares them anyway. Glibc hides them under
`-std=c11`, so the first run on Linux failed with an implicit declaration and
`-Werror` stopped the build.
**Method:** `#define _POSIX_C_SOURCE 200809L` before every include.
`benchmarks/bench.lark` takes the same treatment for `clock_gettime`, which
worked on both platforms but rested on the same accident.

## D166 - A path dependency carries a version
**Status:** settled.
**Reason:** Every entry in `[workspace.dependencies]` gave a path and no
version. `cargo deny` reads that as a wildcard, and `deny.toml` denies
wildcards, so the audit failed on the first push.
**Method:** Each entry gives both. A registry needs the version as well, so a
crate that publishes later needs no further change.

## D167 - The conservative scan opts out of AddressSanitizer
**Status:** settled.
**Reason:** The sanitizer checks every load against the object that owns the
address. Rule M-13 makes `scan_words` read the machine stack word by word,
across every object and across the padding between them, so the two disagree
by design. Clang reported a stack buffer underflow and gcc reported a stack use
after return, and both described the same intended read.

The gate skips the sanitizer on a machine that cannot map its shadow memory,
so the first run in continuous integration was the first run at all.
**Method:** `__attribute__((no_sanitize_address))` on `scan_words` alone.
Every other function in the runtime keeps its instrumentation, so a real fault
still fails the build. The macro reads `__has_feature` for clang and
`__SANITIZE_ADDRESS__` for gcc, and it expands to nothing elsewhere.

The attribute stops the report and not the crash. `detect_stack_use_after_return`
puts a local on a heap fake stack, and rule M-13 takes the top of the scan from
the address of a local. The top then sits on the heap while the base sits on
the real stack, so the scan walks the unmapped memory between them and the
program stops with a fault. The runtime tests therefore run with that one
option off. Every other check the sanitizer makes stays on.

## D168 - Every compilation runs through clang
**Status:** settled.
**Reason:** The driver builds GCC style flags, and MSVC accepts none of them.
A Windows port therefore needed either a second flag dialect or a compiler that
takes the first one. One compiler on every platform is the smaller answer, and
it makes a build behave the same everywhere.
**Method:** Rule F-7. `build.cc` defaults to `clang`. A project that needs
another compiler names it, and the build reports what it ran.

The runtime suite runs a second time under gcc on Linux. Gcc reported the frame
overrun in decision D164 that clang accepted, so the diversity is worth one
extra job. On macOS `gcc` is clang, so that pass is Linux only.

## D169 - A generic return type reaches the frame
**Status:** settled.
**Reason:** `gc Vec<int>* make(void)` emitted the right signature and the wrong
temporary. `frame::return_type_of` read the name and dropped the argument list,
so the type read as `Vec*`, which names no C type. Decision D144 sent the
declaration and the `auto` paths through `lark_mono::resolve` and missed this
one.
**Method:** Rule G-1 and rule M-12. `return_type_of` keeps the argument list,
and `write_function` resolves the text the same way every other path does. A
qualified name keeps only its last segment, per rule X-5.

## D170 - `auto` reads nothing from a name or a call
**Status:** open. Planned for phase E2.
**Reason:** Inference covers a literal, a cast, a `new`, and an operator. A
call, a name, a field, and a method all fall to the error type. A declaration
written as `auto a = f()` therefore emits `auto`, and the C compiler rejects
it. This is not a regression. It never worked.
**Method:** Rules T-12 and T-13 need a name to type environment, which
`lark-types` does not carry. The resolver builds one for namespaces in the same
phase, so the two land together. A standard library needs it, because
`auto v = Vector::with_capacity(8)` is the shape collections use everywhere.

## D171 - The formatter indents a multi-line initializer
**Status:** settled.
**Reason:** An initializer list took no indentation and its `}` always joined
the last field. A nested list therefore flattened: three levels of `new Pair`
all sat at one depth, and every brace piled onto one line. Running the
formatter over the project sources made them harder to read, which is worse
than no formatter.
**Method:** Rule Z-1. Each open list records whether it broke a line. A short
list keeps the old shape, so `new Pair { .value = 1 }` stays on one line. A
list that broke a line indents its fields and puts the brace on a line of its
own.

Two more defects came out of the same run. A blank line inside a body vanished,
because the code allowed one only at depth zero while the stated rule said
"anywhere". A closing generic angle bound tight to whatever followed it, so
`struct Data<T> {` lost its space. Both are fixed.

## D172 - Three fixture groups stay out of the formatter
**Status:** settled.
**Reason:** A `parse` fixture holds a deliberate syntax error, and the
formatter has nothing meaningful to say about text that does not parse. A `ui`
fixture anchors each expected diagnostic to a line number. An `lsp` fixture
marks a cursor with `<|>`. Formatting any of the three moved what the fixture
points at, and twenty tests failed.
**Method:** `scripts/lark-sources.sh` lists the 40 files the formatter owns and
excludes the 22 it does not. The gate runs `lark fmt --check` over that list,
so the sources cannot drift.

## D173 - A card scan reads dirty cards, not every object
**Status:** settled.
**Reason:** `scan_marked_cards` walked every object of the old generation on
every minor collection, and tested each one against its cards. The cost
followed the size of the heap rather than the size of the change.
**Method:** A crossing map records where the first object of each card starts.
The scan walks the card table, joins a run of dirty cards, and reads only the
objects that the run covers. An object that spans several cards starts in an
earlier one, so a card with no start of its own reads from the nearest card
that has one.

The map is rebuilt by a major collection, which moves every object, and it
carries forward through a minor one, which appends.

Measured over three runs each, this took `trees` from 214 to 191 milliseconds,
`walk` from 226 to 207, and `overhead` from 495 to 403. A single run showed
nothing, because the difference sits inside the noise of one measurement.

## D174 - The nursery floor is eight megabytes
**Status:** settled.
**Reason:** The floor was 256 kilobytes. A minor collection costs what
survives, and a nursery fills at the rate the program allocates, so a small
nursery collects often and gains little each time. The `overhead` benchmark ran
1375 collections.
**Method:** The floor is eight megabytes. `overhead` now runs 30 collections.
The heap grows to about twice what the mark and sweep collector holds, which is
what a collector that copies needs anyway.

| Floor | overhead | collections |
|---|---|---|
| 256 KB | 404 ms | 1375 |
| 1 MB | 198 ms | 270 |
| 4 MB | 151 ms | 61 |
| 8 MB | 111 ms | 30 |
| 16 MB | 105 ms | 15 |

Sixteen megabytes reads faster and holds twice the memory, so eight is the
balance.

## D175 - The generational collector loses on a live set that never dies
**Status:** settled.
**Reason:** After D173 and D174 the collector is the fastest of the four on
`churn`, `barrier`, and `overhead`, and it beats `malloc` and `free` on the
last of those. It stays behind the mark and sweep collector on `trees` and
`walk` by about 30 percent.

That is the shape the design gives, not a defect. Both of those benchmarks
retain everything they allocate, so every collection copies the whole live set
and reclaims nothing. A collector that copies pays for what survives, and a
collector that marks and sweeps pays for what dies.
**Method:** No change. The benchmark table states the trade, and
`docs/guide/08-tools.md` says which collector suits which shape of program.

## D176 - An interface takes generic parameters
**Status:** settled.
**Reason:** `iface Seq<T>` did not parse. A standard library needs
`Iterator<T>`, `Eq<T>`, and `Hash<T>`, so collections were blocked without it.
**Method:** Rules O-25, O-26, and O-27. An instantiation of an interface joins
the ones that rule G-1 already builds for a record and a function, so the
monomorphizer gained one kind rather than a second mechanism.

The emitter expands each generic interface into one interface per
instantiation, with every parameter replaced, before anything reads the table.
Every name that the emitter builds then comes from the instantiation, so the
method table, the identity, the fat pointer type, and each thunk all agree.

Four places read a name that the expansion changed: the forward declarations,
the body of an implementation, the declaration of an interface value, and the
type of a local. Each one resolves the written `Seq<int>` to the instantiation
that rule X-5a names.

## D177 - The recognizer for `impl` skips an argument list
**Status:** settled.
**Reason:** The parser took `impl IDENT for` as the shape of an implementation.
`impl Seq<int> for Counter` did not match, so it fell through and parsed as a
declaration, which reported a syntax error at the first argument.
**Method:** Rule L-3 and rule O-26. The lookahead skips a balanced `<...>`
between the name and the `for`. The scan counts a `>>` as two closes, and it
stops at a token that no argument list holds, so a comparison never reads as a
list.

## D178 - A directory is a namespace segment
**Status:** settled.
**Reason:** A standard library is `std::collections::Vector`. A module name was
one word, and a qualified name took exactly one `::`, so neither the file
`std/collections.lark` nor the name that reaches into it could exist.
**Method:** Rules N-16, N-17, and N-18. The loader maps `::` to a directory
separator, so `@import std::collections` reads `std/collections.lark`. A path
in the grammar loops rather than taking one segment, so it reaches any depth.

Rule N-18 flattens the path for anything that C or a file system must hold.
`lark_mono::mangle::module_prefix` owns that rule, and every builder of a
generated name goes through it, so a file name and a symbol never disagree.

Four places read only the first segment of a path and now read all of it: the
generated include of a header, the same include in the emitted C, the C name
of a qualified use, and the descriptor lookup of a `new`.

## D179 - A namespace block names functions and variables, not types
**Status:** settled.
**Reason:** A block that held a type needed nine collectors to descend into it,
and every record tag, descriptor, and use to carry the path. The chosen design
put helpers in a block and types at file level, and a directory namespace
already names a type as `std::collections::Vector`.
**Method:** Rules N-19, N-20, and N-21. A block holds a function and a
variable, and each name it declares carries the path of the block in the
emitted C, so two blocks declare the same name without a collision. Diagnostic
LK0614 reports a type definition inside a block, and it names the fix.

The restriction is not permanent. A later phase can allow a type in a block,
and allowing more is never a breaking change. A reader today knows that
`a::b::Thing` comes from `a/b.lark`, with no second place to look.

Rule N-21 makes a sibling visible without a qualifier inside the block, and a
local shadows, because C already gave the local the name that the programmer
wrote.
