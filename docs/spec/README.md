# Lark Specification

Status: **draft 0.1**. This directory defines the Lark language, its runtime
contract, and its C output.

Read the chapters in order. Each chapter states normative rules. A rule with an
identifier, for example `M-3`, is referenced from the test suite and from the
diagnostic catalogue.

| Chapter | Subject |
|---|---|
| [00-overview.md](00-overview.md) | Scope, the superset contract, delivery phases |
| [01-lexical.md](01-lexical.md) | Tokens, contextual keywords, the disambiguation rule |
| [02-types.md](02-types.md) | The type system, `gc`, `managed`, `auto`, conversions |
| [03-memory.md](03-memory.md) | Memory model, roots, safepoints, foreign calls |
| [04-structs-interfaces.md](04-structs-interfaces.md) | Object layout, `iface`, `impl`, dispatch |
| [05-generics.md](05-generics.md) | Monomorphic generics |
| [06-modules.md](06-modules.md) | `@import`, namespaces, `export` |
| [07-init-globals.md](07-init-globals.md) | `init`, `@global`, `@init` |
| [08-c-interop.md](08-c-interop.md) | Preprocessor, headers, extern declarations |
| [09-codegen.md](09-codegen.md) | Emitted C, name mangling, debug mapping |
| [10-runtime.md](10-runtime.md) | The runtime API and the collector interface |
| [11-config.md](11-config.md) | `lark.toml` and command line overrides |
| [12-diagnostics.md](12-diagnostics.md) | Diagnostic codes |

Supporting documents:

- [../decisions.md](../decisions.md) records every design decision and its reason.
- [../grammar/lark.ebnf](../grammar/lark.ebnf) is the formal grammar delta over ISO C11.

## Rule identifiers

| Prefix | Area |
|---|---|
| `L-` | Lexical |
| `T-` | Types |
| `M-` | Memory |
| `O-` | Objects and interfaces |
| `G-` | Generics |
| `N-` | Modules and names |
| `I-` | Initialization |
| `C-` | C interoperation |
| `X-` | Code generation |
