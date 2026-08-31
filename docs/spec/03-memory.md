# 03 - Memory Model

## 1. Two worlds

Lark has a managed world and an unmanaged world. The unmanaged world is C. The
managed world is the collector heap. Constraint D-3 requires every crossing to be
explicit.

## 2. Placement rules

These rules decide where a managed pointer can live. They exist so that the
collector can find every managed pointer in the program.

**Rule M-1.** A managed pointer can live in exactly four places.

1. A local variable, a function parameter, or a temporary.
2. A field of a `managed struct`.
3. A declaration inside a `@global` block.
4. An element of an array that a `managed struct` owns.

**Rule M-2.** A managed pointer must not live in memory from `malloc`, in a
plain C global, or in a field of a plain `struct`. Diagnostic LK0310 reports the
violation.

**Rule M-3.** A `managed struct` value can live on the stack. The shadow stack
traces it. A `managed struct` value must not live in memory from `malloc`.
Diagnostic LK0311 reports the violation.

Rule M-3 has one consequence worth stating. `malloc(sizeof(Person))` for a
`managed struct Person` is a compile error, not a runtime problem.

## 3. Object headers

**Rule M-4.** Every managed object carries a header. The header sits immediately
before the payload. A `gc T*` points at the payload, not at the header.

```c
typedef struct lark_header {
    const lark_typeinfo *type;   /* size, alignment, field map, itables */
    uintptr_t            flags;  /* mark bits and collector private state */
} lark_header;

#define LARK_HEADER(p) (((lark_header *)(p)) - 1)
```

Rule M-4 preserves constraint D-7. The payload has C layout. A `gc Person*` and
a `Person*` point at the same bytes. C code reads the fields with no adaptation.

**Rule M-5.** The `lark_typeinfo` for a type lists the byte offset of every `gc`
field. This is the **field map**. It makes heap tracing precise, under every
collector.

**Rule M-5a.** Every record with a body gets its own `lark_typeinfo`, whether or
not it holds a `gc` field. `lark_new` copies `size` bytes from the initializer,
and the size comes from the descriptor. A record with no `gc` field gets a
descriptor with an empty field map. See decision D100.

## 4. Interior pointers

**Rule M-6.** A managed pointer can address any byte inside a managed object. An
interior pointer is a first class value.

```c
gc char* buf = new char[256];
gc char* p   = &buf[3];        /* legal */
```

**Rule M-7.** The allocator maintains a block map. Any address inside a managed
block resolves to its payload base in constant time. The collector uses the block
map to find the header for an interior pointer.

**Rule M-8.** A collector declares whether it supports interior pointers. If the
selected collector does not, the transpiler rejects every construction of an
interior pointer. Diagnostic LK0320 reports it.

Rule M-8 is the mechanism that constraint D-4 requires. A future moving collector
sets the flag to false. The language does not change. The build configuration
does.

## 5. Root sets

**Rule M-9.** The collector treats three sets as roots.

1. The **global root set**. Every `@global` block declaration with a managed
   type. The `@init` for that block registers it. Chapter 07 gives the ordering.
2. The **stack root set**. The mechanism depends on the configured mode.
3. The **pinned set**. Objects that a foreign call holds. Section 8 defines it.

### 5.1 Shadow stack mode

The build selects this mode with `roots = "shadow-stack"`. It gives precise stack
roots.

A function that holds a managed local emits a frame record. The record links into
a per thread list.

**A frame holds two arrays.** The first holds the *address* of each managed
local, so the local keeps the name that the programmer wrote and every use of it
stays as written. Rule X-2 needs that. The second holds the *value* of each
temporary, because a fresh object belongs to no local yet.

```c
void f(void) {
    /* lark: shadow stack frame, 2 managed locals, 1 temporary */
    struct { lark_frame_hdr h; void **s[2]; void *t[1]; } _lk_frame;
    _lk_frame.h.slots = _lk_frame.s;
    _lk_frame.h.temps = _lk_frame.t;
    _lk_frame.h.nslots = 0;
    _lk_frame.h.ntemps = 1;
    _lk_frame.t[0] = NULL;
    lark_frame_push(&_lk_frame.h);

    Person *first = (_lk_frame.t[0] = lark_new(&app__Person__ti, &(Person){ .age = 1 }),
                     (Person *)_lk_frame.t[0]);
    _lk_frame.s[0] = (void **)&first;
    _lk_frame.h.nslots = 1;

    Person *second = first;
    _lk_frame.s[1] = (void **)&second;
    _lk_frame.h.nslots = 2;

    lark_frame_pop(&_lk_frame.h);
}
```

**Rule M-10.** A function with no managed local and no managed temporary emits
no frame record and pays no cost. Constraint D-1 holds for all C code and for
all unmanaged Lark code.

**Rule M-10a.** A managed parameter is a managed local, so it gets a slot. The
slot joins the frame right after the push, because a parameter already holds
its value there. A function that rule M-18 shows cannot reach an allocation
reaches no safepoint either, so it roots no parameter and keeps the zero cost
that constraint D-1 gives.

**Rule M-11.** A slot joins the frame after its local has a value. The count
rises as each local comes into scope, so a collection never reads a local that
does not exist yet. Every temporary slot is null before the push.

