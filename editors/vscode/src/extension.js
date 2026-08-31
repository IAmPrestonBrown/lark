// The Lark extension for Visual Studio Code.
//
// The extension is plain JavaScript, so it needs no build step. A reader opens
// this file and sees what runs. See the note in README.md.
//
// It does three things.
//
//   1. It names `.lark` as a language, so the grammar in `syntaxes` colours it.
//      That works with no compiler installed at all.
//   2. It starts `lark-lsp` and speaks the language server protocol to it.
//      That gives completion, hover, go to definition, and diagnostics.
//   3. It runs `lark fmt` on request, and on save when the setting asks.

const { workspace, window, commands } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { execFile } = require("node:child_process");
const path = require("node:path");

/** The running client, or null when the server is off. */
let client = null;

/** The channel that carries a message a person needs to read. */
let output = null;

/**
 * Returns the setting for one key, with the default from `package.json`.
 *
 * @param {string} key
 * @returns {any}
 */
function setting(key) {
  return workspace.getConfiguration("lark").get(key);
}

/**
 * Returns the first workspace folder, or the empty string.
 *
 * @returns {string}
 */
function workspaceRoot() {
  const folders = workspace.workspaceFolders;
  return folders && folders.length > 0 ? folders[0].uri.fsPath : "";
}

/**
 * Returns the directories that `@import` searches.
 *
 * Rule N-3 searches the directory of the file first, and the server adds that
 * itself. A relative path here is read from the workspace folder, so a setting
 * that a team shares does not name one person's disk.
 *
 * @returns {string[]}
 */
function searchPaths() {
  const root = workspaceRoot();
  const configured = setting("server.searchPaths") || [];
  return configured.map((entry) =>
    path.isAbsolute(entry) || root === "" ? entry : path.join(root, entry)
  );
}

/**
 * Starts the language server.
 *
 * A server that does not start is reported once and then left alone. The
 * grammar still colours the file, so the editor stays usable.
 */
async function startServer() {
  if (setting("server.enable") === false) {
    return;
  }

  const command = setting("server.path") || "lark-lsp";
  const options = {
    command,
    args: searchPaths(),
    transport: TransportKind.stdio,
    options: { cwd: workspaceRoot() || undefined },
  };

  client = new LanguageClient(
    "lark",
    "Lark Language Server",
    { run: options, debug: options },
    {
      documentSelector: [{ scheme: "file", language: "lark" }],
      synchronize: {
        fileEvents: workspace.createFileSystemWatcher("**/lark.toml"),
      },
      outputChannel: output,
    }
  );

  try {
    await client.start();
  } catch (error) {
    client = null;
    window.showWarningMessage(
      `Lark: cannot start \`${command}\`. Syntax highlighting still works. ` +
        "Install the compiler, or set `lark.server.path`."
    );
    output.appendLine(`the server did not start: ${error}`);
  }
}

/** Stops the language server, if one runs. */
async function stopServer() {
  if (client === null) {
    return;
  }
  const running = client;
  client = null;
  await running.stop();
}

/**
 * Formats one document with `lark fmt`.
 *
 * Rule Z-1. The formatter has one style and nothing to configure, so the
 * command takes no options either.
 *
 * @param {import("vscode").TextDocument} document
 * @returns {Promise<void>}
 */
function formatDocument(document) {
  return new Promise((resolve) => {
    if (document.languageId !== "lark" || document.uri.scheme !== "file") {
      resolve();
      return;
    }
    const command = setting("format.path") || "lark";
    execFile(
      command,
      ["fmt", document.uri.fsPath],
      { cwd: workspaceRoot() || undefined },
      (error, _stdout, stderr) => {
        if (error) {
          window.showWarningMessage(
            `Lark: \`${command} fmt\` failed. ${stderr || error.message}`
          );
        }
        resolve();
      }
    );
  });
}

/**
 * Sets the extension up. The editor calls this once.
 *
 * @param {import("vscode").ExtensionContext} context
 */
async function activate(context) {
  output = window.createOutputChannel("Lark");
  context.subscriptions.push(output);

  context.subscriptions.push(
    commands.registerCommand("lark.restartServer", async () => {
      await stopServer();
      await startServer();
      window.showInformationMessage("Lark: the language server restarted.");
    })
  );

  context.subscriptions.push(
    commands.registerCommand("lark.format", async () => {
      const editor = window.activeTextEditor;
      if (editor === undefined) {
        return;
      }
      await editor.document.save();
      await formatDocument(editor.document);
    })
  );

  // The formatter rewrites the file on disk, so the save has to finish first.
  // `onDidSaveTextDocument` runs after it, and the editor then reloads.
  context.subscriptions.push(
    workspace.onDidSaveTextDocument(async (document) => {
      if (setting("format.onSave") === true) {
        await formatDocument(document);
      }
    })
  );

  // A change to the server settings takes effect without a reload.
  context.subscriptions.push(
    workspace.onDidChangeConfiguration(async (event) => {
      if (event.affectsConfiguration("lark.server")) {
        await stopServer();
        await startServer();
      }
    })
  );

  await startServer();
}

/** Tears the extension down. The editor calls this once. */
async function deactivate() {
  await stopServer();
}

module.exports = { activate, deactivate };
