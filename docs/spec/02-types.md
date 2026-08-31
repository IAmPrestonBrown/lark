# 02 - Types

## 1. The C type system

Lark includes every C11 type without change. Integer types, floating types,
pointers, arrays, unions, enums, function types, and plain structs behave as C
defines them. Their size, alignment, and representation match the C ABI of the
target.

Lark adds three type constructs: the `gc` qualifier, the `managed` struct, and
the interface type.

## 2. The `gc` qualifier

`gc` is a type qualifier. It sits in the same grammar position as `const` and
`volatile`.

**Rule T-1.** `gc` qualifies a **pointer**, not a pointee. In `gc char* name`,
the value `name` is a managed reference. It refers to a character array in
managed memory.

**Rule T-1a.** `gc` in the declaration specifiers qualifies the outermost
pointer that the declarator builds. A second `gc` qualifies the next level in.
`gc` after a `*` qualifies that pointer.

```c
gc char*   a;    /* a managed pointer to char */
gc T**     b;    /* a managed pointer to an unmanaged T* */
gc gc T**  c;    /* a managed pointer to a managed T* */
T* gc      d;    /* the same as gc T* d */
```

This differs from `const`. A `const` in the specifiers qualifies the base type,
and `gc` cannot, because rule T-2 allows `gc` only on a pointer.

**Rule T-2.** `gc` applies only to a pointer type. `gc int x;` is invalid.

**Rule T-3.** A `gc T*` refers to memory that a collector owns. The collector
finds the object header for that address. Chapter 03 states where the header
comes from.

**Rule T-4.** A struct type that contains a `gc` field must be declared
`managed`. The collector needs a field map, and the field map lives in the
object header.

Rule T-4 is the load bearing rule of the memory model. It guarantees that every
managed pointer in the heap sits inside an object the collector can trace.

### Multiple levels

`gc T** p` reads as: `p` is a managed pointer to a `T*`. The inner `T*` is an
unmanaged pointer. To make both levels managed, write `gc gc T** p`. Rule T-1a
gives the placement.

**Rule T-1b.** A function declarator builds a function type, and `gc` in the
specifiers qualifies what the function returns, the same way `const` does in C.
`gc Node* make(int n)` returns a managed pointer to `Node`.

```c
gc Node* make(int n);      /* returns gc Node*      */
gc gc Node** pair(void);   /* returns gc gc Node**  */
```

## 3. Conversions

**Rule T-5.** No implicit conversion exists between a managed pointer type and
an unmanaged pointer type, in either direction.

**Rule T-6.** An explicit cast performs the crossing. `(void*)p` strips `gc`.
`(gc void*)p` adds `gc`.

**Rule T-7.** A cast that adds `gc` is an assertion by the programmer. The
address must be the address of a live managed object, or a null pointer. A build
with checks enabled verifies the assertion at runtime and aborts on failure. A
release build performs no check and emits no code.

| From | To | Result |
|---|---|---|
| `T*` | `gc T*` | Explicit cast only. Diagnostic LK0301 without a cast. |
| `gc T*` | `T*` | Explicit cast only. Diagnostic LK0301 without a cast. |
| `gc T*` | `gc void*` | Implicit, as in C. |
| `gc T*` | `gc U*` | Explicit cast, as in C. |
| `T*` | `void*` | Implicit, as in C. |
| Null pointer constant | `gc T*` | Implicit. |

**Rule T-8.** A string literal has static storage duration. Its address lies
outside the managed heap. A collector that identifies managed addresses by range
therefore ignores it safely. A string literal is assignable to `gc char*`
without a cast.

Rule T-8 removes the friction from the most common case. It does not weaken rule
T-5, because a literal is not an unmanaged *pointer value* under the programmer's
control. It is a constant with a known lifetime that never ends.

A runtime copy into managed memory uses a library function, not syntax. The
standard library provides `str::from_cstr`.

## 4. Type inference with `auto`

**Rule T-9.** `auto` infers the declared type from the initializer. An `auto`
declaration must have an initializer. Diagnostic LK0210 reports a missing one.

**Rule T-10.** Inference takes the type of the initializer expression, after
array to pointer decay and function to pointer decay, and drops top level
qualifiers other than `gc`.

`gc` survives inference. This is deliberate. If `auto p = new Person { ... };`
dropped `gc`, the declaration would silently cross the managed boundary and
violate constraint D-3.

```c
auto preston = new Person { .name = "Preston", .age = 18 };
// preston has type: gc Person*
```

**Rule T-11.** `auto` applies to a block scope variable and to a `@global` block
declaration. `auto` does not apply to a function parameter, a return type, or a
struct member.

## 5. Interface types

An interface name is a type. A value of that type is a **fat pointer**: the
object reference and the method table pointer.

```c
struct { gc void *obj; const IfaceVTable *vt; }
```

**Rule T-12.** An interface value is two machine words. It has no C ABI
equivalent. A function that takes an interface value is not callable from C.

**Rule T-13.** An interface value contains a managed pointer. The placement
rules of chapter 03 apply to it exactly as they apply to a `gc T*`.

**Rule T-14.** A `gc T*` converts implicitly to an interface value `I` when `T`
implements `I`. The reverse direction needs an explicit checked cast, which
chapter 04 defines.

## 6. `Self`

**Rule T-15.** Inside an `iface` declaration, `Self` names the implementing type.
Inside an `impl` body, `Self` names the concrete type of that `impl`.

**Rule T-16.** Outside an `iface` or `impl`, `Self` is an ordinary identifier.
