# 3. Managed memory

This is the page that matters. Everything else in Lark is convenience.

## Three markers

```c
managed struct Node {    /* this type carries an object header */
    gc Node* next;       /* this pointer is managed            */
    int value;
}

auto n = new Node { .value = 1 };    /* allocate one */
```

| Marker | What it says |
|---|---|
| `gc` on a pointer | The collector manages what it points at. |
| `managed` on a struct | The type carries an object header, so the collector can trace it. |
| `new` | Allocate in the collected heap. |

A struct with a `gc` field **must** say `managed`, because the collector needs
a field map for it. Rule T-4 reports the case, so you cannot forget.

## A working program

```c
@import stdio

managed struct Node {
    gc Node* next;
    int value;
}

int build_and_sum(int count) {
    gc Node* head = 0;
    for (int index = 1; index <= count; index++) {
        head = new Node { .next = head, .value = index };
    }

    int total = 0;
    gc Node* walk = head;
    while (walk != 0) {
        total = total + walk->value;
        walk = walk->next;
    }
    return total;
}

init int main(void) {
    stdio::printf("sum %d\n", build_and_sum(10));

    gc Node* items = new Node[4];
    for (int index = 0; index < 4; index++) {
        items[index].value = index * index;
    }
    stdio::printf("array %d\n", items[3].value);
    return 0;
}
```

```
sum 55
array 9
```

There is no `free`. The collector reclaims the list when nothing reaches it,
and `build_and_sum` allocates ten nodes per call with no bookkeeping.

## Where a managed pointer can live

Rule M-1 gives three places, and nowhere else.

| Place | Example |
|---|---|
| On the stack | `gc Node* head = ...;` inside a function |
| In a managed struct | `gc Node* next;` inside `managed struct Node` |
| In a `@global` block | See [page 6](06-startup.md) |

A managed pointer at file scope is an error, because nothing roots it.

```
$ lark check bound.lark
error[LK0310]: a managed pointer cannot live here
 --> bound.lark:2:1
  |
2 | gc Person* global_person;
  | ^^ a managed pointer at file scope has no root
  = note: rule M-1 allows one on the stack, in a managed struct, and in a `@global` block
  |
help: move the declaration into a `@global` block
```

That rule is what makes the collector able to find every root. A pointer it
cannot find is a pointer to an object it will free.

## What it costs

Run `lark emit` on the program above and read `build_and_sum`.

```c
struct { lark_frame_hdr h; void **s[2]; void *t[1]; } _lk_frame;
_lk_frame.h.slots = _lk_frame.s;
_lk_frame.h.temps = _lk_frame.t;
_lk_frame.h.nslots = 0u;
_lk_frame.h.ntemps = 1u;
_lk_frame.t[0] = 0;
lark_frame_push(&_lk_frame.h);

Node* head = 0; _lk_frame.s[0] = (void **)&head; _lk_frame.h.nslots = 1u;
```

That is the whole cost of managed memory in a function: a struct on the stack,
one slot per managed local, and a push and a pop.

A slot holds the **address** of the local, not its value. That is what lets a
collector move an object: it writes the new address through the slot, and your
local names the copy. You write no reload.

A loop that can allocate also carries a poll:

```c
for (...) { LARK_POLL(); ... }
```

`LARK_POLL` is a load and a branch. A function that cannot reach an allocation
gets none at all, which rule M-18 computes from the call graph.

## The rules you meet first

| Rule | What it says |
|---|---|
| M-1 | The three places a managed pointer can live. |
| M-4 | Every managed object carries a header, before the payload. |
| M-5 | The header names a descriptor, which lists every `gc` field. |
| M-10 | A managed local gets a shadow stack slot, holding its address. |
| M-16 | A poll goes at every loop back edge that can allocate. |
| T-4 | A struct with a `gc` field must carry `managed`. |
| T-5 | No implicit conversion between a managed and a raw pointer. |

Rule T-5 is the one that catches people. `gc Person*` and `Person*` are
different types. Converting needs a cast, and the cast is an assertion you are
making.

```c
gc Person* p = (gc Person*) raw;    /* you are asserting this is managed */
```

## Interior pointers

A managed pointer can address any byte inside an object.

```c
gc char* buf = new char[256];
gc char* mid = &buf[3];      /* legal, rule M-6 */
```

Whether that works depends on the collector you chose. A collector that moves
objects cannot follow one, and rule R-1 reports it at build time rather than
letting it fail at run time. See [page 8](08-tools.md).

## Choosing a collector

`lark.toml` picks one. The default reclaims, does not move, and follows
interior pointers, which is the fewest surprises.

```toml
[gc]
strategy = "precise-marksweep"
```

Four ship. [Page 8](08-tools.md) says when to use which.

## Testing that you got it right

Turn on torture mode, and every safepoint runs a full collection.

```toml
[gc]
torture = true
```

A correct program gives identical output either way. A program that loses a
root gives different output, or crashes, and it does so on the first run rather
than in a month. That one setting finds more root defects than any amount of
reading.

---

Next: [Interfaces and generics](04-types.md).
