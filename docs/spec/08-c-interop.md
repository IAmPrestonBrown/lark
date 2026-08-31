# 08 - C Interoperation

## 1. The preprocessor

**Rule C-1.** Lark does not implement the C preprocessor. The front end invokes
the configured C compiler with `-E` and reads the result.

**Rule C-1a.** The front end preprocesses the `#include` directives of a module,
not the module itself. It writes the directives to one generated C file and
preprocesses that file. Lark source never reaches the preprocessor. See decision
D095.

**Rule C-1b.** The names that the preprocessed unit declares join the module
name table. A typedef name binds to a type, and a function, an object, an enum
constant, and a macro name each bind to a value. A module name wins a clash with
a header name. See rule N-12.

**Rule C-1c.** The preprocessor runs with `-dD`, so a macro name is part of the
table. `stdout` and `EOF` are macros with no declaration behind them, and a
table without them is not complete. See decision D096.

**Rule C-1d.** A module whose every `#include` was read has a complete name
table, which is what rule L-15 tests. A header that does not read leaves the
table incomplete and reports the problem.

**Rule C-1e.** A macro whose replacement list names a type binds to a type, not
to a value. `<stdbool.h>` defines `bool` as `_Bool`, and a program writes
`bool ready = 1;`. The replacement must hold only type keywords, known type
names, and pointer stars, so `#define LIMIT 8` stays a value.

**Rule C-2.** The front end keeps the original source text as well. Diagnostics
report positions in the file the programmer wrote, not in the preprocessed text.

**Rule C-2a.** A name that the module defines with `#define` joins the module
name table as a value. Lark does not preprocess a module, so such a name is
otherwise unbound, and rule L-15 would read `index < LIMIT` as a generic
argument list. See decision D101.

**Rule C-3.** A `#include` directive passes through to the emitted C unchanged.
Lark never re-emits a declaration that a header already provides.

**Rule C-3a.** Every `#include` of a module stands at the top of the emitted C,
before the forward declarations. A forward declaration can name `size_t` or
`FILE`, so the header comes first. The directive keeps its text, and it appears
exactly once. See decision D102.

Rule C-3 has a useful consequence. The emitted C keeps `#include <stdio.h>`, so
the linker resolves `printf` from libc with no work from Lark.

## 2. Extension tolerance

**Rule C-4.** The parser skips these token sequences and attaches no meaning to
them.

| Sequence | Source |
|---|---|
| `__attribute__ ((...))`, `__attribute ((...))` | GCC, Clang |
| `__asm ("...")`, `__asm__ ("...")` | GCC, Clang |
| `__declspec (...)` | MSVC |
| `__typeof (...)`, `__typeof__ (...)` | GCC |
| `__extension__` | GCC |
| `_Nullable`, `_Nonnull`, `_Null_unspecified`, `_Nullable_result` | Clang |

**Rule C-4a.** The parser reads such a sequence in each of these places. A real
header uses every one.

| Place | Example |
|---|---|
| Among the declaration specifiers | `__attribute__((noreturn)) void f(void);` |
| After a `*` in a pointer | `int (* _Nullable close)(void *);` |
| After a declarator | `int fopen(const char *) __asm("_fopen");` |
| After a record body | `struct s { char z[4]; } __attribute__((aligned(4)));` |
| After an enumerator | `enum e { a __attribute__((deprecated)) = 1 };` |

**Rule C-4b.** A name that spells a standard keyword with two leading
underscores lexes as that keyword. C reserves the spelling to the
implementation, so no program loses a name.

| Spelling | Keyword |
|---|---|
| `__restrict`, `__restrict__` | `restrict` |
| `__inline`, `__inline__` | `inline` |
| `__const` | `const` |
| `__volatile`, `__volatile__` | `volatile` |
| `__signed`, `__signed__` | `signed` |

**Rule C-4c.** A `^` in a declarator reads as a pointer. Clang writes a block
pointer this way, and Lark gives the form no meaning of its own.

**Rule C-4d.** `({ ... })` is a statement expression. Its value is the value of
the last statement in the block.

**Rule C-4e.** `__asm__`, `__asm`, and `asm` begin an inline assembly
statement. A qualifier can stand between the word and the group, as in
`__asm__ volatile ("nop")`. Lark reads the group and gives it no meaning.

**Rule C-4f.** `__typeof__(x)`, `__typeof(x)`, and `typeof(x)` name the type of
`x`. `__auto_type` names an inferred type. Each stands where a type specifier
stands, so the parser reads it rather than skipping it.

**Rule C-4g.** An argument of a call can be a type name. A macro such as
`va_arg(ap, int)` or `offsetof(struct S, m)` writes one, and Lark expands no
macro in a module. The tokens must start a type and the argument must end right
after the type, so an ordinary expression never takes this reading.

**Rule C-4h.** `&&label` takes the address of a label, and `goto *expr` jumps to
a computed label.

**Rule C-4i.** `[first ... last] = value` in an initializer designates a range
of array elements.

**Rule C-4k.** An unexpanded macro stands where an attribute stands. A module
is not preprocessed, so `static int PRINTF(2) name(void);` arrives with the
macro in place. The parser reads the name and its group when a name or a star
follows the group, because a declarator ends the declaration instead. The
emitted C keeps the text, and the C compiler expands it.

**Rule C-4j.** `extern "C" { ... }` is a linkage block, and `extern "C"` before
one declaration applies to it. A header writes this behind `#ifdef __cplusplus`.
Lark evaluates no directive, so the parser reads the block and gives the linkage
name no meaning.

**Rule C-5.** The parser recognizes `__builtin_*` as an ordinary function name.
Its declaration comes from the header or from a built in table.

**Rule C-6.** An unknown extension in declaration position produces a warning,
not an error, and the parser skips to the next declaration. A header must never
stop a build.

## 3. Extern declarations

**Rule C-7.** A function declaration with no body is an extern declaration. It
carries `gc_leaf` or `gc_safe`, or it defaults to `gc_safe` under rule M-21.

```c
gc_leaf void handle_opaque_data(void* data);
gc_safe void handle_gc_managed(gc void* data);
```

**Rule C-8.** A `gc_leaf` function must not take a managed parameter. Rule M-22
states this.

**Rule C-9.** A Lark function that C code calls must not take an interface value
and must not take a `managed struct` by value. Both have no C ABI form. Chapter
02 rule T-12 and chapter 04 cover them.

## 4. Data compatibility

**Rule C-10.** These representations match C exactly.

| Lark type | C representation |
|---|---|
| Every C11 type | Itself |
| `gc T*` | `T*` |
| A `managed struct` payload | The same plain struct |
| An enum, a union, an array | Itself |

**Rule C-11.** These representations have no C form.

| Lark type | Reason |
|---|---|
| An interface value | Two words, no C equivalent |

**Rule C-12.** A `managed struct` passed to C passes as a pointer to its
payload. The header stays reachable at a negative offset. C code must not free
it, must not reallocate it, and must not store the pointer past the call unless
the call is `gc_safe` and the object stays rooted.
