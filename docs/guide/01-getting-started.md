# 1. Getting started

## Install

Lark needs a Rust toolchain to build the compiler, and a C compiler to build
what the compiler produces. Any `cc` that accepts `-std=c11` works.

```sh
cargo install --path crates/lark-cli     # the `lark` command
cargo install --path crates/lark-lsp     # the editor server, optional
```

Both land in `~/.cargo/bin`, which is normally on your `PATH` already.

```sh
lark
```

That prints the commands.

## Your first program

Lark is a superset of C11, so your first program is a C program.

```c
/* hello.lark */
#include <stdio.h>

int main(void)
{
    printf("hello from Lark\n");
    return 0;
}
```

A project needs one configuration file beside it.

```toml
# lark.toml
[package]
name = "hello"
version = "0.1.0"
```

Then build it and run it.

```sh
$ lark build hello.lark
build/hello

$ ./build/hello
hello from Lark
```

That is the whole cycle. `lark build` writes the emitted C, compiles it, and
links a binary into `build/`.

## The three commands you use most

| Command | What it does |
|---|---|
| `lark check <file>` | Reads the program and reports problems. Writes nothing. |
| `lark build <file>` | Checks it, emits C, and links a binary. |
| `lark emit <file>` | Prints the emitted C, so you can read the cost. |

Each one takes `--section.field=value`, which sets a configuration field for
that run. Rule F-1.

```sh
lark build --gc.strategy=semispace --build.opt=2 hello.lark
```

`lark check` is the fast one. Use it while you write, and `lark build` when you
want to run something.

## What the build writes

```
build/
  hello              the binary
  hello.c            the emitted C
  hello.lark.h       the exported declarations
  lark-build.toml    the settings that produced this directory
  lark_lldb.py       a debugger script
  lark_gdb.py        a debugger script
  .lark-cache/       object files, keyed by content
```

Nothing there is hidden. `hello.c` is ordinary C that you can read, and it
carries `#line` directives back to your source, so a compiler error or a
debugger names the Lark file and the Lark line.

Read it once. It is the fastest way to learn what a construct costs.

```sh
lark emit hello.lark
```

## When something is wrong

A diagnostic names a numbered rule, and that rule is in the specification.

```
$ lark check wrong.lark
error[LK0700]: no function carries the `init` marker
 --> wrong.lark:1:1
  |
1 | managed struct Person {
  | ^ no function carries the `init` marker
  = note: rule I-3 puts the runtime startup in that function
  |
help: write `init` before the entry point, as in `init void main(void)`
```

Three things are always there: the code, the rule that decides it, and a fix.
If the help line is not enough, `docs/spec/` explains rule I-3 in full.

## Set up your editor

There is a Visual Studio Code extension in
[editors/vscode/](../../editors/vscode/).

```sh
cd editors/vscode
npm install
npx @vscode/vsce package
code --install-extension lark-lang-0.1.0.vsix
```

Highlighting works with no compiler installed. Completion, hover, go to
definition, and errors as you type need `lark-lsp` on your `PATH`.

---

Next: [Coming from C](02-from-c.md).
