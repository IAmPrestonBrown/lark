# Working rules for this repository

Read `docs/conventions.md` before you write anything. It is the authority for
style, checks, and process. This file is a summary.

## Before you write

1. Read `docs/conventions.md`.
2. Find the specification rule that covers the change. The rules live in
   `docs/spec/`. Name the rule in the code comment and in the commit message.
3. Find the phase in `docs/build-plan.md`. Work stays inside the current phase.

## Hard rules

- **ASCII only.** Every tracked file holds only ASCII bytes. No em dash. No
  curly quote. No arrow character. Rule C-1.1.
- **Simple English.** Short sentences. Active voice. No contraction. No
  semicolon in prose. One word for one meaning. Rule C-1.2.
- **No tool attribution.** No file, comment, commit message, or trailer names
  an AI assistant or a language model. Rule C-1.3.
- **Clean gates.** `cargo fmt --all --check`, then
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`, then
  `cargo test --workspace`. All three pass with no warning before any push.
  Rule C-2.1.
- **Idiomatic Rust.** Index based data structures, not reference counted
  graphs. Diagnostics for user errors, `Result` for internal failures. No
  `unwrap` in library code. Rules C-2.3 and C-2.4.
- **Current dependencies.** Add with `cargo add`. Never write a version number
  from memory. Rule C-4.1.

## Before you push

```
./scripts/check.sh
```

It runs every check in `docs/conventions.md` section 6. No push happens until it
passes.

## Test first

Every specification rule maps to at least one test. See `docs/test-strategy.md`.
The harness exists before the compiler. Write the failing test first.
