# 2. Coming from C

Lark is a strict superset of C11. That claim is not a slogan. It means you can
rename a `.c` file to `.lark` and build it.

## Rename a file and build it

```c
/* sort.lark, a C file with nothing changed */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define LIMIT 8

typedef struct point { int x; int y; } point;

static int compare_int(const void *left, const void *right)
{
    const int *a = left;
    const int *b = right;
    return (*a > *b) - (*a < *b);
}

int main(void)
{
    int values[LIMIT] = { 5, 3, 9, 1, 7, 2, 8, 4 };
    point origin = { .x = 1, .y = 2 };
    qsort(values, LIMIT, sizeof(int), compare_int);
    printf("%d %d %d\n", values[0], values[LIMIT - 1], origin.x + origin.y);
    return 0;
}
```

```sh
$ lark build sort.lark && ./build/sort
1 9 3
```

Macros, designated initializers, function pointers, old style definitions,
`_Generic`, `_Static_assert`: all of it works. So do the compiler extensions
that fill every system header, because a header that does not read is a header
you cannot use.

The proof is not this example. It is the gumbo HTML parser, 32,979 lines across
seventeen files, which compiles through Lark under its own file names.

## No word is reserved

Every Lark keyword is **contextual**. Lark recognizes it only where valid C11
cannot parse. So a C program that uses `new` or `gc` as a name keeps working.

```c
int new = 1;
int gc = 2;
int managed = 3;

int impl(int export)
{
    return export + new + gc + managed;
}
```

That compiles and returns 10 for `impl(4)`. Rule S-2 promises it, and the
parser tests check every keyword as a variable, a function, a parameter, and a
field.

This is why the additions look the way they do. `new Person { ... }` reads as
an allocation because `new` sits before a type name in expression position, and
nothing else in C does. Rule L-3 gives the full recognition rule.

## What Lark adds

Four things, each with a marker you can see.

```c
@import stdio                   /* a module              */

managed struct Node {           /* an object header      */
    gc Node* next;              /* a managed pointer     */
    int value;
}

iface Show { ... }              /* an interface          */
impl Show for Node { ... }

struct Box<T> { T item; }       /* a generic             */

auto n = new Node { .value = 1 };   /* an allocation     */
```

There is no hidden allocation, no hidden dispatch, and no hidden write. A
function that can trigger a collection is a function you called with `new` in
it, or one you marked.

## What Lark leaves alone

| | |
|---|---|
| The machine model | A struct is a struct. Layout, padding, and alignment are C's. |
| `malloc` and `free` | They still work. Lark adds a collector, it does not remove C. |
| The preprocessor | Lark runs yours. It writes no macros of its own. |
| Undefined behaviour | Lark does not fix it. A cast is still a cast. |

Lark does not make C safe. It adds a collector for the memory you want managed
and leaves the rest to you. A `gc Person*` and a `Person*` are different types,
and rule T-5 refuses to convert between them without a cast, so you always know
which one you hold.

## Two things to know early

**A collected program needs an entry point.** Write `init` before it, usually
on `main`. That is where the runtime starts.

```c
init int main(void) { ... }
```

A program with no managed memory needs nothing: `sort.lark` above has no
`init`, and it never links the runtime.

**The emitted C is yours to read.** When you wonder what something costs, run
`lark emit` and look. That is the whole design: the cost is at the call site,
and if you doubt it, the C is right there.

---

Next: [Managed memory](03-memory.md).
