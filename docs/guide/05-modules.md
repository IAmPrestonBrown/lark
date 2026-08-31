# 5. Modules and packages

A module is one `.lark` file. A package is a git repository holding a
`lark.toml`. There is nothing else to learn.

## Two files

Write a module. Mark what other modules can see.

```c
/* geometry.lark */
@import stdio

export managed struct Circle {
    int radius;
}

export int area(gc Circle* c) {
    return 3 * c->radius * c->radius;
}

// No `export`, so no other module sees this name.
int secret(void) { return 1; }
```

Import it, and name what you use.

```c
/* main.lark */
@import stdio
@import geometry

init int main(void) {
    auto c = new geometry::Circle { .radius = 4 };
    stdio::printf("area %d\n", geometry::area(c));
    return 0;
}
```

```sh
$ lark build main.lark && ./build/main
area 48
```

Tell the compiler where to look.

```toml
# lark.toml
[paths]
search = ["."]
```

## Two rules cover the whole system

**A name is private unless it says `export`.** Rule N-6.

```
$ lark check bad.lark
error[LK0611]: the name is not exported from that module
 --> bad.lark:2:40
  |
2 | init int main(void) { return geometry::secret(); }
  |                                        ^^^^^^ `secret` is private to module `geometry`
  |
help: write `export` before the function
```

**A name from another module is written with the module.** Rule N-4. There is
no `using`, and no way to pull a name into your scope without saying where it
came from. A reader always knows.

```c
geometry::area(c)      /* always written this way */
```

The emitted C keeps only the name, so `geometry::area` links as `area`. Rule
X-5 gives the mapping, and it is what makes a Lark module callable from C.

## Cycles are an error

Two modules that import each other stop the build. Rule N-5 reports the whole
cycle, so you see every step.

## Packages

A package is a git repository with a `lark.toml`. There is no upload step and
no server.

```sh
lark add https://github.com/preston/lark-json --tag v1.2.0
```

That writes the dependency and a `lark.lock`, and then `@import json` works.

An **index** is a git repository listing packages and their versions. Add one,
and you name a dependency by version instead of by URL.

```toml
[registry]
main = { git = "https://github.com/preston/lark-index" }

[dependencies]
json = "1.2.0"                                             # through the index
zlib = { git = "https://example.com/zlib", tag = "v2.1" }  # direct
local = { path = "../lark-http" }                          # on disk
```

An index entry pins a full commit hash, never a tag. Rule K-3 makes the reader
refuse anything else, because a tag moves and a commit does not.

| Command | What it does |
|---|---|
| `lark add <name>@<version>` | Adds a dependency through an index. |
| `lark add <git-url> --tag <tag>` | Adds one directly. |
| `lark update` | Moves the lock file forward. |
| `lark tree` | Prints the dependency graph. |
| `lark vendor` | Copies every dependency into `vendor/`. |
| `lark publish` | Writes an index entry for your package. |

## The lock file

`lark.lock` records the commit that every package resolved to. A build with a
lock file fetches by commit and reads no index at all, so it repeats exactly.

```toml
[[package]]
name = "json"
version = "1.2.0"
source = "registry+main"
repository = "https://github.com/preston/lark-json"
commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"
```

Commit it. The lock file and `lark-build.toml` together name everything that
changes an output.

## The rules you meet first

| Rule | What it says |
|---|---|
| N-3 | A module name finds `<name>.lark` on the search path. |
| N-4 | A name from another module is written `module::name`. |
| N-5 | An import cycle is an error, and the report names the cycle. |
| N-6 | A name is private unless it says `export`. |
| K-1 | An index is a git repository, one file per package. |
| K-3 | An index entry pins a full commit hash. |
| K-4 | The graph is flat: one version of one package per build. |
| K-7 | `lark.lock` records the commit of every package. |

---

Next: [Startup and globals](06-startup.md).
