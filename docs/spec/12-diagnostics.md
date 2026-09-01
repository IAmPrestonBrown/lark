# 12 - Diagnostics

## 1. Codes

Every diagnostic has a stable code. A test asserts the code, not the message
text. A message can improve without a change to the test suite.

| Range | Area |
|---|---|
| `LK01xx` | Lexical and syntax |
| `LK02xx` | Types |
| `LK03xx` | Memory and the managed boundary |
| `LK04xx` | Managed structs and interfaces |
| `LK05xx` | Generics |
| `LK06xx` | Modules and names |
| `LK07xx` | Initialization |
| `LK08xx` | C interoperation |
| `LK09xx` | Configuration and build |

## 2. Catalogue

### Lexical and syntax

| Code | Rule | Message |
|---|---|---|
| `LK0100` | L-6 | unresolved name before `<`, cannot decide generic or comparison |
| `LK0101` | L-3 | `@` directive is unknown |
| `LK0102` | n/a | unterminated generic argument list |
| `LK0103` | L-10 | the block comment does not end before the end of the file |
| `LK0104` | L-11 | the literal does not end on the line where it starts |
| `LK0105` | L-12 | the character cannot start a token |
| `LK0110` | n/a | the parser expected a different token |

### Types

| Code | Rule | Message |
|---|---|---|
| `LK0200` | T-2 | `gc` applies only to a pointer type |
| `LK0210` | T-9 | `auto` declaration needs an initializer |
| `LK0211` | T-11 | `auto` is not valid in this position |

### Memory and the managed boundary

| Code | Rule | Message |
|---|---|---|
| `LK0301` | T-5 | no implicit conversion between a managed pointer and a raw pointer |
| `LK0310` | M-2 | a managed pointer cannot live here |
| `LK0311` | M-3 | a managed struct cannot live in unmanaged memory |
| `LK0320` | M-8 | the selected collector does not support an interior pointer |
| `LK0330` | M-15 | `longjmp` crosses a frame that holds a managed local |
| `LK0340` | M-22 | a `gc_leaf` function cannot take a managed parameter |

### Managed structs and interfaces

| Code | Rule | Message |
|---|---|---|
| `LK0400` | O-2, G-11 | this struct needs the `managed` marker |
| `LK0410` | O-13 | the implementation is missing a function that the interface declares |
| `LK0411` | O-13 | the implementation declares a function that the interface does not |
| `LK0412` | O-14 | an interface applies only to a managed struct |
| `LK0413` | O-15 | an implementation must live with its interface or with its type |
| `LK0420` | O-18 | the address of a stack object is not a managed pointer |
| `LK0421` | O-21 | the method name is ambiguous across two interfaces |
| `LK0430` | O-12 | an interface function needs a receiver |
| `LK0440` | C-9 | an exported signature has no C form |

### Generics

| Code | Rule | Message |
|---|---|---|
| `LK0500` | G-8 | the instantiation depth limit is reached |
| `LK0501` | G-6a | a call to a generic function needs a type argument list |
| `LK0502` | G-2, L-7 | a generic argument must be a type |

### Modules and names

| Code | Rule | Message |
|---|---|---|
| `LK0600` | N-3 | the module is not found on the search path |
| `LK0610` | N-10 | an exported declaration names a private type |
| `LK0611` | N-11 | the name is not exported from that module |
| `LK0612` | N-2 | a module reference needs the `name::` prefix |
| `LK0613` | N-2 | no module with that name is imported here |
| `LK0614` | N-20 | a namespace block holds no type definition |

### Initialization

| Code | Rule | Message |
|---|---|---|
| `LK0700` | I-1 | no function carries the `init` marker |
| `LK0701` | I-1 | more than one function carries the `init` marker |
| `LK0710` | I-11 | `@init` names an unknown global block |
| `LK0711` | I-17 | this initializer reads a global that is not initialized yet |

### C interoperation

| Code | Rule | Message |
|---|---|---|
| `LK0800` | C-9 | this type has no C representation, so C cannot call this function |
| `LK0801` | C-6 | an unknown extension is skipped. Warning, not an error |

### Configuration

| Code | Rule | Message |
|---|---|---|
| `LK0900` | F-1 | the configuration field is unknown |
| `LK0901` | R-1 | the collector name is unknown |

## 3. Severity

Every code in the catalogue is an error, except the codes in this list.

**Warning codes:** `LK0801`

A warning does not stop the compiler from accepting the input.

## 4. Message quality

**Rule DQ-1.** Every diagnostic reports the source file, the line, and the
column of the original `.lark` file, never of the preprocessed text.

**Rule DQ-2.** A diagnostic that comes from an instantiation reports both the
error location and the instantiation location.

**Rule DQ-4.** A later pass reports nothing inside a construct that already
carries a diagnostic from an earlier pass. One problem produces one report.

A declaration that the parser could not read has no reliable type, so the type
checks skip it.

**Rule DQ-3.** A diagnostic for a boundary error states the fix. For `LK0301`,
the message names the exact cast to write.

The renderer in `lark-diag` produces this format. A test in that crate checks
it, so the example below and the code stay the same.

```
error[LK0301]: no implicit conversion between a managed pointer and a raw pointer
   --> app.lark:104:24
    |
104 |     handle_opaque_data(count);
    |                        ^^^^^ this is `gc Data<int>*`, the parameter is `void*`
    |
help: write the cast
    |
104 |     handle_opaque_data((void*)count);
    |                        +++++++
```
