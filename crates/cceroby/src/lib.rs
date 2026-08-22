//! Local museum image search.
//!
//! Domain types and HTML rendering form the functional core. The command-line,
//! process, network, browser, and shared-state modules form the imperative shell.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod app;
pub mod core;
pub mod providers;
pub mod render;
pub mod search;
pub mod server;
