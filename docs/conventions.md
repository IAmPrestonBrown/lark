# Conventions

These rules apply to every file in this repository. Read this document before
you write code, documentation, a comment, or a commit message.

Every rule has an identifier. Every rule states how a machine checks it. A rule
that no check enforces is a rule that decays.

---

## 1. Text

### C-1.1 ASCII only
Every tracked file contains only ASCII characters, from `0x00` to `0x7F`.

This covers source code, comments, documentation, commit messages, test
fixtures, and configuration.

Write `-` instead of an en dash or an em dash. Write `"` instead of a curly
quote. Write `->` instead of an arrow. Write `...` instead of an ellipsis
character.

**Exception.** A test fixture that tests UTF-8 handling needs non-ASCII bytes.
Such a file lists itself in `.ascii-exempt`. The check reads that file.

**Check.** `scripts/check-ascii.sh`

### C-1.2 Simple English
Write short, plain sentences. A reader whose first language is not English must
understand the text on one read.

| Rule | Detail |
|---|---|
| Sentence length | 20 words for an instruction. 25 words for a description. |
| One idea | One instruction per sentence. |
| Voice | Active. Name the actor. |
| Tense | Simple present, simple past, simple future. |
| Condition first | Write "If the test fails, read the log." |
| Punctuation | No semicolon. Write two sentences. |
| Contractions | None. Write "does not", not "doesn't". |
| Vocabulary | One word for one meaning, through the whole document. |

Delete words that carry no fact: `simply`, `just`, `basically`, `powerful`,
`robust`, `seamless`, `comprehensive`, `leverage`, `in order to`,
`it is worth noting that`.

Replace `utilize` with `use`. Replace `prior to` with `before`. Replace
`in the event that` with `if`.

Use American spelling.

**Check.** `scripts/check-prose.py` flags a banned word, a contraction, a
semicolon in prose, and a sentence over 25 words.

**Exception.** A file listed in `.prose-exempt` is skipped. The list holds this
document, which states the banned words, and `lang.md`, which is source material
from the author.

### C-1.3 No tool attribution
No file in this repository mentions an AI assistant, a large language model, or
any product name of one.

This covers source comments, documentation, commit messages, commit trailers,
pull request text, issue text, and configuration.

Write commit messages as the author of the change. Add no `Co-Authored-By`
trailer for a tool. Add no session link. Add no generation notice.

Keep per-machine tool configuration out of the repository. A file that one
tool reads belongs in `.git/info/exclude`, which git never commits. It does
not belong in `.gitignore`, because that file is tracked and naming the tool
there breaks this rule.

**Exception.** This document and `AGENTS.md` state the rule, so they name the
subject. Both list themselves in `.attribution-exempt`. The two files that
carry the word list, `scripts/check-attribution.sh` and
`.githooks/commit-msg`, name it because the check cannot work otherwise. No
other file gets an entry.

**Check.** `scripts/check-attribution.sh` greps a word list over every tracked
file and over the commit range.

`.githooks/commit-msg` runs the same word list over one message, before the
commit exists. Enable it once per clone.

```sh
git config core.hooksPath .githooks
```

### C-1.4 Comments explain why
A comment states the reason for the code, or the invariant the code holds. A
comment that restates the code adds nothing.

A comment that implements a specification rule names the rule.

```rust
// Rule M-11: zero every slot before the push. A collection between the push
// and the first assignment must not read an uninitialized slot.
```

---

## 2. Rust

### C-2.1 Format and lint
Code passes three gates before any push.

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

No warning survives. A lint that does not fit gets an `#[allow]` on the
narrowest possible item, with a comment that gives the reason. A blanket
`#![allow]` at crate level needs an entry in `docs/decisions.md`.

### C-2.2 Lint configuration
Lints live in `[workspace.lints]` in the root `Cargo.toml`. Each crate opts in
with `[lints] workspace = true`. No `#![warn(...)]` block at the top of a file.

The workspace sets at least these:

```toml
[workspace.lints.rust]
missing_docs = "warn"
unsafe_code = "forbid"          # each crate that needs FFI overrides this
unused_qualifications = "warn"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
panic = "warn"
todo = "warn"
dbg_macro = "warn"
```

`clippy.toml` turns off the panic, unwrap, and expect lints inside a test. A
test proves a failure by panicking.

