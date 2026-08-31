# Lark for Visual Studio Code

Language support for [Lark](https://github.com/preston/lark-lang), a systems
language that translates to C11.

## What it gives you

| Feature | What you need |
|---|---|
| Syntax highlighting | Nothing. It works with no compiler installed. |
| Completion, hover, go to definition | `lark-lsp` on your `PATH`. |
| Errors as you type | `lark-lsp` on your `PATH`. |
| Format on save | `lark` on your `PATH`, and one setting. |

The highlighting and the language server are separate. An editor with no
compiler still colours a file correctly, so the extension is useful before you
install anything.

## Install the compiler

```sh
cargo install --path crates/lark-cli     # the `lark` command
cargo install --path crates/lark-lsp     # the `lark-lsp` server
```

Both go to `~/.cargo/bin`, which is normally on your `PATH` already.

## Settings

| Setting | Default | What it does |
|---|---|---|
| `lark.server.enable` | `true` | Run the language server. Turn it off to keep highlighting alone. |
| `lark.server.path` | `lark-lsp` | The server to run. A bare name is looked up on the `PATH`. |
| `lark.server.searchPaths` | `[]` | Directories that `@import` searches. A relative path is read from the workspace folder. |
| `lark.format.onSave` | `false` | Run `lark fmt` when a file is saved. |
| `lark.format.path` | `lark` | The compiler to run for `lark fmt`. |
| `lark.trace.server` | `off` | Log the traffic between the editor and the server. |

The formatter has one style and nothing to configure. See rule Z-1 in
`docs/spec/15-tools.md`.

## Commands

| Command | What it does |
|---|---|
| `Lark: Restart the language server` | Stops the server and starts it again. |
| `Lark: Format this file` | Saves the file and runs `lark fmt` on it. |

## When the server does not start

The extension reports it once and then leaves it alone. Highlighting keeps
working, so the editor stays usable. Open the `Lark` output channel to read
what happened, and check that `lark-lsp` runs from a terminal.

## Build it yourself

```sh
cd editors/vscode
npm install
npx @vscode/vsce package
code --install-extension lark-lang-0.1.0.vsix
```

The extension is plain JavaScript with no build step, so the file you read in
`src/extension.js` is the file that runs.
