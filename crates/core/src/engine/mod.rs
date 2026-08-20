//! The public transition and the machinery that drains its consequences.

pub mod apply;
pub(crate) mod board;
pub(crate) mod damage;
pub(crate) mod destruction;
pub(crate) mod effects;
pub mod loss;
pub(crate) mod payment;
pub mod resolution;
pub(crate) mod skills;
pub(crate) mod stack;
pub(crate) mod triggers;
pub(crate) mod turn;
pub mod upkeep;
