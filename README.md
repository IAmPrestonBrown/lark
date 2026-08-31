# Lark

Lark is a systems programming language that translates to C11.

Lark keeps the C machine model. Lark adds a small set of primitives for garbage
collection, interfaces, generics, and modules. Every primitive carries an
explicit marker in the source, so a reader sees the cost at the call site.

Lark is a strict superset of C11. A valid C file is a valid Lark file.

```c
@import stdio

export managed struct Person {
    gc char* name;
    int age;
}

export iface Greet {
    void say_hi(Self this);
}

impl Greet for Person {
    void say_hi(Person this) {
        stdio::printf("%s\n", this.name);
    }
}

init int main(void) {
    auto p = new Person { .name = "Preston", .age = 18 };
    p.say_hi();
    return 0;
}
```

## Start here

Download an archive from the releases page, unpack it, and put `bin` on your
`PATH`. The archive carries the runtime, so nothing else is needed.

Or build from source.

```sh
cargo install --path crates/lark-cli     # the `lark` command
cargo install --path crates/lark-lsp     # the editor server

lark build hello.lark
./build/hello
```

The [guide](docs/guide/) takes you from that to a working managed program.

## What works

| | |
|---|---|
| The language | Modules, managed memory, interfaces, generics, foreign calls |
| The C superset | A `.c` file compiles unchanged as a `.lark` file |
| Collectors | Four, chosen in `lark.toml`. One of them moves objects. |
| The toolchain | Package manager, incremental build, formatter, editor support |

The proof of the superset claim is a third party C library. The gumbo HTML
parser, 32,979 lines across seventeen files, compiles through Lark under its
own file names, links, and prints what the same library built by `cc` prints.

## Documents

| Document | Subject |
|---|---|
| [docs/guide/](docs/guide/) | How to write Lark, from nothing to a managed program |
| [docs/spec/](docs/spec/) | The language specification, sixteen chapters |
| [docs/decisions.md](docs/decisions.md) | Every design decision and its reason |
| [docs/conventions.md](docs/conventions.md) | Style, checks, and process |
| [docs/test-strategy.md](docs/test-strategy.md) | How the project tests itself |
| [docs/build-plan.md](docs/build-plan.md) | The workspace layout and the phases |
| [docs/toolchain-plan.md](docs/toolchain-plan.md) | The tools around the language |
| [docs/grammar/lark.ebnf](docs/grammar/lark.ebnf) | The grammar, as a delta over ISO C11 |
| [examples/tour.lark](examples/tour.lark) | Every construct in one file |
| [benchmarks/](benchmarks/) | What each collector costs, and what `gc` costs against C |
| [editors/vscode/](editors/vscode/) | The Visual Studio Code extension |

The guide is the place to start. The specification is the authority, and it
answers a question the guide leaves open. Every rule in it carries a number, so
a diagnostic and a decision both point at the same line.

## Contributing

Read [docs/conventions.md](docs/conventions.md) before you write anything.

Turn on the repository hooks once per clone.

```sh
git config core.hooksPath .githooks
```

Run the gate before every push.

```sh
./scripts/check.sh
```

The gate runs the text checks, `cargo fmt`, `clippy` with `-D warnings`, every
Rust test, and the runtime suite against every collector under two sanitizers.
It also checks that every numbered rule in the specification has a test, and it
runs the benchmarks at their small size, which proves that the four collectors
return the same answer for the same work.

## License

MIT. See [LICENSE](LICENSE).
