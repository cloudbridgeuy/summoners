//! Metropolitan Museum provider availability stub.

use crate::core::SourceKind;

use super::ProviderEntry;

#[derive(Debug, Clone, Copy, Default)]
pub struct MetProvider;

impl MetProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub const fn kind(self) -> SourceKind {
        SourceKind::MetropolitanMuseum
    }

    #[must_use]
    pub const fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Unavailable {
            source: SourceKind::MetropolitanMuseum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_identifies_the_met_source() {
        assert_eq!(MetProvider::new().kind(), SourceKind::MetropolitanMuseum);
        assert_eq!(
            MetProvider::new().entry().unavailable_notice(),
            Some(crate::core::ProviderNotice::Unavailable {
                source: SourceKind::MetropolitanMuseum
            })
        );
    }
}
