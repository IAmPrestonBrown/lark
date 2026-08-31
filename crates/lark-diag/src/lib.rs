//! The Lark diagnostic catalogue, model, and renderer.
//!
//! A user error is a diagnostic, never a `Result`. The compiler collects
//! diagnostics and continues, so one run reports many problems. See rule C-2.3
//! in `docs/conventions.md`.
//!
//! Every diagnostic carries a stable [`Code`]. A test asserts the code, never
//! the message text. See rule P-1 in `docs/test-strategy.md`.
//!
//! The [`CATALOG`] mirrors chapter 12 of the specification. The test
//! in `tests/catalog_matches_spec.rs` checks that the two stay the same.

mod code;
mod diagnostic;
mod render;

pub use code::{CATALOG, Code, CodeInfo, Severity};
pub use diagnostic::{Diagnostic, Diagnostics, Label, Suggestion};
pub use render::{render, render_all};

pub use code::*;
