//! Shared helpers for integration tests in this directory.
//!
//! Each `tests/*.rs` file is compiled as a separate crate and can only see
//! `mcp_nano_lib`'s public API. To avoid duplicating scenario helpers across
//! files, we re-export everything from `controllers::debug` (the single
//! source of truth, kept inside the lib so it has access to private modules
//! via `pub mod` widening in `lib.rs`).
//!
//! Usage from a test file:
//!
//! ```ignore
//! mod common;
//! use common::{spawn_qdrant, load_embedders, cosine, ...};
//! ```
//!
//! See: <https://doc.rust-lang.org/rust-by-example/testing/integration_testing.html>

pub use mcp_nano_lib::controllers::debug::*;
