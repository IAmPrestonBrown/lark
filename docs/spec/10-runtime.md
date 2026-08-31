# 10 - Runtime

## 1. Structure

The runtime is a static C library. It has two layers.

**The core.** Thread registration, the shadow stack, safepoints, and the state
machine for foreign calls. Every program that uses a managed type links it.

**The collector.** One implementation behind a fixed interface. The build
selects it. A program that uses no managed type links neither layer.

Constraint D-6 requires this split. A tiny program gets a tiny runtime.

## 2. Core API

```c
/* startup and shutdown */
void lark_startup(void);
void lark_shutdown(void);

/* threads */
void lark_thread_attach(void);
void lark_thread_detach(void);

/* shadow stack, precise mode */
typedef struct lark_frame_hdr {
    struct lark_frame_hdr *prev;
    uint32_t               nslots;
} lark_frame_hdr;

void lark_frame_push(lark_frame_hdr *f);
void lark_frame_pop (lark_frame_hdr *f);

/* safepoints */
extern _Atomic int lark_gc_request;
void lark_safepoint(void);
#define LARK_POLL() do { if (lark_gc_request) lark_safepoint(); } while (0)

/* foreign call transitions */
void lark_enter_safe(void);
void lark_leave_safe(void);

/* non local jumps, rule M-15 */
typedef struct { jmp_buf jb; lark_frame_hdr *top; } lark_jmp_buf;
#define lark_setjmp(b)  (((b)->top = lark_frame_top()), setjmp((b)->jb))
void lark_longjmp(lark_jmp_buf *b, int v);

/* allocation, forwards to the collector */
void *lark_alloc(const lark_typeinfo *ti);
void *lark_alloc_array(const lark_typeinfo *ti, size_t n);

/* global roots */
void lark_root_register(void **slot, size_t n);
```

## 3. Collector interface

A collector implements this record. A program links exactly one.

```c
typedef struct lark_collector {
    const char       *name;
    lark_gc_caps      caps;

    void   (*init)(const lark_gc_config *cfg);
    void   (*shutdown)(void);

    void  *(*alloc)(const lark_typeinfo *ti, size_t n);
    void   (*collect)(void);

    /* interior pointer support, rule M-7 */
    void  *(*base_of)(void *interior);

    /* write barrier, a no-op for a non generational collector */
    void   (*write_barrier)(void **slot, void *value);
} lark_collector;
```

**Rule R-1.** The transpiler reads `caps` at build time and enforces the source
rules that depend on it. Chapter 03 rule M-8 is one example.

**Rule R-2.** A store of a managed pointer into a managed object goes through
`lark_write_barrier`. The barrier performs the store, so the caller writes
nothing else.

A collector that needs no barrier leaves the function pointer empty, and the
core then performs the store and returns. The `write_barrier` capability says
which case holds, and the transpiler emits the call only when it is true. Every
other build pays nothing at all.

```c
a->next = b;                                 /* no barrier */
lark_write_barrier((void **)&a->next, b);    /* a barrier */
```

The transpiler emits the call for a store whose target names a `gc` field of
some record in the module. A call where none is needed is correct and only
slower, because the barrier performs the store either way.

**Rule R-3.** A program links exactly one collector. The `gc.strategy` setting
names it, and a name that no collector answers to is an error rather than a
fall back to the default.

**Rule R-4.** A capability describes the collector rather than the wish of its
author. A collector that says `interior_pointers` must answer `base_of` for an
interior address, and one that says false must answer `NULL`. A collector that
says `moving` must also say `precise_stack`, because it writes a new address
into every root, and it cannot also say `interior_pointers`, because an interior
pointer has no slot that a copy can update.

**Rule R-5.** A collector that moves objects accepts rule M-10 shadow stack
roots alone. A rule M-13 conservative scan cannot say which words are roots, and
writing a new address into a word that is an integer would corrupt the program.
The runtime stops such a build at startup with a message that names the
setting.

**Rule R-6.** A collector that reclaims sizes its next collection from what the
last one left, never from a constant. A fixed trigger collects once per
allocation as soon as the live set passes it, and each of those collections
frees nothing, so the program stops making progress.

The rule holds for every space a collector grows.

| Collector | What follows the live set |
|---|---|
| `precise-marksweep` | The heap limit, at twice the heap that the sweep left |
| `semispace` | The destination space, at twice what the collection can copy |
| `generational` | The old generation, and the nursery at a share of it |

