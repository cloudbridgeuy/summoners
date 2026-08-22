//! Provider registry and typed availability notices.

use crate::core::SourceKind;

/// One provider slot in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSlot {
    kind: SourceKind,
}

impl ProviderSlot {
    #[must_use]
    pub const fn kind(self) -> SourceKind {
        self.kind
    }

    #[must_use]
    pub const fn unavailable_notice(self) -> ProviderNotice {
        ProviderNotice::Unavailable { source: self.kind }
    }
}

/// A provider outcome that the page can render without a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderNotice {
    Unavailable { source: SourceKind },
}

/// Fixed slots for all supported collections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRegistry {
    slots: [ProviderSlot; 5],
}

impl ProviderRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [
                ProviderSlot {
                    kind: SourceKind::ArtInstituteChicago,
                },
                ProviderSlot {
                    kind: SourceKind::ClevelandMuseum,
                },
                ProviderSlot {
                    kind: SourceKind::MetropolitanMuseum,
                },
                ProviderSlot {
                    kind: SourceKind::Smithsonian,
                },
                ProviderSlot {
                    kind: SourceKind::WikimediaCommons,
                },
            ],
        }
    }

    #[must_use]
    pub fn slots(&self) -> &[ProviderSlot; 5] {
        &self.slots
    }

    #[must_use]
    pub fn unavailable_for(&self, sources: &[SourceKind]) -> Vec<ProviderNotice> {
        self.slots
            .iter()
            .copied()
            .filter(|slot| sources.contains(&slot.kind()))
            .map(ProviderSlot::unavailable_notice)
            .collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_one_ordered_slot_for_each_source() {
        let registry = ProviderRegistry::new();
        assert_eq!(registry.slots().map(ProviderSlot::kind), SourceKind::ALL);
    }

    #[test]
    fn selected_slots_return_typed_unavailable_notices() {
        let notices = ProviderRegistry::new()
            .unavailable_for(&[SourceKind::ClevelandMuseum, SourceKind::WikimediaCommons]);
        assert_eq!(
            notices,
            vec![
                ProviderNotice::Unavailable {
                    source: SourceKind::ClevelandMuseum
                },
                ProviderNotice::Unavailable {
                    source: SourceKind::WikimediaCommons
                },
            ]
        );
    }
}
