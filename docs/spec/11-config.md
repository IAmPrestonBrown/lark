# 11 - Configuration

## 1. `lark.toml`

The file sits at the root of a package. The transpiler reads it before anything
else.

```toml
[package]
name    = "myapp"
version = "0.1.0"

[build]
cc     = "clang"
std    = "c11"
out    = "build/"
emit_c = true          # keep the generated C on disk
debug  = true          # pass -g, so a debugger names a local
opt    = "0"           # pass -O0. Use "2" for a release build

[gc]
strategy = "precise-marksweep"
roots    = "shadow-stack"     # or "conservative"
checks   = true               # verify a cast that adds gc, rule T-7
torture  = false              # collect at every safepoint, for tests

[paths]
search = ["./lib", "../shared"]
```

## 2. Fields

| Field | Type | Default | Meaning |
|---|---|---|---|
| `package.name` | string | required | The package name |
| `package.version` | string | `"0.0.0"` | The package version |
| `build.cc` | string | `"clang"` | The C compiler for `-E` and for the final build. See rule F-7 |
| `build.std` | string | `"c11"` | The C standard for the output |
| `build.out` | string | `"build/"` | The output directory |
| `build.emit_c` | bool | `true` | Keep the generated C |
| `build.debug` | bool | `true` | Pass `-g`. See rule Z-5 |
| `build.opt` | string | `"0"` | The optimization level. See rule F-5 |
| `build.runtime` | string | `""` | The runtime directory, or empty to find it |
| `gc.strategy` | string | `"precise-marksweep"` | The collector. Chapter 10 section 4 lists the names. |
| `gc.roots` | string | `"shadow-stack"` | The stack root mechanism |
| `gc.checks` | bool | `true` in debug | Runtime check on a cast that adds `gc` |
| `gc.torture` | bool | `false` | Collect at every safepoint. See rule F-3 |
| `paths.search` | list | `[]` | Directories for `@import`, rule N-3 |

## 3. Command line overrides

Every field has a command line flag. The flag wins over the file.

```
lark build --gc.roots=conservative --build.cc=clang
```

**Rule F-1.** A field name on the command line matches the dotted path in the
file.

**Rule F-2.** The transpiler records the effective configuration in the output
directory, so a build is reproducible.

**Rule F-3.** Torture mode holds `lark_gc_request` set, so every safepoint runs a
full collection. A correct program produces identical output under torture mode
and under normal mode. The test suite uses this to find a missing root, a missing
poll, and a missing barrier.

**Rule F-4.** The environment variable `LARK_GC_TORTURE=1` turns on torture mode
for a binary that the build did not configure for it.

**Rule F-7.** The default compiler is `clang` on every platform. One compiler
means one flag dialect, so a build behaves the same everywhere and a port to a
new platform needs no second set of flags.

`build.cc` names another compiler. A compiler that does not accept the flags
that rule F-5 and the warning set produce is the caller's problem, and the
build reports what it ran.

**Rule F-5.** `build.opt` becomes `-O<value>` on every compile, so the C
compiler decides what each level means. The default is `"0"`, which matches the
default `build.debug = true`: the plain build is a debug build. A release build
sets `opt = "2"`.

The level is part of the settings that rule F-2 records and part of the key
that rule Y-2 gives each object file, so a change to it rebuilds rather than
reusing an object built at another level.
