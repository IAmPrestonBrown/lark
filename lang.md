# Lark: Language Philosophy

Lark is a C-compatible systems programming language built around a simple idea:

> **Keep C's level of control and simplicity, but remove the parts of C that make ordinary software unnecessarily tedious.**

Lark is not intended to be a "safer C," a Rust replacement, or a high-level language disguised as a systems language. The goal is to preserve the fundamental feeling of C: the programmer should understand and control what the machine is doing.

The difference is that Lark provides a small set of powerful abstractions for things that C leaves entirely to the programmer.

## Core Philosophy

### 1. C should remain the foundation

Lark should feel immediately familiar to a competent C programmer.

Types, pointers, structs, functions, layouts, calling conventions, and interaction with the operating system should remain understandable in terms of C and the underlying machine.

Ordinary code should not incur hidden runtime behavior.

If a programmer does not opt into an abstraction, they should get something close to ordinary C semantics.

### 2. Abstractions must be explicit

One of Lark's most important principles is:

> **If Lark adds meaningful runtime or semantic machinery, the source code should make that obvious.**

For example, managed memory should be visibly different from an ordinary pointer.

Runtime-aware types should be explicitly marked.

Interfaces should be explicitly declared.

The programmer should never have to wonder whether an innocuous-looking piece of code secretly introduced a GC dependency, dynamic dispatch, allocation, or other significant runtime behavior.

Lark should avoid "magic."

### 3. Make high-level programming pleasant without making it high-level

Lark should make it pleasant to write software involving:

* dynamic data structures
* strings
* trees and graphs
* compilers and interpreters
* networking
* tooling
* complex application state
* collections
* large object graphs

These things are possible in C, but C often forces the programmer to spend enormous amounts of effort implementing memory management, generic containers, module systems, and other infrastructure rather than solving the actual problem.

Lark should provide these capabilities while retaining C's underlying model.

### 4. Memory management should be flexible, not ideological

Lark should support garbage collection, but GC should not define the entire language.

Managed and unmanaged memory should coexist.

A programmer should be able to write ordinary manually managed C-like code alongside garbage-collected code.

Different programs should be able to choose different memory-management strategies, including potentially:

* no GC
* conservative GC
* precise GC
* moving GC
* non-moving GC
* custom collectors

The language defines the mechanisms and contracts necessary for these systems, rather than forcing every program into one memory-management philosophy.

### 5. No hidden crossings between abstraction levels

Managed and unmanaged worlds should be deliberately separated.

Conversions between them should be explicit rather than implicit.

There should be no situation where an ordinary-looking type conversion can accidentally turn a managed reference into an unmanaged pointer or vice versa.

The source code should make these boundaries visible.

This principle extends beyond memory: Lark should generally prefer explicitness whenever crossing between fundamentally different semantic models.

### 6. Powerful primitives, small language

Lark should resist accumulating features simply because other modern languages have them.

The goal is not to maximize the number of language features.

The goal is to find a small set of orthogonal primitives that compose extremely well.

Generics, interfaces, objects, GC, modules, and other facilities should ideally be understandable as extensions of a small underlying model rather than independent piles of special cases.

A useful question for every proposed feature is:

> **Can this be expressed using existing primitives instead?**

If so, prefer the existing primitives.

### 7. The runtime should not own the programmer

Lark should make it possible to build sophisticated software without requiring the programmer to adopt a particular runtime architecture.

The runtime should be modular and replaceable.

A tiny program should be able to have a tiny runtime.

A program that needs sophisticated GC or other facilities should be able to opt into them.

The programmer should remain in control of what exists between their code and the operating system.

### 8. C interoperability is fundamental

Lark should not create a new isolated ecosystem.

Existing C libraries, operating-system APIs, tools, and code should remain useful.

Lark code should be able to interact naturally with C code, and Lark's fundamental data representations should remain compatible with C wherever practical.

The purpose is not to abandon the C ecosystem, but to make it substantially nicer to program against.

### 9. The package/module system should be boring

Lark should have a simple, modern package system because C's header/linker-based module conventions become painful as projects grow.

Packages should be easy to create, depend on, version, and distribute.

A package should not require a massive ecosystem or complicated build system to function.

Repositories such as GitHub should be usable as package sources, with registries being an optional convenience rather than a requirement.

The package system should solve dependency management without becoming another language of its own.

## What Lark Should Feel Like

The ideal Lark program should make a programmer think:

> "This is basically C, except I don't have to fight the language."

A C programmer should be able to understand Lark code without learning an entirely new programming paradigm.

At the same time, a Lark programmer should be able to write things that would be unnecessarily cumbersome in C:

```text
gc objects
generic containers
interfaces
modules
packages
dynamic data structures
automatic memory management
```

without sacrificing the ability to drop down to raw pointers, explicit allocation, C APIs, and machine-level control.

## What Lark Is NOT

Lark should not become:

* Rust with different syntax
* C++ with a cleaner design
* Java/Go with pointers
* a language that hides its runtime
* a language where everything is automatically managed
* a language with dozens of competing abstraction mechanisms
* a language that sacrifices C interoperability for purity
* a language that tries to prevent programmers from doing dangerous things

Lark is allowed to let programmers do dangerous things.

It should simply make **intentional danger explicit** and make common abstractions pleasant.

## The Ultimate Goal

The ideal Lark language is surprisingly small.

Its power should come from the fact that its primitives compose well, not from having an enormous feature set.

The guiding question throughout the project should be:

> **How much better can we make programming while changing as little as possible about the fundamental C model?**

Lark should feel like someone took C, identified the parts that cause unnecessary friction in real software development, and carefully added mechanisms for solving those problems without taking control away from the programmer.
