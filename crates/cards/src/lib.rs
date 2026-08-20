//! Strict parsing and conversion for authored Summoners card documents.
#![deny(clippy::unwrap_used, clippy::expect_used)]

mod error;
mod document;
mod identity;
mod v1;

use std::collections::BTreeMap;

pub use error::{
    DocumentKind, LoadPhase, SemanticRule, SetLoadCause, SetLoadError, StableKeyKind,
};
use summoners_core::domain::cards::{CardSet, EntityId};

/// One parsed Set with its converted core definitions and stable lookup data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSet {
    id: EntityId,
    code: String,
    revision: u32,
    name: String,
    cards: CardSet,
    card_ids: BTreeMap<String, EntityId>,
}

impl LoadedSet {
    /// The deterministic identity minted from the Set's stable code.
    #[must_use]
    pub const fn id(&self) -> EntityId {
        self.id
    }

    /// The authored stable Set code.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The authored content revision. It does not contribute to identity.
    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    /// The Set's display name. It does not contribute to identity.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// All converted top-level core card definitions.
    #[must_use]
    pub const fn cards(&self) -> &CardSet {
        &self.cards
    }

    /// Resolve one card's stable code inside this Set.
    #[must_use]
    pub fn card_id(&self, code: &str) -> Option<EntityId> {
        self.card_ids.get(code).copied()
    }
}

/// Parse one complete Set document from caller-held bytes.
pub fn parse_set(bytes: &[u8]) -> Result<LoadedSet, SetLoadError> {
    let decoded = document::decode(bytes)?;
    v1::load(decoded)
}
