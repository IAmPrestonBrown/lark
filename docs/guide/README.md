# The Lark Guide

This guide takes you from nothing to a working managed program. Read it in
order the first time. Every example in it compiles and runs.

| Page | What it covers |
|---|---|
| [1. Getting started](01-getting-started.md) | Install the compiler. Build and run a program. |
| [2. Coming from C](02-from-c.md) | What Lark adds, and what it leaves alone. |
| [3. Managed memory](03-memory.md) | `gc`, `managed`, `new`, and what the collector does. |
| [4. Interfaces and generics](04-types.md) | `iface`, `impl`, and one definition for many types. |
| [5. Modules and packages](05-modules.md) | `@import`, `export`, and a dependency. |
| [6. Startup and globals](06-startup.md) | `init`, `@global`, and the order they run in. |
| [7. Talking to C](07-c-interop.md) | Headers, foreign calls, and the two markers. |
| [8. The tools](08-tools.md) | The formatter, the editor, the debugger, the collectors. |

## What this guide is not

It is not the specification. The specification in [../spec/](../spec/) is the
authority, and it states every rule with a number. A guide that repeats it
would go stale, so this one points at it instead: when a page says **rule
M-10**, that rule is in the specification and it says exactly what happens.

It is not a reference either. It leaves out constructs that a first reader does
not need. [examples/tour.lark](../../examples/tour.lark) holds every construct
in one file, and the specification explains each one.

## The shape of the language

Lark is C with four additions. Each one carries a marker that a reader sees.

| Addition | The marker | The page |
|---|---|---|
| Garbage collection | `gc`, `managed`, `new` | [3](03-memory.md) |
| Interfaces | `iface`, `impl` | [4](04-types.md) |
| Generics | `Name<T>` | [4](04-types.md) |
| Modules | `@import`, `export` | [5](05-modules.md) |

Nothing is implicit. A pointer that the collector manages says `gc`. A struct
that carries an object header says `managed`. A call that a collection can run
during says `gc_safe`. You read the cost at the call site, not in a manual.
