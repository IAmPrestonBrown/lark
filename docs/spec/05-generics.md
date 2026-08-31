# 05 - Generics

## 1. Model

**Rule G-1.** Lark generics are monomorphic. The transpiler emits one concrete C
definition for each distinct set of type arguments. No runtime machinery exists
for a generic.

**Rule G-2.** A generic parameter is a type. Chapter 01 rule L-7 states this. A
value parameter, as in a C++ non type template parameter, does not exist.

## 2. Declarations

A generic struct:

```c
struct Data<T> {
    T* data_point;
}
```

A generic function:

```c
T* first<T>(T* items, int n) {
    return n > 0 ? &items[0] : NULL;
}
```

**Rule G-3.** A generic struct and a generic function both exist. A generic
`iface` does not exist in version 1.

**Rule G-4.** Version 1 has no constraint syntax. A type error appears after
substitution, at the instantiation site. The diagnostic reports both the error
and the instantiation that caused it.

## 3. Instantiation

**Rule G-5.** An instantiation is written `Name<Arg, ...>`. Chapter 01 rule L-6
gives the parse rule.

```c
gc Data<int>* count = new Data<int> { .data_point = NULL };
swap<Person>(&a, &b);
```

**Rule G-6.** A call to a generic function carries an explicit type argument
list.

```c
swap<int>(&a, &b);
```

**Rule G-6a.** Version 1 infers no type argument. A call with no list is
diagnostic LK0501, and the message names the list to write.

Inference needs the type of every call argument, which needs the full expression
type checker. Version 1 asks for the list instead, and the diagnostic makes the
cost one edit rather than a puzzle.

**Rule G-7.** Two instantiations with the same type arguments share one emitted
definition, across the whole program.

**Rule G-8.** Instantiation is not recursive without bound. The transpiler stops
at a configured depth and reports diagnostic LK0500.

## 4. Generics and managed memory

**Rule G-9.** A generic struct is `managed` only when its declaration says so.
`struct Data<T>` is a plain struct at every instantiation.

**Rule G-10.** `managed struct Name<T>` declares a **conditionally managed**
generic. The transpiler decides per instantiation.

- If the instantiation contains a `gc` field after substitution, or an `impl`
  targets it, the instantiation carries an object header.
- Otherwise the instantiation carries no header and costs what a plain struct
  costs.

```c
managed struct Box<T> { T value; }

Box<int>         a;   /* no header, plain C struct */
Box<gc Person*>  b;   /* header, field map with one entry */
```

Rule G-10 keeps constraint D-2. The `managed` marker is visible at the
declaration, so a reader sees that the type can carry machinery. Rule G-10 keeps
constraint D-1 as well, because an instantiation that needs nothing pays
nothing.

**Rule G-11.** A generic struct without the `managed` marker, whose
instantiation contains a `gc` field after substitution, is diagnostic LK0400.
The diagnostic names the instantiation and the declaration, and the help text
names the `managed` marker.

```c
struct Bad<T> { T value; }
Bad<gc Person*> x;    /* LK0400 */
```

**Rule G-12.** Two instantiations of one conditionally managed generic are two
distinct types with two distinct layouts. This is already true of any generic.

**Rule G-13.** A managed instantiation gets its own `lark_typeinfo`. The field
map differs per instantiation, so the transpiler cannot share one. An
instantiation with no header emits no `lark_typeinfo`.

## 5. Name mangling

Chapter 09 defines the full scheme. The generic part:

```
lk_<module>__<name>__<argmangle>
```

| Type | Mangle |
|---|---|
| `int` | `i` |
| `unsigned int` | `j` |
| `char` | `c` |
| `T*` | `P<T>` |
| `gc T*` | `G<T>` |
| A user type | `<len><module>_<name>` |

`Data<int>` in module `app` becomes `lk_app__Data__i`.
