# 13 - Packages

## 1. What a package is

A package is a git repository holding a `lark.toml` with a `[package]` table.
There is no upload step, no server to run, and no format beyond what git
already stores.

**Rule K-1.** An **index** is a git repository holding one TOML file per
package. The file names the source repository and every published version. An
index is the source of truth for what versions exist.

```toml
# js/on/json.toml
name = "json"
repository = "https://github.com/preston/lark-json"

[[version]]
version = "1.2.0"
commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"

[[version]]
version = "1.1.0"
commit = "3f7a1c9d2e8b4a6057913f2e8d4c6b0a97531e42"
yanked = true
reason = "the parser accepted a trailing comma"
```

The path of a file inside an index spreads the name over two levels, so no
directory grows to thousands of entries.

| Name | Path |
|---|---|
| `a` | `1/a.toml` |
| `at` | `2/at.toml` |
| `net` | `3/n/net.toml` |
| `json` | `js/on/json.toml` |

The lookup ignores case, so two packages cannot differ by case alone.

## 2. Naming a dependency

**Rule K-2.** A project names a dependency in one of three ways, and it uses
any of them in one file.

```toml
[registry]
main = { git = "https://github.com/preston/lark-index" }

[dependencies]
json = "1.2.0"                                            # through an index
http = { version = "^0.4", registry = "main" }             # through an index
zlib = { git = "https://example.com/zlib", tag = "v2.1" }  # direct
local = { path = "../lark-http" }                          # on disk
```

A dependency reads from one source. An entry that names two is an error, and so
is an entry that names none.

## 3. Versions

**Rule K-3.** An index entry pins a full commit hash. A tag moves and a branch
moves, so neither is a version. A reader refuses an entry that names anything
else.

That rule is what makes an index worth having. A direct dependency trusts
whoever controls the tag. An index dependency trusts a hash, and a hash cannot
change under it.

**Rule K-4.** The dependency graph is flat. One version of one package per
build. Every requirement for a package holds at the same time, and the
resolution takes the highest version that satisfies them all. When none does,
the error names every requirement and the path that asked for it.

```text
no version of `json` satisfies every requirement
  this project asks for =1.0.0
  http asks for ^2
  the index lists 1.0.0, 2.0.0
```

A range resolves against an index alone, because a range needs a list of
versions and only an index has one. A direct dependency names a tag, a branch,
or a commit, and gets no range.

**Rule K-5.** A tag and a branch both move, so a build that depends on one
warns once. The lock file records what the reference pointed at, so the build
repeats.

**Rule K-6.** A yanked version never resolves fresh. A lock file that already
names one keeps working, so an existing build does not break when a version is
withdrawn.

## 4. The lock file

**Rule K-7.** `lark.lock` records the commit that every package resolved to,
direct and transitive. A build with a lock file fetches by commit and reads no
index at all.

```toml
version = 1

[[package]]
name = "json"
version = "1.2.0"
source = "registry+main"
repository = "https://github.com/preston/lark-json"
commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"
```

The lock file is what makes a build reproducible, as rule F-2 makes the build
settings reproducible. The two together name everything that changes an output.

A lock file states its format. A reader that meets a number it does not know
says so, rather than reading the file wrongly.

## 5. Where a package lives

**Rule K-8.** A dependency is a module search root. `@import json` finds
`json.lark` inside the package, because rule N-3 already searches a path and a
dependency adds one entry to it.

Two packages that export the same name collide at link time, which rule X-5c
already states for two modules. Lark renames nothing.

**Rule K-9.** A fetched package lives read only under `LARK_HOME`, shared
between projects. A project holds no copy of its own.

```text
~/.lark/
  index/<host>/<owner>/<repo>/              a clone of an index
  store/<host>/<owner>/<repo>/<commit>/     one version of one package
```

The default is `~/.lark`. The test suite sets `LARK_HOME`, so no test touches a
real home directory.

## 6. Publishing

**Rule K-10.** Publishing is a pull request against an index repository: one
file, one new `[[version]]` entry. The tool writes the entry and pushes
nothing.

```text
$ lark publish
# add this to js/on/json.toml in your index

name = "json"
repository = "https://github.com/preston/lark-json"

[[version]]
version = "1.2.0"
commit = "9c1f2ab4e8d7c6b5a4938271605f4e3d2c1b0a99"
```

## 7. Commands

| Command | What it does |
|---|---|
| `lark add <name>@<version>` | Add a dependency through the index |
| `lark add <git-url> [--tag t]` | Add a dependency directly |
| `lark update [<name>]` | Refetch and rewrite the lock file |
| `lark tree` | Print the dependency graph |
| `lark vendor` | Copy every dependency into `./vendor` |
| `lark publish` | Print the index entry to submit |

`lark add` edits `lark.toml` as text, so every comment in the file survives.

## 8. What is out of scope

| Left out | Why |
|---|---|
| A build script | A package that runs code at build time is a hazard the language does not need. |
| A hosted index service | An index is a git repository. Anyone hosts one. |
| Credential handling | The tool runs `git`, which already handles it. |
| Two versions of one package | Rule K-4 allows one. The error names both paths. |