**Rule M-12.** Every exit path pops the frame. A `return` evaluates its
expression into a temporary, pops the frame, and then returns the temporary. The
locals stay rooted while the expression runs.

**Rule M-27.** Every `new` expression in a function gets one temporary slot. The
result goes into the slot first, and the expression then reads it back.

An expression such as `f(new A(), new B())` allocates twice. The first result
belongs to no local when the second allocation runs, and its temporary slot
keeps it. A slot holds its object until the same `new` runs again, or until the
function returns.

### 5.2 Conservative mode

The build selects this mode with `roots = "conservative"`. The collector scans
the machine stack and the registers, and treats every word that the block map
resolves as a root.

**Rule M-28.** The allocation runs before the initializer. An allocation is a
safepoint, so an initializer that names a managed local must read the local
after the collection rather than before it. A collector that moves objects
otherwise stores the address that the local held before the move.

```c
/* wrong: `head` is read before `lark_new` can move it */
t[0] = lark_new(&ti, &(Node){ .next = head });

/* right: `head` is read after */
t[0] = lark_new(&ti, 0), *(Node *)t[0] = (Node){ .next = head };
```

**Rule M-28a.** No allocation stands between an allocation and the stores that
fill it. A `new` inside an initializer is hoisted, so it runs first and lands in
its own temporary slot, and the initializer then reads slots alone. Without the
hoist, a collection during the inner allocation moves the outer object while the
emitted C already holds the address it is about to store through. C does not fix
the order in which the two sides of an assignment are evaluated, so the fault is
not one a reading of the output catches.

```c
/* `new Pair { .first = new Pair { ... } }` */
t[0] = lark_new(&ti, 0), *(Pair *)t[0] = (Pair){ .value = 1 },
t[1] = lark_new(&ti, 0), *(Pair *)t[1] = (Pair){ .first = (Pair *)t[0] }
```

**Rule M-13.** Conservative mode emits no frame record. It costs nothing at run
time. It cannot support a moving collector, and it can retain dead objects.

**Rule M-14.** The two modes are source compatible. The same program compiles
under either. The mode is a configuration choice, never a source change.

## 6. Non local jumps

**Rule M-15.** `longjmp` skips every frame pop between the jump and its target.
The runtime therefore saves the shadow stack head inside the jump buffer, and
restores it on the jump.

Lark provides `lark_setjmp` and `lark_longjmp` for this. Plain `setjmp` and
`longjmp` stay available and stay correct for code with no managed local. A
`longjmp` across a frame that holds a managed local, through the plain C
functions, is undefined. Diagnostic LK0330 warns where the transpiler can prove
the case.

## 7. Safepoints

**Rule M-16.** A safepoint poll appears at two places.

1. Immediately before every managed allocation.
2. At every loop back edge in a function that can allocate or that can call a
   function that can allocate.

**Rule M-17.** The poll is a load and a branch.

```c
#define LARK_POLL() do { if (lark_gc_request) lark_safepoint(); } while (0)
```

**Rule M-18.** A function that cannot reach an allocation emits no poll. The
transpiler computes this by a call graph analysis. An indirect call and an
unmarked extern both count as able to allocate.

## 8. Foreign calls

Two markers describe the contract of a function that Lark does not compile.

**Rule M-19.** `gc_safe` means a collection can run while the callee runs. The
caller enters the safe state before the call and leaves it after. Managed
arguments stay rooted across the call.

**Rule M-20.** `gc_leaf` means the callee triggers no collection, blocks on
nothing, allocates no managed memory, and calls no Lark function. No state
transition happens. The call is as cheap as a C call.

**Rule M-21.** An extern declaration with no marker is `gc_safe`. The safe
default is always correct. `gc_leaf` is the opt in optimization.

This matches every mainstream implementation. CoreCLR transitions on every
P/Invoke and needs `[SuppressGCTransition]` to opt out. LLVM inserts a safepoint
for every call and needs the `"gc-leaf-function"` attribute to opt out. HotSpot
JNI and Go cgo always transition.

**Rule M-22.** A `gc_leaf` function must not take a parameter of managed type.
Diagnostic LK0340 reports it. A managed argument to a leaf call has no root
across the call.

Emitted form of a `gc_safe` call:

```c
lark_enter_safe();
r = foo(a, b);
lark_leave_safe();
```

## 9. Threads

**Rule M-23.** The runtime is thread safe. Lark adds no thread syntax. Threads
come from the host, through the C interface.

**Rule M-24.** A thread must register with the runtime before it touches managed
memory, and must unregister before it exits. `lark_thread_attach` and
`lark_thread_detach` do this.

**Rule M-25.** Each thread owns its shadow stack head and its allocation buffer.
Both live in thread local storage.

**Rule M-26.** A collection stops the world. The collector sets `lark_gc_request`
and waits for every registered thread to reach a safepoint or to enter the safe
state.

## 10. Collector capabilities

A collector declares a capability record. The transpiler reads it and enforces
the matching source rules.

```c
typedef struct lark_gc_caps {
    bool interior_pointers;   /* rule M-8 */
    bool moving;              /* forbids interior pointers and pinning free calls */
    bool precise_heap;        /* always true, the field map is mandatory */
    bool precise_stack;       /* shadow stack mode only */
    bool concurrent;
} lark_gc_caps;
```
