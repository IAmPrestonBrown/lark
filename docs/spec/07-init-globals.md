# 07 - Initialization

## 1. The `init` function

**Rule I-1.** A program that uses managed memory carries exactly one `init`
function. Zero is diagnostic LK0700. Two or more is diagnostic LK0701, whatever
the program uses.

A program that uses no managed memory needs no marker. Rule I-3 puts the runtime
startup in the `init` function, and such a program starts no runtime. Rule S-1
then holds, because a valid C11 file carries no marker and needs none.

```c
init void main(void) {
    ...
}
```

**Rule I-2.** `init` marks where the runtime startup code goes. It does not make
the function the entry point. C `main` stays the entry point, so rule S-1 holds.

**Rule I-3.** The transpiler inserts the runtime startup at the first statement
of the `init` function, before any other inserted code and before the first
statement that the programmer wrote.

**Rule I-4.** The startup performs these steps in order.

1. Initialize the collector from the build configuration.
2. Register the calling thread.
3. Register the global root set.
4. Install the safepoint state.

**Rule I-5.** The `init` function is normally `main`. Any function can carry the
marker. A program that starts from a host callback puts `init` on that callback.

## 2. `@global` blocks

A C program can initialize a global only with a constant expression. Lark lifts
that limit without hidden startup code.

```c
@global main_globals {
    gc Person* person2 = new Person { .name = "Joe", .age = 37 };
}
```

**Rule I-6.** Every declaration in a `@global` block becomes a global variable.
The block name is not a scope. It is a handle for initialization.

**Rule I-7.** At program start, every such global holds a zero value. No
initializer runs.

**Rule I-8.** A declaration with a managed type joins the global root set. The
`@init` for the block registers it.

**Rule I-9.** `@init name;` runs the initializers of the block `name`, in
declaration order.

**Rule I-10.** A block initializes at most once. A guard flag protects it. A
second `@init` for the same block does nothing.

**Rule I-11.** `@init` on an undeclared block is diagnostic LK0710.

## 3. Attached blocks

A block can attach itself to a function. The transpiler inserts the `@init` call.

```c
@global(main) globals_2 { ... }
@global(main, 0) other_globals { ... }
```

**Rule I-12.** `@global(f)` inserts `@init` for the block at the start of `f`.

**Rule I-13.** `@global(f, n)` gives the block the order number `n`. A lower
number runs first.

**Rule I-14.** Blocks with no order number run after every numbered block, in
declaration order. A tie between equal numbers resolves by declaration order.

**Rule I-15.** The runtime startup from rule I-3 always runs first. Its position
is not configurable, and no order number places a block before it.

## 4. Ordering summary

For the `init` function, the order is fixed:

1. Runtime startup.
2. Attached blocks with an order number, lowest first.
3. Attached blocks with no order number, in declaration order.
4. Explicit `@init` statements, where the programmer wrote them.
5. The rest of the function body.

## 5. Initializer dependencies

**Rule I-16.** An initializer in block `A` that reads a global from block `B`
requires `B` to initialize first. The transpiler does not reorder blocks to
satisfy this. The programmer controls the order with rule I-13.

**Rule I-17.** The transpiler reports a read of an uninitialized global as
diagnostic LK0711 when it can prove the case from the order numbers. It does not
prove the case across an indirect call.

Rule I-16 keeps constraint D-2. An automatic topological sort is hidden
machinery. An order number is visible in the source.
