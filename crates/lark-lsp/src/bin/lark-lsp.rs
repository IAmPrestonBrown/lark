//! The `lark-lsp` binary.
//!
//! An editor starts it and speaks the language server protocol over standard
//! input and output.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    // Every argument is a directory that `@import` searches. See rule N-3.
    let search: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    match lark_lsp::server::run(search) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("the language server stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