**Rule R-7.** A collector that moves objects grows a space only where a
collection can rewrite the pointers into it. A space that holds live objects
cannot move under the program: every root and every field would keep an address
that names nothing.

A semispace collector therefore enlarges the empty destination and lets the
collection copy into it. A generational collector enlarges the empty reserve
and runs a major collection, which copies into the reserve and swaps.

**Rule R-8.** An allocation that a collector cannot satisfy returns `NULL`. A
collector never aborts because a space filled, because the caller can report
the failure with the source position that rule X-3 gives.

## 4. The collectors

A program links exactly one. Rule R-3 makes `gc.strategy` name it.

| Name | Reclaims | Interior pointers | Moving | Barrier | Root modes |
|---|---|---|---|---|---|
| `precise-marksweep` | yes | yes | no | no | shadow stack, conservative |
| `arena` | no | yes | no | no | shadow stack, conservative |
| `semispace` | yes | no | yes | no | shadow stack |
| `generational` | yes | no | yes | yes | shadow stack |

### `precise-marksweep`

The default, and the one that every other collector is measured against.

Design: size classed pages, a block map for rule M-7, a mark bitmap, and a stop
the world mark and sweep.

| Capability | Value |
|---|---|
| `interior_pointers` | true |
| `moving` | false |
| `precise_heap` | true |
| `precise_stack` | true under `roots = "shadow-stack"` |
| `concurrent` | false |
| `reclaims` | true |

### `arena`

Allocation from a bump pointer, and nothing is ever freed. A `collect` call
walks no object and returns.

The collector serves three purposes. It is the smallest plugin that the seam
accepts, so it proves that the seam is a real boundary rather than a shape that
one collector happens to fit. It gives a baseline for the cost of collection,
because a program that runs under it pays for allocation alone. It removes the
collector from a bug hunt, because a fault that survives here is not a collector
fault.

A program that runs under it holds every object it ever allocated, so it suits
a short lived tool and a test, not a long running service.

| Capability | Value |
|---|---|
| `interior_pointers` | true |
| `moving` | false |
| `precise_heap` | true |
| `precise_stack` | true |
| `concurrent` | false |
| `reclaims` | false |

### `semispace`

A moving, copying, two space collector. Cheney's algorithm. Allocation is a
bump pointer in from space. A collection copies every reachable object to to
space, leaves a forwarding address behind, and swaps the two spaces. Everything
that it did not copy is dead, and the whole of from space becomes free at once.

The collector exists to prove that the seam supports a moving design. It needs
two things that a non moving collector does not, and both are properties of the
seam rather than of the collector: the address of every root, which rule M-10
supplies, and the field map of every object, which rule M-5 makes mandatory.

Rule R-5 makes it refuse conservative roots. Rule M-8 does not hold for it, so
rule R-1 rejects a program that needs an interior pointer.

| Capability | Value |
|---|---|
| `interior_pointers` | false |
| `moving` | true |
| `precise_heap` | true |
| `precise_stack` | true |
| `concurrent` | false |
| `reclaims` | true |

### `generational`

A nursery and one old generation, with a card table for the remembered set.
Allocation is a bump pointer in the nursery. A minor collection copies the
survivors of the nursery into the old generation, which makes it a moving
collector with the constraints that `semispace` already established.

Most objects die young, so a minor collection walks a small part of the heap
and costs a small part of a full collection.

That saving needs one thing that a full collection does not. A pointer from an
old object to a young one is a root that nothing in the nursery reaches, and a
minor collection does not walk the old generation. The card table answers it:
the old generation is divided into cards, a store of a managed pointer marks
the card that holds the field, and a minor collection scans every marked card
as a root.

Rule R-2 makes the transpiler emit the call, and only for a collector that asks
for one.

| Capability | Value |
|---|---|
| `interior_pointers` | false |
| `moving` | true |
| `precise_heap` | true |
| `precise_stack` | true |
| `concurrent` | false |
| `reclaims` | true |
| `write_barrier` | true |

## 5. Planned collectors

These do not ship. They exist to show what the seam still has room for.

| Name | `moving` | Note |
|---|---|---|
| `concurrent-mark` | false | Marks while the program runs. It needs the barrier that rule R-2 already defines, plus a tricolour invariant, and it changes the stop the world protocol that rule M-26 states. |

