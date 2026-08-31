//! Source files, byte spans, and line mapping.
//!
//! Every position in the Lark compiler is a byte offset into a source file.
//! A [`Span`] is a half open byte range. A [`SourceMap`] owns the text of
//! every file and turns an offset into a line and a column.
//!
//! This crate holds no compiler logic. It depends on nothing.

mod source;
mod span;

pub use source::{LineCol, SourceFile, SourceId, SourceMap};
pub use span::Span;
