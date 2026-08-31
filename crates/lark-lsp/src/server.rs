//! The protocol side of the language server.
//!
//! The server speaks JSON-RPC over standard input and output, and asks
//! [`Analysis`] for every answer. It uses raw JSON rather than
//! a types crate, so the protocol surface stays small and stable.

use std::collections::BTreeMap;
use std::path::PathBuf;

use lsp_server::{Connection, ExtractError, Message, Request, RequestId, Response};
use serde_json::{Value, json};

use crate::{Analysis, CompletionKind, position};

/// The text of every file that the editor opened.
#[derive(Default)]
struct Documents {
    files: BTreeMap<String, String>,
    search: Vec<PathBuf>,
}

impl Documents {
    /// Returns the analysis for one file.
    fn analyze(&self, uri: &str) -> Option<(Analysis, String)> {
        let text = self.files.get(uri)?.clone();
        let path = path_of(uri);
        let name = path.file_stem().map_or_else(
            || "main".to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        Some((Analysis::new(&name, &path, &text, &self.search), text))
    }
}

/// Runs the server until the editor asks it to stop.
///
/// # Errors
///
/// Returns an error when the connection fails.
pub fn run(search: Vec<PathBuf>) -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let (connection, threads) = Connection::stdio();
    let capabilities = json!({
        "textDocumentSync": 1,
        "completionProvider": { "triggerCharacters": [".", ":", ">"] },
        "hoverProvider": true,
        "definitionProvider": true,
    });
    let _ = connection.initialize(capabilities)?;

    let mut documents = Documents {
        search,
        ..Documents::default()
    };

    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                let response = answer(&documents, &request);
                connection.sender.send(Message::Response(response))?;
            }
            Message::Notification(note) => {
                let published = record(&mut documents, &note.method, &note.params);
                if let Some((uri, diagnostics)) = published {
                    let params = json!({ "uri": uri, "diagnostics": diagnostics });
                    connection.sender.send(Message::Notification(
                        lsp_server::Notification::new(
                            "textDocument/publishDiagnostics".to_owned(),
                            params,
                        ),
                    ))?;
                }
            }
            Message::Response(_) => {}
        }
    }

    threads.join()?;
    Ok(())
}

/// Records an open or a change, and returns the diagnostics to publish.
fn record(documents: &mut Documents, method: &str, params: &Value) -> Option<(String, Value)> {
    let uri = params.get("textDocument")?.get("uri")?.as_str()?.to_owned();
    match method {
        "textDocument/didOpen" => {
            let text = params
                .get("textDocument")?
                .get("text")?
                .as_str()?
                .to_owned();
            documents.files.insert(uri.clone(), text);
        }
        "textDocument/didChange" => {
            let changes = params.get("contentChanges")?.as_array()?;
            let text = changes.last()?.get("text")?.as_str()?.to_owned();
            documents.files.insert(uri.clone(), text);
        }
        "textDocument/didClose" => {
            documents.files.remove(&uri);
            return Some((uri, json!([])));
        }
        _ => return None,
    }
    let (analysis, text) = documents.analyze(&uri)?;
    Some((uri, diagnostics_json(&analysis, &text)))
}

/// Answers one request.
fn answer(documents: &Documents, request: &Request) -> Response {
    let id = request.id.clone();
    let method = request.method.clone();
    let Some((uri, offset)) = target(request) else {
        return Response::new_ok(id, Value::Null);
    };
    let Some((analysis, text)) = documents.analyze(&uri) else {
        return Response::new_ok(id, Value::Null);
    };

    match method.as_str() {
        "textDocument/completion" => {
            let items: Vec<Value> = analysis
                .completions(offset)
                .into_iter()
                .map(|item| {
                    json!({
                        "label": item.label,
                        "kind": completion_kind(item.kind),
                        "detail": item.detail,
                    })
                })
                .collect();
            Response::new_ok(id, json!(items))
        }
        "textDocument/hover" => match analysis.hover(offset) {
            Some(item) => Response::new_ok(
                id,
                json!({
                    "contents": {
                        "kind": "markdown",
                        "value": format!("`{} {}`\n\n{}", item.kind.word(), item.label, item.detail),
                    }
                }),
            ),
            None => Response::new_ok(id, Value::Null),
        },
        "textDocument/definition" => match analysis.definition(offset) {
            Some(item) => {
                let target_text = std::fs::read_to_string(&item.path).unwrap_or_default();
                let (start_line, start_character) =
                    position::to_position(&target_text, item.span.start);
                let (end_line, end_character) = position::to_position(&target_text, item.span.end);
                Response::new_ok(
                    id,
                    json!({
                        "uri": uri_of(&item.path),
                        "range": {
                            "start": { "line": start_line, "character": start_character },
                            "end": { "line": end_line, "character": end_character },
                        }
                    }),
                )
            }
            None => Response::new_ok(id, Value::Null),
        },
        _ => {
            let _ = text;
            Response::new_ok(id, Value::Null)
        }
    }
}

/// Returns the file and the byte offset that a request points at.
fn target(request: &Request) -> Option<(String, u32)> {
    let uri = request
        .params
        .get("textDocument")?
        .get("uri")?
        .as_str()?
        .to_owned();
    let position = request.params.get("position")?;
    let line = u32::try_from(position.get("line")?.as_u64()?).ok()?;
    let character = u32::try_from(position.get("character")?.as_u64()?).ok()?;
    let text = std::fs::read_to_string(path_of(&uri)).unwrap_or_default();
    Some((uri, position::to_offset(&text, line, character)))
}

/// Returns the diagnostics of a file, in the shape the protocol wants.
fn diagnostics_json(analysis: &Analysis, text: &str) -> Value {
    let items: Vec<Value> = analysis
        .diagnostics()
        .items()
        .iter()
        .map(|item| {
            let (start_line, start_character) =
                position::to_position(text, item.primary.span.start);
            let (end_line, end_character) = position::to_position(text, item.primary.span.end);
            json!({
                "range": {
                    "start": { "line": start_line, "character": start_character },
                    "end": { "line": end_line, "character": end_character },
                },
                "severity": if item.severity.is_fatal() { 1 } else { 2 },
                "code": item.code.to_string(),
                "source": "lark",
                "message": item.message,
            })
        })
        .collect();
    json!(items)
}

/// Returns the protocol number for a completion kind.
fn completion_kind(kind: CompletionKind) -> u32 {
    match kind {
        CompletionKind::Module => 9,
        CompletionKind::Type => 22,
        CompletionKind::Interface => 8,
        CompletionKind::Function => 3,
        CompletionKind::Global | CompletionKind::Local => 6,
        CompletionKind::Field => 5,
        CompletionKind::Method => 2,
        CompletionKind::Keyword => 14,
    }
}

/// Turns a file URI into a path.
fn path_of(uri: &str) -> PathBuf {
    PathBuf::from(uri.strip_prefix("file://").unwrap_or(uri))
}

/// Turns a path into a file URI.
fn uri_of(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}

/// The unused import keeps the protocol types visible to this module.
const _: fn(RequestId) = |_| {};
const _: fn(ExtractError<Request>) = |_| {};
