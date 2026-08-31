# 09 - Code Generation

## 1. Output

**Rule X-1.** The transpiler emits C11. It uses no compiler extension.

**Rule X-2.** The emitted C is readable. It keeps the identifier names of the
source, it keeps the structure of the source, and it carries comments that
mark inserted machinery.

**Rule X-3.** The emitted C carries `#line` directives that map every construct
back to the `.lark` source. A debugger and a compiler error both report the
original file and line.

Rules X-2 and X-3 serve constraint D-2. A programmer who wants to see the cost of
a construct reads the output.

## 2. File structure

One module emits one `.c` file and one `.h` file.

```
build/
  app.c              module body
  app.lark.h         exported declarations
  lark-build.toml    the settings of the build, rule F-2
  lark_rt.h          runtime declarations
```

**Rule X-4.** The `.h` file contains only exported declarations. Chapter 06 rule
N-6 defines exported.

**Rule X-4a.** An exported definition lives in exactly one place.

| Kind | Header | Body |
|---|---|---|
| A type: struct, union, enum, or `typedef` | The definition | Nothing |
| A function | The prototype | The definition |
| A variable | `extern` and the declaration | The definition and its initializer |

A type definition in both files is a redefinition, and a variable definition in
a header defines one object per file that includes it.

**Rule X-4b.** A generated file name carries `.lark.`, so that it can never
take the name of a file that a programmer wrote. A module `attribute` emits
`attribute.lark.h`, and a neighbouring `attribute.h` keeps its own name.

The build directory holds only generated files. A source header reaches the
compiler through `-iquote` on the directory of the module, which comes after the
build directory, so the two sets never meet. See decision D117.

## 3. Name mangling

**Rule X-5.** A function, a global, and a type that the programmer writes keep
their names.

```c
export void draw(gc Point* p);   ->   void draw(Point *p)
```

Constraint D-7 requires this. A C file must be able to call an exported Lark
function by its name, and a Lark program must be able to define `main`.

**Rule X-5a.** A symbol that the transpiler generates uses the `lk_` scheme. The
programmer never writes one of these names.

| Kind | Form |
|---|---|
| Generic instantiation | `lk_<module>__<name>__<argmangle>` |
| Interface method implementation | `lk_<module>__<iface>__<type>__<method>` |
| Method table | `lk_<module>__<iface>__<type>__vt` |
| Type descriptor | `lk_<module>__<type>__ti` |
| Global block initializer | `lk_<module>__<block>__init` |
| Shadow stack frame | `lk_frame` |

**Rule X-5b.** A private definition emits as `static`, so two modules can each
hold a private symbol of the same name.

Three definitions never become `static`.

1. A function named `main`. It is the entry point.
2. A declaration that already carries `extern`.
3. A declaration with no body whose name the module does not define. It names a
   symbol that lives elsewhere.

A declaration with no body whose name the module **does** define is a forward
declaration. The emitted C carries one for every function definition, so a call
never precedes a declaration.

**Rule X-5c.** Two modules that export the same name collide at link time, as
two C files do. Lark does not rename around it.

**Rule X-5d.** A symbol that an included header declares keeps external linkage.
The header states the symbol to the rest of the program, so the `static` marker
would contradict the prototype. This is what lets a plain C file keep the
linkage that C gives it. See decision D105.

**Rule X-6a.** The emitted C writes every `#include` first, then every local
`typedef`, then the forward declarations. Each stage can name something from the
stage before it, and rule L-8 makes a module order independent, so the order is
fixed rather than the order of the source. See decision D106.

**Rule X-6.** An extern C symbol keeps its exact name. It gets no prefix.

**Rule X-8.** A `struct`, `union`, or `enum` definition also emits a `typedef`
of the same name.

```c
managed struct Person { gc char* name; }
```

```c
struct Person { char *name; };
typedef struct Person Person;
```

Lark code names a type without the keyword, as `gc Person* p`. C needs either
the keyword or a `typedef`, and the `typedef` keeps the emitted C readable.

**Rule X-7.** A generated name that the linker sees carries `lk_`, the module
name, and a double underscore between every part. A well formed user program
cannot collide with it, because the shape belongs to the transpiler alone.

