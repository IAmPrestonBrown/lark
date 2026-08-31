# 6. Startup and globals

Two things this page covers. Where the runtime starts, and the one place a
managed pointer can live outside a function.

## `init` marks the entry point

A program with managed memory needs one function with the `init` marker. The
runtime starts there.

```c
init int main(void) { ... }
```

Leave it off and the build stops.

```
$ lark check noinit.lark
error[LK0700]: no function carries the `init` marker
 --> noinit.lark:1:1
  |
1 | managed struct Person {
  | ^ no function carries the `init` marker
  = note: rule I-3 puts the runtime startup in that function
  |
help: write `init` before the entry point, as in `init void main(void)`
```

The marker writes four lines at the top of the function.

```c
lark_gc_config _lk_config = lark_gc_config_default();
_lk_config.roots = LARK_ROOTS_SHADOW_STACK;
_lk_config.torture = false;
lark_startup_with(_lk_config);
```

The settings come from `lark.toml`. Nothing decides them at run time, so a
build is what it says it is.

A program with no managed memory needs no `init` and never links the runtime.

## `@global` holds a managed pointer

Rule M-1 gives a managed pointer three homes, and this is the third. A managed
pointer at file scope has no root, so it is an error. A `@global` block gives
it one.

```c
@global people {
    gc Person* founder = new Person { .name = "Ada", .age = 36 };
}
```

The block becomes a function, and the emitted C registers the root before the
initializer runs.

```c
static void lk_start__people__init(void) {
    if (lk_start__people__done) { return; }
    lk_start__people__done = 1;
    ...
    lark_root_register((void **)&founder, 1);
    founder = (Person *)lark_new(&lk_start__Person__ti, ...);
    lark_frame_pop(&_lk_frame.h);
}
```

The guard is rule I-10: a block runs once, however many times you ask.

## Running a block

An unattached block runs where you write `@init`.

```c
@init people;
```

An attached block runs before the function it names.

```c
@global(main, 0) first  { int a = 1; }
@global(main, 1) second { int b = 2; }
@global(main)    third  { int c = 3; }
```

A lower number runs first, and a block with no number runs after every numbered
one. Rules I-13 and I-14 give the order, and it never depends on file order or
on link order.

## A whole program

```c
@import stdio

managed struct Person {
    gc char* name;
    int age;
}

@global people {
    gc Person* founder = new Person { .name = "Ada", .age = 36 };
}

@global(main) third  { int c = 3; }
@global(main, 1) second { int b = 2; }
@global(main, 0) first  { int a = 1; }

init int main(void) {
    // The attached blocks already ran, in order.
    stdio::printf("%d %d %d\n", a, b, c);

    @init people;
    @init people;      /* rule I-10 makes the second call quiet */
    stdio::printf("%s %d\n", founder->name, founder->age);
    return 0;
}
```

```
1 2 3
Ada 36
```

A name a block declares is visible after the block, in the enclosing scope.
That is what makes `a`, `b`, `c`, and `founder` readable in `main`.

## When to use one

Use a `@global` block for a managed pointer that must outlive every function,
and for setup that must run before a particular function. Use nothing at all
for a constant, because a constant needs no root.

Keep the blocks few. A program whose startup order matters is a program that is
hard to read, and the numbers make the order explicit rather than making it
good.

## The rules you meet first

| Rule | What it says |
|---|---|
| I-1 | One function carries `init`, and the runtime starts there. |
| I-3 | The startup goes at the top of that function. |
| I-8 | A managed global joins the root set before anything runs. |
| I-9 | The initializers of a block run in declaration order. |
| I-10 | A block runs once, however many times it is asked. |
| I-13 | A lower number runs first. |
| I-14 | An unnumbered block runs after every numbered one. |
| M-1 | A `@global` block is one of the three homes for a managed pointer. |

---

Next: [Talking to C](07-c-interop.md).
