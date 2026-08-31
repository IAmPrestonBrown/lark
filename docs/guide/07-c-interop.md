# 7. Talking to C

Lark is a superset of C11, so calling C is not a feature. You include the
header and you call the function.

## Including a header

```c
#include <stdio.h>
#include "own_header.h"

int label_length(const Label *item)
{
    return item->length;
}

int main(void)
{
    Label one = { .text = "gumbo", .length = 5 };
    printf("label %s %d\n", one.text, label_length(&one));
    printf("limit %d\n", LABEL_LIMIT);
    return 0;
}
```

```
label gumbo 5
limit 32
```

Lark runs your preprocessor and reads what comes out, so a macro, a typedef,
and a compiler extension all work. The proof is the gumbo HTML parser, which
compiles through Lark under its own file names.

Your own header keeps its name. Lark writes `main.lark.h`, never `main.h`, so
a module named `own_header` can include `own_header.h` and get yours. Rule
X-4b.

## The two markers

A foreign call must say one thing: can a collection run while it runs?

```c
gc_leaf size_t strlen(const char* s);   /* no */
gc_safe int puts(const char* s);        /* yes */
```

The difference is in the emitted C.

```c
int n = (int) strlen("hello");                        /* nothing added */
(lark_enter_safe(), lk_leave__i(puts("hi")));         /* two calls added */
```

`gc_safe` parks the thread before the call and unparks it after. Another thread
can then collect while this one waits. `gc_leaf` adds nothing at all.

**A declaration with no marker is `gc_safe`.** Rule M-21 picks the safe
default, because a wrong `gc_leaf` is a hang and a wrong `gc_safe` costs two
calls. Write `gc_leaf` when you know the callee returns without blocking.

## A `gc_leaf` function takes no managed pointer

```
$ lark check bad.lark
error[LK0340]: a `gc_leaf` function cannot take a managed parameter
 --> bad.lark:2:19
  |
2 | gc_leaf void take(gc Buffer* b);
  |                   ^^ a leaf call has no safepoint, so this argument has no root
  |
help: mark the function `gc_safe`, or take a raw pointer
```

Rule M-22. A leaf call reaches no safepoint, so the runtime cannot pin what you
passed. Either mark the function `gc_safe`, or hand it a raw pointer and keep
the managed one rooted yourself.

## Passing managed memory to C

A managed object can move, and C knows nothing about that. Two ways to handle
it, and the collector you chose decides which you need.

**Choose a collector that does not move.** The default,
`precise-marksweep`, never moves an object, so a pointer you hand to C stays
valid. This is the simple answer, and it is the default for that reason.

**Or copy.** Allocate with `malloc`, copy into it, call, and free. The cost is
explicit and it works under every collector.

A `gc_safe` call keeps the object rooted while the callee runs, so the object
stays alive. Alive and unmoved are different promises, and only the collector
gives the second one.

## Calling Lark from C

The emitted C is ordinary C, and rule X-5 makes the names ordinary too.
`geometry::area` links as `area`. Include the generated header and call it.

```c
#include "geometry.lark.h"

int main(void) { return area(circle); }
```

A managed struct carries a header before its payload, so rule C-9 refuses to
pass one by value across the boundary. Take a pointer instead.

```c
export int area(gc Circle* c);     /* yes */
export int area(Circle c);         /* rejected, LK0440 */
```

## The rules you meet first

| Rule | What it says |
|---|---|
| C-1 | Lark reads your preprocessed headers. |
| C-9 | A managed struct does not cross the boundary by value. |
| M-19 | `gc_safe` means a collection can run during the call. |
| M-20 | `gc_leaf` means the callee blocks on nothing and allocates nothing. |
| M-21 | No marker means `gc_safe`. |
| M-22 | A `gc_leaf` function takes no managed parameter. |
| X-4b | The generated header never takes the name of yours. |
| X-5 | An exported name links under its own name. |

---

Next: [The tools](08-tools.md).
