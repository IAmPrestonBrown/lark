//! Dependency resolution and fetching for Lark packages.
//!
//! Everything resolves through git. There is no upload step and no server.
//!
//! A project names a dependency in one of three ways, and rule K-2 allows all
//! three at once.
//!
//! | Form | Where the versions come from |
//! |---|---|
//! | `json = "1.2.0"` | An index. Rule K-3 pins each version to a commit. |
//! | `zlib = { git = "...", tag = "v2" }` | The repository itself. |
//! | `local = { path = "../x" }` | The directory. Nothing is fetched. |
//!
//! The index is the source of truth for what versions exist. Rule K-3 makes an
//! index entry pin a full commit hash, which is what makes an index worth
//! having: a direct dependency trusts whoever controls the tag, and an index
//! dependency trusts a hash, which cannot change under it.
//!
//! `lark.lock` records what a build used. A build with a lock file fetches by
//! commit and reads no index, so it is reproducible. Rule F-2 asks for the same
//! property of the build settings.

pub mod index;
pub mod lock;
pub mod manifest;
pub mod resolve;
pub mod store;
pub mod sync;
