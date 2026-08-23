//! The library behind the `summoners` command-line shell.
//!
//! This crate holds the pure argument mapping, exit-code classification,
//! and result formatting the binary renders, plus the thin per-command
//! adapters that open files and call into the game and transcript
//! libraries. It owns no game rule and no transcript rule of its own.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod cli;
pub mod error;
pub mod exit;
pub mod report;
pub mod verify;
