# 8. The tools

## The formatter

`lark fmt` writes one shape and takes no options.

```sh
lark fmt src/*.lark            # rewrite in place
lark fmt --check src/*.lark    # exit non zero when a file would change
```

```c
int f(int a,int b){if(a>b){return a;}else{return b;}}
```

becomes

```c
int f(int a, int b) {
    if (a > b) {
        return a;
    } else {
        return b;
    }
}
```

The formatter writes from the same tree the compiler reads, so it never changes
what a program means. Two properties hold on every run: the tokens come back
unchanged, and a second pass changes nothing.

## The editor

There is a Visual Studio Code extension in
[editors/vscode/](../../editors/vscode/).

```sh
cd editors/vscode
npm install
npx @vscode/vsce package
code --install-extension lark-lang-0.1.0.vsix
```

Highlighting needs no compiler. Completion, hover, go to definition, and errors
as you type need `lark-lsp` on your `PATH`.

```sh
cargo install --path crates/lark-lsp
```

The server speaks LSP over stdio, so any editor that speaks LSP can use it.

## The debugger

The emitted C carries `#line` directives back to your source, so a debugger
already names the Lark file and the Lark line. The build writes two scripts
that add the rest.

```sh
lldb ./build/hello
(lldb) command script import build/lark_lldb.py

gdb ./build/hello
(gdb) source build/lark_gdb.py
```

They add commands that read the runtime: the shadow stack of a thread, the
header of an object, and the field map of a type.

Debug information is on by default. Turn it off for a release build.

```toml
[build]
debug = false
```

## The collectors

`lark.toml` picks one, and the build enforces what it can do.

```toml
[gc]
strategy = "precise-marksweep"
```

| Name | Reclaims | Interior pointers | Moving | When to use it |
|---|---|---|---|---|
| `precise-marksweep` | yes | yes | no | The default. The fewest surprises. |
| `arena` | no | yes | no | A short program that exits before memory matters. |
| `semispace` | yes | no | yes | Allocation heavy work, compact heap. |
| `generational` | yes | no | yes | Many short lived objects. |

A moving collector cannot follow a pointer into the middle of an object, and
the build says so rather than letting it fail at run time.

```
$ lark check interior.lark
error[LK0320]: the selected collector does not support an interior pointer
 --> interior.lark:4:20
  |
4 |     gc char* mid = &buf[3];
  |                    ^^^^^^^ this takes the address of an element inside `buf`
  = note: a collector that moves an object cannot follow an interior pointer, because the pointer has no root of its own to rewrite
  |
help: hold the whole object, and pass the index beside it
```

Rule R-1 checks every capability this way. Change one line in `lark.toml`, and
the build tells you what no longer holds.

## Torture mode

Every safepoint runs a full collection.

```toml
[gc]
torture = true
```

A correct program gives identical output either way. A program that loses a
root gives different output, or crashes, on the first run rather than in a
month. Run your tests under it.

## The build cache

`lark build` keys every object file by content, in `build/.lark-cache/`. A
rebuild that changes nothing compiles nothing. The key covers the source, every
header it reads, and every build setting, so a stale object is never reused.

`build/lark-build.toml` records the settings that produced the directory. That
file and `lark.lock` together name everything that changes an output.

## Measuring

`benchmarks/` holds five programs that run against every collector.

```sh
cd benchmarks
./run.sh              # the full workload
./run.sh --quick      # the small workload, which the gate runs
```

The table gives milliseconds, collections, and peak heap, one column per
collector. Read two collectors in the same run, never two runs on different
machines.

Build with `opt = "2"` before you measure anything. The default build is a
debug build, and measuring one of those measures nothing.

```toml
[build]
opt = "2"
debug = false
```

## The gate

The project checks itself with one command.

```sh
./scripts/check.sh
```

It runs the text checks, `cargo fmt`, `clippy` with `-D warnings`, every Rust
test, and the runtime suite against all four collectors under two sanitizers.
It also checks that every numbered rule in the specification has a test, and it
runs the benchmarks at their small size to prove that the four collectors agree
on the answer.

---

That is the whole toolchain. The [specification](../spec/) answers what this
guide leaves open, and every rule in it carries a number that a diagnostic
points at.
