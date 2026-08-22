//! Wikimedia Commons provider availability stub.

use crate::core::SourceKind;

use super::ProviderEntry;

#[derive(Debug, Clone, Copy, Default)]
pub struct CommonsProvider;

impl CommonsProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub const fn kind(self) -> SourceKind {
        SourceKind::WikimediaCommons
    }

    #[must_use]
    pub const fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Unavailable {
            source: SourceKind::WikimediaCommons,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_identifies_the_commons_source() {
        assert_eq!(CommonsProvider::new().kind(), SourceKind::WikimediaCommons);
        assert_eq!(
            CommonsProvider::new().entry().unavailable_notice(),
            Some(crate::core::ProviderNotice::Unavailable {
                source: SourceKind::WikimediaCommons
            })
        );
    }
}
