//! Smithsonian provider availability stub.

use crate::core::SourceKind;

use super::ProviderEntry;

#[derive(Debug, Clone, Copy, Default)]
pub struct SmithsonianProvider;

impl SmithsonianProvider {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    #[must_use]
    pub const fn kind(self) -> SourceKind {
        SourceKind::Smithsonian
    }

    #[must_use]
    pub const fn entry(&self) -> ProviderEntry<'_> {
        ProviderEntry::Unavailable {
            source: SourceKind::Smithsonian,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_identifies_the_smithsonian_source() {
        assert_eq!(SmithsonianProvider::new().kind(), SourceKind::Smithsonian);
        assert_eq!(
            SmithsonianProvider::new().entry().unavailable_notice(),
            Some(crate::core::ProviderNotice::Unavailable {
                source: SourceKind::Smithsonian
            })
        );
    }
}
