use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt,
    sync::Arc,
};

use summoners_core::domain::cards::{CardSet, EntityId};

use crate::LoadedSet;

/// Why valid Sets could not form one unambiguous card library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryError {
    DuplicateQualifiedKey {
        key: String,
        first_set: String,
        duplicate_set: String,
    },
    DuplicateId {
        id: EntityId,
        first_key: String,
        duplicate_key: String,
    },
}

impl fmt::Display for LibraryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "card library assembly failed: {self:?}")
    }
}

impl Error for LibraryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetRecord {
    revision: u32,
}

/// Valid loaded Sets resolved into qualified definitions and one shared core pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLibrary {
    sets: BTreeMap<String, SetRecord>,
    cards: BTreeMap<String, EntityId>,
    core_cards: Arc<CardSet>,
}

impl CardLibrary {
    /// Assemble loaded Sets into one qualified card index.
    pub fn from_sets(sets: impl IntoIterator<Item = LoadedSet>) -> Result<Self, LibraryError> {
        let sets: Vec<_> = sets.into_iter().collect();
        let cards = qualified_cards(&sets)?;
        reject_duplicate_ids(&sets)?;
        let records = sets
            .iter()
            .map(|set| {
                (
                    set.code.clone(),
                    SetRecord {
                        revision: set.revision,
                    },
                )
            })
            .collect();
        let entities = sets
            .into_iter()
            .flat_map(|set| set.cards.entities().to_vec())
            .collect();
        Ok(Self {
            sets: records,
            cards,
            core_cards: Arc::new(CardSet::new(entities)),
        })
    }

    /// Clone the one immutable core pool built during library assembly.
    #[must_use]
    pub fn core_cards(&self) -> Arc<CardSet> {
        Arc::clone(&self.core_cards)
    }

    /// Find the loaded revision for one stable Set code.
    #[must_use]
    pub fn set_revision(&self, set: &str) -> Option<u32> {
        self.sets.get(set).map(|record| record.revision)
    }

    /// Resolve one \`set/card\` reference to its core definition identity.
    #[must_use]
    pub fn card_id(&self, qualified_key: &str) -> Option<EntityId> {
        self.cards.get(qualified_key).copied()
    }
}

fn qualified_cards(sets: &[LoadedSet]) -> Result<BTreeMap<String, EntityId>, LibraryError> {
    let mut cards = BTreeMap::new();
    let mut owners = BTreeMap::new();
    for set in sets {
        for (card, id) in &set.card_ids {
            let key = format!("{}/{card}", set.code);
            if let Some(first_set) = owners.insert(key.clone(), set.code.clone()) {
                return Err(LibraryError::DuplicateQualifiedKey {
                    key,
                    first_set,
                    duplicate_set: set.code.clone(),
                });
            }
            cards.insert(key, *id);
        }
    }
    Ok(cards)
}

fn reject_duplicate_ids(sets: &[LoadedSet]) -> Result<(), LibraryError> {
    let mut ids = HashMap::new();
    for set in sets {
        insert_unique_id(&mut ids, set.id, format!("set:{}", set.code))?;
        for (card, id) in &set.card_ids {
            insert_unique_id(&mut ids, *id, format!("{}/{card}", set.code))?;
        }
        for ((card, ability), id) in &set.ability_ids {
            insert_unique_id(&mut ids, *id, format!("{}/{card}#{ability}", set.code))?;
        }
    }
    Ok(())
}

fn insert_unique_id(
    ids: &mut HashMap<EntityId, String>,
    id: EntityId,
    key: String,
) -> Result<(), LibraryError> {
    if let Some(first_key) = ids.insert(id, key.clone()) {
        return Err(LibraryError::DuplicateId {
            id,
            first_key,
            duplicate_key: key,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::parse_set;

    const FOUNDATIONS: &[u8] = include_bytes!("../data/foundations.toml");

    fn foundations() -> LoadedSet {
        parse_set(FOUNDATIONS).expect("Foundations is valid")
    }

    #[test]
    fn qualified_cards_indexes_each_definition() {
        let set = foundations();
        let expected = set.card_id("ember-lance");
        let cards = qualified_cards(&[set]).expect("keys are unique");
        assert_eq!(cards.get("foundations/ember-lance").copied(), expected);
    }

    #[test]
    fn qualified_cards_rejects_a_duplicate_key() {
        let set = foundations();
        let error = qualified_cards(&[set.clone(), set]).expect_err("the key repeats");
        assert!(matches!(error, LibraryError::DuplicateQualifiedKey { .. }));
    }

    #[test]
    fn duplicate_id_check_accepts_unique_ids() {
        reject_duplicate_ids(&[foundations()]).expect("all ids are unique");
    }

    #[test]
    fn duplicate_id_check_rejects_a_set_id_collision() {
        let first = foundations();
        let mut second = first.clone();
        second.code = "other-foundations".to_string();
        let error = reject_duplicate_ids(&[first, second]).expect_err("the Set id repeats");
        assert!(matches!(error, LibraryError::DuplicateId { .. }));
    }

    #[test]
    fn from_sets_rejects_a_card_id_collision() {
        let first = foundations();
        let mut second = first.clone();
        second.code = "other-foundations".to_string();
        second.id =
            EntityId::parse("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee").expect("test Set id is valid");

        let error =
            CardLibrary::from_sets([first, second]).expect_err("a card definition id repeats");
        let LibraryError::DuplicateId {
            first_key,
            duplicate_key,
            ..
        } = error
        else {
            panic!("expected duplicate id");
        };
        assert!(first_key.contains('/'));
        assert!(!first_key.contains('#'));
        assert!(duplicate_key.starts_with("other-foundations/"));
        assert!(!duplicate_key.contains('#'));
    }

    #[test]
    fn from_sets_rejects_a_retained_ability_id_collision() {
        let first = foundations();
        let mut second = first.clone();
        second.code = "other-foundations".to_string();
        second.id =
            EntityId::parse("ffffffffffffffffffffffffffffffff").expect("test Set id is valid");
        second.cards = CardSet::new(vec![]);
        second.card_ids.clear();

        let error = CardLibrary::from_sets([first, second]).expect_err("an ability id repeats");
        let LibraryError::DuplicateId {
            first_key,
            duplicate_key,
            ..
        } = error
        else {
            panic!("expected duplicate id");
        };
        assert!(first_key.contains('#'));
        assert!(duplicate_key.contains('#'));
    }

    #[test]
    fn insert_unique_id_reports_both_keys() {
        let set = foundations();
        let mut ids = HashMap::new();
        insert_unique_id(&mut ids, set.id, "first".to_string()).expect("first insert");
        let error = insert_unique_id(&mut ids, set.id, "second".to_string())
            .expect_err("second insert must fail");
        assert_eq!(
            error,
            LibraryError::DuplicateId {
                id: set.id,
                first_key: "first".to_string(),
                duplicate_key: "second".to_string(),
            }
        );
    }
}
