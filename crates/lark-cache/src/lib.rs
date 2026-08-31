//! Content addressed caching for the Lark build.
//!
//! A build recompiles every module every time. Most of that work repeats. The
//! cache stores the result of a step under a key that names every input to it,
//! so a step whose inputs did not change is a file copy rather than a run.
//!
//! # Why the key names the inputs rather than the files
//!
//! A build system that compares timestamps asks whether a file is newer than
//! its output. That answer is wrong whenever a clock moves, a file is
//! restored, or two branches share a directory. A key built from the content
//! of every input is right in all of those cases, and it needs no invalidation
//! step: an entry that no key names is simply never read.
//!
//! Rule Y-1. A wrong cache produces a program that builds and misbehaves,
//! which is the worst failure a build tool has. So the key names every input,
//! and a doubt resolves toward a miss.

mod entry;
mod fingerprint;
mod store;

pub use entry::{Entry, Witness, forget_digests};
pub use fingerprint::{Fingerprint, Key};
pub use store::{Cache, Error};
