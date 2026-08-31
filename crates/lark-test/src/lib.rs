//! The fixture test harness for the Lark compiler.
//!
//! The harness discovers fixtures on disk, runs a front end over each one, and
//! compares the result against an expected file or against inline annotations.
//! `docs/test-strategy.md` describes every test type.
//!
//! The harness drives [`FrontEnd`]. It runs the real parts of the compiler and
//! stands in for the parts that later phases deliver.
//!
//! # Bless mode
//!
//! Set `LARK_BLESS=1` to rewrite every expected file from the actual output.
//! Review the diff before you commit it.
//!
//! ```text
//! LARK_BLESS=1 cargo test
//! ```

pub mod annotation;
pub mod compiler;
pub mod coverage;
pub mod exec;
pub mod fixture;
pub mod runner;
pub mod snapshot;

pub use compiler::{Compile, Config, FrontEnd, Input, Output, Roots};
pub use fixture::{Fixture, KINDS, Kind};
pub use runner::{repository_root, trials};
pub use snapshot::bless_mode;
