//! The public transition and the machinery that drains its consequences.

pub mod apply;
pub(crate) mod board;
pub(crate) mod destruction;
pub mod loss;
pub(crate) mod payment;
pub mod resolution;
pub(crate) mod stack;
pub(crate) mod turn;
pub mod upkeep;
