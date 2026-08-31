# 4. Interfaces and generics

Two additions, and neither one adds a hidden cost. An interface call through a
concrete value is a direct call. A generic makes one C definition per set of
type arguments, and no runtime work at all.

## An interface

`iface` declares the functions. `impl` defines them for one type.

```c
iface Show {
    void show(Self this);
}

impl Show for Person {
    void show(Person this) {
        stdio::printf("person %s\n", this.name);
    }
}
```

`Self` stands for the type that implements the interface. In the `impl` block
you write the real type, because that is what the function takes.

## A working program

```c
@import stdio

managed struct Person {
    gc char* name;
    int age;
}

managed struct Robot {
    int serial;
}

iface Show {
    void show(Self this);
}

impl Show for Person {
    void show(Person this) {
        stdio::printf("person %s\n", this.name);
    }
}

impl Show for Robot {
    void show(Robot this) {
        stdio::printf("robot %d\n", this.serial);
    }
}

// A generic holder. One definition, one copy per instantiation.
managed struct Box<T> {
    T item;
}

// A generic function. The argument is inferred at the call.
T larger<T>(T left, T right) {
    if (left > right) {
        return left;
    }
    return right;
}

init int main(void) {
    auto p = new Person { .name = "Ada", .age = 36 };
    auto r = new Robot { .serial = 7 };

    // A concrete call. The emitted C names the function directly.
    p.show();
    r.show();

    // An interface value. The call goes through a method table.
    Show first = p;
    Show second = r;
    first.show();
    second.show();

    auto held = new Box<int> { .item = 42 };
    stdio::printf("box %d\n", held->item);
    stdio::printf("larger %d\n", larger<int>(3, 9));
    return 0;
}
```

```
person Ada
robot 7
person Ada
robot 7
box 42
larger 9
```

## The two calls cost different things

`p.show()` knows the type, so rule O-19 writes a direct call.

```c
lk_types__Show__Person__show(*p);
```

`first.show()` does not, so it goes through a table. An interface value is two
words: the object, and the table.

```c
typedef struct Show { void *obj; const lk_types__Show__vtable *vt; } Show;

Show first = ((Show){ (void *)p, &lk_types__Show__Person__vt });
first.vt->show(first.obj);
```

One indirect call, and no allocation. The value carries the object pointer, so
the collector roots it through the frame like any other managed local.

Write the concrete type when you know it. Write the interface when you need to
hold several types in one place. The source says which one you get.

## An implementation must be complete

```
$ lark check iface_bad.lark
error[LK0410]: the implementation is missing a function that the interface declares
  --> iface_bad.lark:10:6
   |
10 | impl Show for Robot {
   |      ^^^^ `Show` declares `size`
   |
 7 |     int size(Self this);
   |         ---- declared here
   |
help: define `size` in this implementation
```

## Generics

A generic is one definition that the compiler copies per set of type
arguments. Rule G-1 puts each copy in the module that declares the generic, so
two modules that use `Box<int>` share one definition.

```c
managed struct Box<T> { T item; }

T larger<T>(T left, T right) { ... }
```

The emitted C names each copy.

```c
struct lk_types__Box__i { int item; };
static int lk_types__larger__i(int left, int right) { ... }
```

There is no type erasure, no boxing, and no dispatch. `Box<int>` holds an
`int`, and `larger<int>` is a function that takes two `int` values.

Nesting works, and the inner copy comes first in the emitted C.

```c
auto outer = new Box<Box<int>> { };
```

## An interface with parameters

An interface takes generic parameters, and each set of arguments is a separate
interface.

```c
iface Seq<T> {
    T get(Self this, int index);
}

impl Seq<int> for Counter { ... }
impl Seq<char> for Letters { ... }
```

`Seq<int>` and `Seq<char>` each carry their own method table, because `get`
returns a different type in each. Rule O-25 gives the reason, and rule O-26
makes the implementation name the one it satisfies.

A parameter and `Self` are independent. `Self` is the type that implements the
interface, and a parameter is an argument the instantiation supplies.

## A generic that holds a managed field

The `managed` marker goes on the generic, and each instantiation decides
whether it needs an object header.

```c
managed struct Box<T> { T item; }

Box<int> plain;                       /* no managed field, no header  */
gc Box<gc Person*>* boxed = ...;      /* a managed field, so a header */
```

Leave the marker off and the compiler reports the instantiation that needs it.

```
$ lark check g11.lark
error[LK0400]: this struct needs the `managed` marker
 --> g11.lark:5:8
  |
5 |     Box<gc Person*> b;
  |        ^^^^^^^^^^^^ this instantiation of `Box` holds a managed field
  = note: rule G-11. `Box` carries no `managed` marker, and the collector needs a field map for an object that holds a `gc` field
  |
help: write `managed struct Box<...>`
```

The reason is rule M-5. The collector needs a field map for every object that
holds a managed pointer, and it builds that map from the marker.

## Inferring the type argument

A call can leave the list out when the arguments give the answer.

```c
larger<int>(3, 9);    /* written out */
larger(3, 9);         /* rule G-6 infers int */
```

Write it out when the reader would have to work for it.

## The rules you meet first

| Rule | What it says |
|---|---|
| O-18 | The receiver adapts to the form the method declares. |
| O-19 | A concrete receiver gives a direct call. |
| O-24 | An interface value holds a managed pointer, so it gets a slot. |
| O-25 | An interface takes parameters, and each set of arguments is one interface. |
| O-26 | An implementation names the instantiation it satisfies. |
| G-1 | Each instantiation is emitted in the module that declares the generic. |
| G-6 | A call with no argument list infers the arguments. |
| G-11 | An instantiation that holds a managed field needs the `managed` marker. |
| G-13 | Each instantiation gets its own field map. |

---

Next: [Modules and packages](05-modules.md).