### C-2.3 Errors and diagnostics
A user error is a diagnostic, never a `Result`. The compiler collects
diagnostics and continues, so one run reports many problems.

A `Result` carries an internal failure: file input and output, a broken
invariant, or a subprocess failure.

Library crates define their error types with `thiserror`. The binary crate uses
`anyhow` at the top level only.

`unwrap` and `expect` do not appear in library code. Where a value cannot be
absent, the code states why in a comment and uses `expect` with a message that
explains the invariant.

### C-2.4 Data structures
The compiler uses index based data structures, not reference counted graphs. A
node holds a typed index into an arena. No `Rc<RefCell<T>>` appears in the
compiler crates.

This choice keeps the data cheap to copy, easy to serialize for the language
server, and free of cycles.

### C-2.5 Modules
Use `foo.rs` beside a `foo/` directory. Do not use `foo/mod.rs`.

Keep a module under about 500 lines. A larger module splits by concept, not by
line count.

### C-2.6 Public interface
Every public item has a documentation comment. The first line is one sentence.

A crate root documents what the crate does and what it does not do.

### C-2.7 Tests
Unit tests live in a `#[cfg(test)] mod tests` block in the file they test.
Integration tests live in `tests/`.

A test name states the behavior, not the function. Write
`reports_lk0301_for_implicit_gc_cast`, not `test_cast`.

Every specification rule maps to at least one test. See
[test-strategy.md](test-strategy.md) principle P-6.

### C-2.8 Unsafe code
The compiler crates forbid `unsafe`. Only a crate that binds to C overrides
this, and it states the override in its `Cargo.toml` with a comment.

Every `unsafe` block carries a `// SAFETY:` comment that states the invariant
the caller holds.

---

## 3. C code

The runtime is C11. It follows the same text rules from section 1.

### C-3.1 Style
Four space indent. No tab. Braces on the same line as the statement.

### C-3.2 Naming
Public symbols start with `lark_`. Public types start with `lark_`. A static
symbol needs no prefix.

### C-3.3 Warnings
The runtime builds clean under this set.

```
-std=c11 -Wall -Wextra -Wpedantic -Wconversion -Wshadow -Werror
```

### C-3.4 Sanitizers
Every runtime test runs under AddressSanitizer and UndefinedBehaviorSanitizer in
continuous integration.

---

## 4. Dependencies

### C-4.1 Current versions
Add a dependency with `cargo add`, which resolves the current version from the
registry. Do not write a version number from memory.

Run `cargo update` at the start of each phase. Review the diff.

### C-4.2 Audit
`cargo deny check` and `cargo audit` run in continuous integration. A new
dependency needs a license that `deny.toml` allows.

### C-4.3 Justify a dependency
A new dependency needs a line in `docs/decisions.md` that states what it does
and why the standard library does not suffice.

Prefer a small, focused crate. Prefer a crate that the Rust project itself uses.

### C-4.4 Toolchain
`rust-toolchain.toml` pins the toolchain and the components. The workspace uses
edition 2024.

---

## 5. Version control

### C-5.1 Commit messages
Use Conventional Commits.

```
feat(parser): recognize the gc type qualifier

The qualifier joins the C11 type-qualifier list, so it parses in every
position that const parses. Rule T-1.
```

The subject line is 72 characters or fewer. The subject uses the imperative
mood. The body states the reason for the change, not the content of the diff.

A commit that implements a specification rule names the rule.

Rule C-1.3 applies. No tool attribution appears in a commit message or a
trailer.

### C-5.2 Branches
Work on a branch. Do not commit to the default branch.

### C-5.3 One change per commit
A commit does one thing. A refactor and a behavior change go in two commits.

---

## 6. The gate

One command runs every check.

```
./scripts/check.sh
```

It runs, in order:

1. `scripts/check-ascii.sh`
2. `scripts/check-prose.py`
3. `scripts/check-attribution.sh`
4. `cargo fmt --all --check`
5. `cargo clippy --workspace --all-targets --all-features -- -D warnings`
6. `cargo test --workspace`
7. The runtime build under the flags in C-3.3
8. The runtime tests under the sanitizers in C-3.4
9. The specification coverage report from P-6

Continuous integration runs the same script. No push happens until it passes.
