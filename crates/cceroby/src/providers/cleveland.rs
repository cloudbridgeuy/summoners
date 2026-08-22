//! Cleveland Museum provider availability stub.

use crate::core::SourceKind;

use super::ProviderEntry;

#[derive(Debug, Clone, Copy, Default)]
pub struct ClevelandProvider;

impl ClevelandProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub const fn kind(self) -> SourceKind {
        SourceKind::ClevelandMuseum
    }

    #[must_use]
    pub const fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Unavailable {
            source: SourceKind::ClevelandMuseum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_identifies_the_cleveland_source() {
        assert_eq!(ClevelandProvider::new().kind(), SourceKind::ClevelandMuseum);
        assert_eq!(
            ClevelandProvider::new().entry().unavailable_notice(),
            Some(crate::core::ProviderNotice::Unavailable {
                source: SourceKind::ClevelandMuseum
            })
        );
    }
}
