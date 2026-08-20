use std::{
    error::Error,
    fmt,
    sync::{Arc, OnceLock},
};

use crate::{CardLibrary, Deck, DeckLoadError, LibraryError, SetLoadError, parse_deck, parse_set};

const FOUNDATIONS: &[u8] = include_bytes!("../data/foundations.toml");
const SET_PATHS: &[u8] = include_bytes!("../data/set-paths.toml");
const BARROW_HERD: &[u8] = include_bytes!("../data/barrow-herd.toml");

static CATALOG: OnceLock<Result<Arc<BuiltInCatalog>, BuiltInError>> = OnceLock::new();

/// Which embedded runtime step failed while the built-in catalog was loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltInError {
    Foundations(SetLoadError),
    Library(LibraryError),
    SetPaths(DeckLoadError),
    BarrowHerd(DeckLoadError),
}

impl fmt::Display for BuiltInError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "built-in catalog load failed: {self:?}")
    }
}

impl Error for BuiltInError {}

/// The embedded Foundations library and its two resolved test Decks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltInCatalog {
    library: CardLibrary,
    set_paths: Deck,
    barrow_herd: Deck,
}

impl BuiltInCatalog {
    /// The one library shared by all built-in Decks and game states.
    #[must_use]
    pub const fn library(&self) -> &CardLibrary {
        &self.library
    }

    /// The Matter/Mind Set Paths Deck.
    #[must_use]
    pub const fn set_paths(&self) -> &Deck {
        &self.set_paths
    }

    /// The Matter/Spirit Barrow Herd Deck.
    #[must_use]
    pub const fn barrow_herd(&self) -> &Deck {
        &self.barrow_herd
    }
}

/// Load the three embedded documents through the public parsers once.
pub fn built_in_catalog() -> Result<Arc<BuiltInCatalog>, BuiltInError> {
    CATALOG.get_or_init(load_catalog).clone()
}

fn load_catalog() -> Result<Arc<BuiltInCatalog>, BuiltInError> {
    let foundations = parse_set(FOUNDATIONS).map_err(BuiltInError::Foundations)?;
    let library = CardLibrary::from_sets([foundations]).map_err(BuiltInError::Library)?;
    let set_paths = parse_deck(SET_PATHS, &library).map_err(BuiltInError::SetPaths)?;
    let barrow_herd = parse_deck(BARROW_HERD, &library).map_err(BuiltInError::BarrowHerd)?;
    Ok(Arc::new(BuiltInCatalog {
        library,
        set_paths,
        barrow_herd,
    }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn loader_uses_all_three_embedded_documents() {
        let catalog = load_catalog().expect("the embedded catalog is valid");
        assert_eq!(catalog.library.core_cards().entities().len(), 20);
        assert_eq!(catalog.set_paths.body().len(), 20);
        assert_eq!(catalog.barrow_herd.body().len(), 20);
    }
}