A generated name that lives inside one block carries the `lk_` prefix and no
module name, because no other translation unit can see it: `lk_frame`,
`lk_self`, `lk_result`, and `lk_config`.

## 4. Desugaring table

| Source | Emitted C |
|---|---|
| `gc T* p` | `T *p` |
| `new T { ... }` | `lark_alloc(&lk_m__T__ti)` then field stores |
| `new T[n]` | `lark_alloc_array(&lk_m__T__ti, n)` |
| `x.m(a)` on a concrete type | `lk_m__I__T__m(recv, a)` |
| `gc`, `managed`, `init`, `gc_leaf`, `gc_safe` | Removed. They mark Lark machinery, not C. |
| A managed struct | Its definition, a `typedef`, and a `lark_typeinfo` |
| A function with a managed value | A shadow stack frame around its body |
| A loop in a module that allocates | `LARK_POLL();` at the top of its body |
| The `init` function | `lark_startup();` as its first statement |
| `x.m(a)` on an interface value | `x.vt->m(recv, a)` |
| `I g = p` | `(I){ .obj = p, .vt = &lk_m__I__T__vt }` |
| `mod::f(a)` | `f(a)`. Rule X-5 keeps the name. |
| `@import m` | `#include "m.h"` |
| `export` on an item | The item joins the module header |
| An item with no `export` | `static`, under rule X-5b |
| `@init b;` | `lk_m__b__init()` |
| `Data<int>` | `struct lk_m__Data__i` |
| A `gc_safe` call | `lark_enter_safe(); r = f(); lark_leave_safe();` |
| A `gc_leaf` call | `r = f();` |
| A managed local | A shadow stack slot, in shadow stack mode |
| A loop back edge | `LARK_POLL();` when rule M-18 requires it |

## 5. Worked example

Source:

```c
managed struct Person { gc char* name; int age; }

iface Greet { void say_hi(Self this); }

impl Greet for Person {
    void say_hi(Person this) { stdio::printf("%s\n", this.name); }
}

init void main(void) {
    auto p = new Person { .name = "Preston", .age = 18 };
    p.say_hi();
}
```

Emitted C, with the machinery marked:

```c
#line 1 "app.lark"
typedef struct Person { char *name; int age; } Person;

/* lark: field map for Person */
static const uint32_t lk_app__Person__ptrs[] = { offsetof(Person, name) };
static const lark_typeinfo lk_app__Person__ti = {
    .name = "Person", .size = sizeof(Person), .align = _Alignof(Person),
    .nptrs = 1, .ptr_offsets = lk_app__Person__ptrs,
    .nitables = 1, .itables = lk_app__Person__itabs,
};

#line 6 "app.lark"
static void lk_app__Greet__Person__say_hi(Person this) {
    printf("%s\n", this.name);
}

static const struct Greet_vtable lk_app__Greet__Person__vt = {
    .say_hi = lk_app__Greet__Person__say_hi,
};

#line 9 "app.lark"
int main(void) {
    /* lark: shadow stack frame, 1 managed local */
    struct { lark_frame_hdr h; void *s[1]; } _lf;
    _lf.h.nslots = 1; _lf.s[0] = NULL;
    lark_frame_push(&_lf.h);

    lark_startup();                       /* lark: init */

#line 10 "app.lark"
    LARK_POLL();
    _lf.s[0] = lark_alloc(&lk_app__Person__ti);
    ((Person *)_lf.s[0])->name = "Preston";
    ((Person *)_lf.s[0])->age  = 18;

#line 11 "app.lark"
    lk_app__Greet__Person__say_hi(*(Person *)_lf.s[0]);

    lark_frame_pop(&_lf.h);
    return 0;
}
```

## 6. Build pipeline

1. Read `lark.toml`.
2. Preprocess the `#include` set of each module with `cc -E -dD`. Rule C-1a
   keeps the Lark source out of the preprocessor.
3. Pass one. Collect every top level name across every module.
4. Pass two. Parse bodies, check types, monomorphize.
5. Emit C and headers.
6. Invoke the C compiler and the linker with the selected runtime.
