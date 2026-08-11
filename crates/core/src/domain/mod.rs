//! Game vocabulary: identities, state, actions, events, errors, and the
//! provisional card tree. Nothing here reads a file, calls a network, or
//! owns a clock — the domain is the pure core.

pub mod actions;
pub mod cards;
pub mod errors;
pub mod events;
pub mod ids;
pub mod state;
