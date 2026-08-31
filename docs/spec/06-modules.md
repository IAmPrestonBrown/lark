# 06 - Modules and Names

## 1. Modules

**Rule N-1.** One file is one module. The module name is the file name without
the `.lark` extension.

**Rule N-2.** `@import name` makes the module `name` available. Every reference
to a symbol in it uses the `name::` prefix.

```c
@import stdio

void f(void) {
    stdio::printf("%s\n", "hi");
}
```

## 2. Resolution

**Rule N-3.** `@import stdio` searches for `stdio.lark` in this order.

1. The directory of the importing file.
2. Each directory in `paths.search` from `lark.toml`, in order.
3. The package directories, once the package system exists.

The first match wins. No match is diagnostic LK0600.

**Rule N-4.** An import cycle is legal. Chapter 01 rule L-8 gives two pass
resolution, so a cycle needs no forward declaration. A cycle in `@global`
initializer *values* is a separate error. Chapter 07 covers it.

## 3. Visibility

**Rule N-5.** A top level declaration is private to its module by default.

**Rule N-6.** `export` makes a declaration visible to a module that imports it.

```c
export managed struct Person { gc char* name; int age; }
export void greet(gc Person* p);
```

**Rule N-7.** `export` applies to a function, a type, an interface, a `@global`
block, and a declaration inside a `@global` block.

**Rule N-8.** `export` on a `@global` block exports every declaration in it. A
declaration inside a non exported block can carry its own `export`.

**Rule N-9.** An `impl` is exported with its type. A type without its methods is
not usable. An `impl` carries no `export` marker of its own.

**Rule N-10.** An exported declaration must not name a private type in its
signature. Diagnostic LK0610 reports it, and names both the declaration and the
private type.

**Rule N-11.** An import is never re-exported. If module `a` imports `b`, a
module that imports `a` does not see `b`. It must import `b` itself.

## 4. C symbols

**Rule N-12.** A symbol from `#include` enters the global namespace, as C
defines. It needs no prefix. A module namespace and the C global namespace are
separate.

**Rule N-13.** A module that declares an extern C function exports it under the
module namespace. `stdio.lark` declares `printf`, so `stdio::printf` names it.
The emitted call uses the unmangled C name `printf`.

Rule N-13 is what makes `stdio.lark` work with one line of content:

```c
int printf(const char *restrict format, ...);
```

## 5. Name mangling and linkage

**Rule N-14.** A Lark symbol keeps its name in the emitted C. Chapter 09 rule
X-5 states this, and rule X-5a lists the generated names that do not follow it.

**Rule N-15.** A private definition emits as `static`. Chapter 09 rule X-5b
gives the three definitions that stay external.

Visibility and linkage answer two different questions. Visibility says which
Lark module can name a symbol. Linkage says which object file can reach it. The
`export` marker sets both.
