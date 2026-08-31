//! The Lark lexer, the lossless syntax tree, and the parser.
//!
//! The tree keeps every byte of the source. `Parse::text` returns the input,
//! byte for byte, even for a file that does not parse. That is invariant R
//! from `docs/test-strategy.md`, and rule L-13 in the specification.
//!
//! The parser never depends on the name resolver. It asks a
//! [`NameOracle`](oracle::NameOracle) instead, so a language server can parse a
//! file whose names do not resolve.

pub mod kind;
pub mod lexer;
pub mod oracle;
pub mod parser;
pub mod tree;

pub use kind::SyntaxKind;
pub use lexer::{LexError, Lexed, Token, tokenize};
pub use oracle::{Binding, KnownNames, NameOracle, NoNames};
pub use parser::{Parse, SyntaxError, parse};
pub use rowan::NodeOrToken;
pub use tree::{Lark, SyntaxElement, SyntaxNode, SyntaxToken, all_tokens, child_tokens};
