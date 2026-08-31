# 00 - Overview

## 1. What Lark is

Lark is a systems programming language that translates to C11 source code. Lark
keeps the C machine model. Lark adds a small set of primitives for garbage
collection, interfaces, generics, and modules. Every primitive carries an
explicit marker in the source text.

The reference implementation is a transpiler written in Rust. The transpiler
emits C11 and calls a C compiler to produce a binary.

## 2. The superset contract

**Rule S-1.** Every strictly conforming ISO C11 translation unit is a valid Lark
translation unit. Its meaning does not change.

**Rule S-2.** Lark adds no reserved word. Every Lark keyword is *contextual*. A
contextual keyword is recognized only at a source position where valid C11
cannot parse. Chapter 01 defines each position.

**Rule S-3.** Lark accepts the GNU and Clang extensions that appear in the
system headers of the supported platforms. Chapter 08 lists them. Lark does not
promise to model the semantics of an extension. Lark promises to parse past it.

Rule S-2 is the reason Lark does not add `var`, `let`, `func`, or `class`. Each
of those names is a legal identifier in C. A program that uses one as a typedef
name breaks under a language that reserves it.

## 3. Design constraints

These constraints come from the project philosophy. The specification treats
them as normative. A proposed feature that violates one is rejected.

| ID | Constraint |
|---|---|
| D-1 | Code that opts into no Lark feature behaves as C, and costs what C costs. |
| D-2 | Every construct that adds runtime or semantic machinery carries a visible marker. |
| D-3 | A crossing between the managed world and the unmanaged world is always explicit. |
| D-4 | The memory strategy is a program level choice, not a language level one. |
| D-5 | A new feature must not duplicate an existing primitive. |
| D-6 | The runtime is modular. A program pays only for the parts it uses. |
| D-7 | C data layout and C calling conventions stay intact. |

## 4. Delivery phases

The end state is one language. The phases control the order of work.

**Phase A - Lark core.**
The front end parses every Lark construct and the subset of C that Lark itself
needs. A `#include` directive passes through to the output without type
analysis. `@import` works fully. The full test suite runs. An LSP serves
completions for Lark symbols.

**Phase B - C declarations.**
The front end parses full C11 declarations from preprocessed headers. A call to
`printf` type-checks. An LSP serves completions for C symbols.

**Phase C - full superset.**
The front end parses full C11 statements and expressions. A `.c` file compiles
unchanged as a `.lark` file. Rule S-1 holds completely.

## 5. Terminology

**Managed memory.** Memory that the collector owns and reclaims.

**Unmanaged memory.** All other memory. Static storage, the stack, and memory
from `malloc`.

**Managed pointer.** A pointer with the `gc` qualifier. It refers to managed
memory. Chapter 02 defines it.

**Managed struct.** A struct declared with `managed`. It carries an object
header.

**Root.** A location that the collector treats as live without tracing. Chapter
03 lists the root sets.

**Safepoint.** A program point where the thread state is consistent, so a
collection can start.

**Collector.** The component that reclaims managed memory. A program selects one
collector at build time.

## 6. Conformance

An implementation conforms when it satisfies three conditions.

1. It accepts every program that this specification declares valid.
2. It rejects every program that this specification declares invalid, and it
   reports the diagnostic code from chapter 12.
3. Its emitted C compiles under a conforming C11 compiler.

## 7. Open items

This draft does not yet define the standard library. It defines the language and
the runtime contract that a standard library needs.
