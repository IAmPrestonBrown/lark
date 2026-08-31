# 04 - Managed Structs and Interfaces

## 1. `managed struct`

**Rule O-1.** A `managed struct` carries an object header. Chapter 03 rule M-4
gives the layout.

**Rule O-2.** A struct must be `managed` when either condition holds.

1. It contains a `gc` field. The collector needs the field map.
2. An `impl` targets it. The dispatch needs the method tables.

Diagnostic LK0400 reports a plain struct that needs the marker.

For a generic struct, chapter 05 rule G-10 applies the marker per
instantiation. `managed struct Box<T>` carries a header only for an
instantiation that needs one.

**Rule O-3.** The payload of a `managed struct` has C layout. Field order,
padding, and alignment match the C rules for the same field list.

Rule O-3 is what keeps constraint D-7. A `managed struct Person` passes to a C
function that expects the matching plain struct, with a cast and no copy.

**Rule O-25.** A definition that ends with `}` and declares no variable does
not need a trailing semicolon.

```c
managed struct Person { gc char* name; int age; }   /* legal */
managed struct Person { gc char* name; int age; };  /* also legal */
```

**Rule O-25a.** The declaration specifiers end at a `}` body. A qualifier and a
storage class both come before the type.

```c
static struct Point { int x; } origin;   /* correct */
struct Point { int x; } static origin;   /* the `static` starts a new item */
```

C accepts both orders. Rule O-25 makes the semicolon optional, so the second
order becomes ambiguous, and Lark reads the second word as the next item.

C requires the semicolon. Lark accepts it and accepts its absence, so rule S-1
holds and ordinary Lark code stays quiet. A definition that does declare a
variable keeps the C rule.

```c
struct Point { int x; int y; } origin;   /* the semicolon is required */
```

## 2. Allocation

**Rule O-4.** `new T { ... }` allocates a `T` in managed memory, initializes it
from the designated initializer list, and yields a `gc T*`.

```c
auto p = new Person { .name = "Joe", .age = 37 };   /* p : gc Person* */
```

**Rule O-5.** A field with no designator is zero. This matches the C rule for a
partial initializer.

**Rule O-6.** `new T[n]` allocates an array of `n` elements and yields a
`gc T*`. The header records the element count.

**Rule O-7.** `new` is valid only in expression position. Chapter 01 rule L-3
gives the recognition rule.

**Rule O-8.** Lark has no `delete`. The collector reclaims managed memory. Lark
has no finalizer and no weak reference in version 1.

## 3. Type information

The transpiler emits one `lark_typeinfo` per managed type.

```c
typedef struct lark_typeinfo {
    const char             *name;
    size_t                  size;         /* payload size */
    uint32_t                align;
    uint32_t                nptrs;
    const uint32_t         *ptr_offsets;  /* the field map, rule M-5 */
    uint32_t                nitables;
    const lark_itable_ent  *itables;      /* sorted by interface id */
} lark_typeinfo;
```

## 4. Interfaces

**Rule O-9.** An `iface` declares a set of function signatures. It declares no
data and no default body.

**Rule O-10.** The first parameter of every interface function is the receiver.
The programmer writes it. The compiler never inserts a parameter.

```c
iface Greet {
    void say_hi(Self this);
    void name_change(gc Self* this, gc char* new_name);
}
```

**Rule O-11.** The receiver takes one of two forms.

| Form | Meaning |
|---|---|
| `Self this` | By value. The callee gets a copy. |
| `gc Self* this` | By managed pointer. The callee can mutate. |

**Rule O-12.** Lark has no static method and no static field. Every interface
function has a receiver.

## 5. Implementations

**Rule O-13.** `impl I for T` defines every function that `I` declares, for the
type `T`. A missing function is diagnostic LK0410. An extra function is
diagnostic LK0411.

**Rule O-14.** `T` must be a `managed struct`. Diagnostic LK0412 reports any
other type.

**Rule O-15.** `impl I for T` must appear in the module that defines `I` or in
the module that defines `T`. This rule prevents two modules from defining
conflicting implementations for the same pair.

**Rule O-16.** In the body, `Self` names `T`. The programmer can write either
name. The transpiler treats them as one type.

Each `impl` emits one static method table.

```c
static const struct Greet_vtable lk_Greet__Person__vt = {
    .say_hi      = lk_mod__Greet__Person__say_hi,
    .name_change = lk_mod__Greet__Person__name_change,
};
```

## 6. Method calls

**Rule O-17.** `x.m(args)` resolves `m` across every interface that the static
type of `x` implements.

**Rule O-18.** The receiver adapts to the declared form.

| Static type of `x` | Receiver form | Emitted receiver |
|---|---|---|
| `gc T*` | `Self this` | `*x` |
| `gc T*` | `gc Self* this` | `x` |
| `T` lvalue in managed memory | `Self this` | `x` |
| `T` lvalue in managed memory | `gc Self* this` | `&x` |
| `T` lvalue on the stack | `gc Self* this` | Diagnostic LK0420 |

The last row enforces constraint D-3. The address of a stack object is not a
managed pointer.

**Rule O-19.** When the static type is a concrete `managed struct`, the call
compiles to a direct call. No table lookup happens.

```c
preston.say_hi();
/* emits: lk_mod__Greet__Person__say_hi(*preston); */

preston.name_change("just preston");
/* emits: lk_mod__Greet__Person__name_change(preston, "just preston"); */
```

**Rule O-20.** When the static type is an interface, the call reads the method
from the table in the fat pointer.

```c
g.say_hi();
/* emits: g.vt->say_hi(*(Person *)g.obj);  via the stored table */
```

**Rule O-21.** When two interfaces on one type declare the same method name,
`x.m()` is diagnostic LK0421. The qualified form `x.I::m()` selects one.

## 7. Interface values

**Rule O-22.** `I g = p;` builds a fat pointer from a `gc T*` when `T`
implements `I`. The conversion is implicit.

**Rule O-23.** The reverse direction uses a checked cast. `(gc T*)g` yields the
object pointer when the runtime type is `T`, and a null pointer otherwise.

**Rule O-24.** An interface value holds a managed pointer. The placement rules
M-1 and M-2 apply to it.
