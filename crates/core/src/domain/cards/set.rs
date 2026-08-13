//! `CardSet`: the indexed collection of top-level entities.
//!
//! `CardSet::new` builds two indices once, at construction, and judges
//! nothing about the entities it is handed — the core is permissive about
//! card shape. Both indices cover top-level entities only; a nested ability
//! id stays unindexed, because an action that activates a Skill already
//! names the card carrying it, so the core resolves the ability by scanning
//! that one card's components.
//!
//! A duplicate id or a duplicate accounting code resolves first-wins,
//! deliberately: `HashMap::insert` is last-wins by default, so the indices
//! are built with `entry().or_insert()` instead.

use std::collections::HashMap;

use super::entity::{AccountingId, Entity, EntityId};

/// The authored card pool: every top-level entity, plus the two indices
/// built over it. Holds no interior mutability — once built, a shared
/// `Arc<CardSet>` stays immutable to every holder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSet {
    entities: Vec<Entity>,
    by_id: HashMap<EntityId, usize>,
    by_accounting: HashMap<String, EntityId>,
}

impl CardSet {
    /// Build both indices from the authored entity list. Duplicate ids and
    /// duplicate accounting codes both resolve first-wins; a card without an
    /// `AccountingId` simply has no entry in `by_accounting`.
    pub fn new(entities: Vec<Entity>) -> CardSet {
        let mut by_id = HashMap::new();
        let mut by_accounting = HashMap::new();

        for (index, entity) in entities.iter().enumerate() {
            by_id.entry(entity.id).or_insert(index);
            if let Some(accounting) = entity.get::<AccountingId>() {
                by_accounting.entry(accounting.code()).or_insert(entity.id);
            }
        }

        CardSet {
            entities,
            by_id,
            by_accounting,
        }
    }

    /// The authored entity list, in authored order.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// The top-level entity carrying this id, or `None`. A duplicate id
    /// resolves to the first entity that carried it.
    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.by_id.get(&id).map(|&index| &self.entities[index])
    }

    /// The top-level entity printing this accounting code, e.g. `QRY-014`,
    /// or `None`. A duplicate code resolves to the first card that printed
    /// it.
    pub fn by_accounting(&self, code: &str) -> Option<&Entity> {
        self.by_accounting.get(code).and_then(|id| self.get(*id))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::domain::cards::entity::{
        Breakage, Component, ComponentKind, Life, Name, RetreatCost, Skill,
    };

    fn id(byte: u8) -> EntityId {
        EntityId::parse(&format!("{byte:02x}").repeat(16)).expect("valid fixture id")
    }

    fn card(byte: u8, accounting: Option<(&str, u32)>) -> Entity {
        let mut components = vec![Component::Name(Name(format!("Card {byte}")))];
        if let Some((prefix, number)) = accounting {
            components.push(Component::AccountingId(AccountingId {
                prefix: prefix.to_string(),
                number,
            }));
        }
        Entity {
            id: id(byte),
            components,
        }
    }

    #[test]
    fn new_builds_a_set_from_entity_literals() {
        let set = CardSet::new(vec![card(1, Some(("QRY", 14)))]);
        assert_eq!(set.entities().len(), 1);
    }

    #[test]
    fn by_accounting_finds_the_card_by_its_printed_code() {
        let set = CardSet::new(vec![card(1, Some(("QRY", 14))), card(2, None)]);
        let found = set.by_accounting("QRY-014").expect("QRY-014 is indexed");
        assert_eq!(found.id, id(1));
    }

    #[test]
    fn by_accounting_returns_none_for_an_unprinted_code() {
        let set = CardSet::new(vec![card(1, None)]);
        assert_eq!(set.by_accounting("QRY-999"), None);
    }

    #[test]
    fn get_finds_a_card_by_its_entity_id() {
        let set = CardSet::new(vec![card(1, None), card(2, None)]);
        assert_eq!(set.get(id(2)).map(|entity| entity.id), Some(id(2)));
    }

    #[test]
    fn get_returns_none_for_an_id_not_in_the_set() {
        let set = CardSet::new(vec![card(1, None)]);
        assert_eq!(set.get(id(99)), None);
    }

    #[test]
    fn duplicate_entity_ids_resolve_first_wins() {
        let first = Entity {
            id: id(5),
            components: vec![Component::Life(Life(10))],
        };
        let second = Entity {
            id: id(5),
            components: vec![Component::Life(Life(999))],
        };
        let set = CardSet::new(vec![first, second]);
        let resolved = set.get(id(5)).expect("id 5 is indexed");
        assert_eq!(resolved.get::<Life>(), Some(&Life(10)));
    }

    #[test]
    fn duplicate_accounting_codes_resolve_first_wins() {
        let first = card(1, Some(("QRY", 14)));
        let second = card(2, Some(("QRY", 14)));
        let set = CardSet::new(vec![first, second]);
        let resolved = set.by_accounting("QRY-014").expect("QRY-014 is indexed");
        assert_eq!(resolved.id, id(1));
    }

    /// The slice's acceptance demo, exercised end to end: build a set from
    /// entity literals; find one by its printed accounting code; read the
    /// first of two duplicate `Life` components; read every one of three
    /// `Skill` components in authored order; read nothing from `get` and a
    /// named `Breakage` from `demand` for a `RetreatCost` no entity here
    /// prints; resolve a duplicate id first-wins.
    #[test]
    fn the_demo_builds_a_set_and_proves_every_read() {
        let move_skill = Entity {
            id: id(20),
            components: vec![Component::Name(Name("Move".to_string()))],
        };
        let swap_skill = Entity {
            id: id(21),
            components: vec![Component::Name(Name("Swap".to_string()))],
        };
        let produce_skill = Entity {
            id: id(22),
            components: vec![Component::Name(Name("Produce".to_string()))],
        };

        let whelp = Entity {
            id: id(1),
            components: vec![
                Component::AccountingId(AccountingId {
                    prefix: "QRY".to_string(),
                    number: 14,
                }),
                Component::Life(Life(40)),
                Component::Life(Life(999)), // duplicate; get::<Life>() reads the first
                Component::Skill(move_skill.clone()),
                Component::Skill(swap_skill.clone()),
                Component::Skill(produce_skill.clone()),
                // No RetreatCost is printed here.
            ],
        };

        let duplicate_of_whelp_id = Entity {
            id: id(1), // same id as `whelp`; must lose to it in `by_id`
            components: vec![Component::Name(Name("Impostor".to_string()))],
        };

        let set = CardSet::new(vec![whelp, duplicate_of_whelp_id]);

        let found = set
            .by_accounting("QRY-014")
            .expect("QRY-014 is indexed to the first Whelp entity");
        assert_eq!(found.get::<Life>(), Some(&Life(40)));
        assert_eq!(
            found.all::<Skill>(),
            vec![&move_skill, &swap_skill, &produce_skill]
        );
        assert_eq!(found.get::<RetreatCost>(), None);
        assert_eq!(
            found.demand::<RetreatCost>("retreat"),
            Err(Breakage {
                rule: "retreat",
                entity: id(1),
                expected: ComponentKind::RetreatCost,
            })
        );

        // Two entities sharing one id: `by_id` resolves to the first.
        let resolved = set.get(id(1)).expect("id 1 is indexed");
        assert_eq!(resolved.get::<Name>(), None); // the Whelp itself has no Name
    }
}
