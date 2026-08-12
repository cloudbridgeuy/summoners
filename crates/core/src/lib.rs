//! Deterministic rules and state transitions for Summoners.
//!
//! This crate is the functional core (`~/.claude/patterns/functional-core-imperative-shell.md`):
//! it reads no file, calls no network, owns no clock, and holds no random
//! source. `domain` names the game's vocabulary — identities, state,
//! actions, events, errors, and the provisional card tree. `scenario` parses
//! any representable match situation into a `GameState`. `engine` holds the
//! public transition, `apply`, and the machinery that drains its
//! consequences.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod domain;
pub mod engine;
pub mod scenario;
