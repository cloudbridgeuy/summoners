//! A temporary reader that projects one `Entity` from a `CardSet` into a
//! `CardDef`, so every call site written against `CardDef` keeps working
//! unchanged while it migrates onto the container at its own pace (design
//! decision 6).
//!
//! `find_def` is the only entry point. It never mints an id, never judges a
//! card's shape, and never fails on a card this crate's fixtures do not
//! print a fact for — an absent fact simply produces no matching `CardNode`,
//! the same way an absent `CardNode` already meant "ask `find` and get
//! `None`" before this module existed.

use super::entity::{
    Attack, Entity, EntityId, Life, ManaTypes, Name, Respondable, RetreatCost, Skill, Tags, Trigger,
};
use super::{
    CardDef, CardKind, CardNode, CardSet, Cost, EffectLeaf, Form, Modifier, SpellTiming,
    TriggerEvent,
};

/// The card family a fixture's `Tags` names (design decision 6, problem 1):
/// `"spell"` and `"enchantment"` are the two tags this crate's fixtures
/// print; every other entity, tagged or not, is a Summon.
fn family(entity: &Entity) -> CardKind {
    match entity.get::<Tags>() {
        Some(tags) if tags.0.iter().any(|tag| tag == "spell") => CardKind::Spell,
        Some(tags) if tags.0.iter().any(|tag| tag == "enchantment") => CardKind::Enchantment,
        _ => CardKind::Summon,
    }
}

/// Project one `Entity`'s components into the `CardDef` tree. A Skill,
/// Attack, or Trigger component wraps its own nested entity, read one level
/// down; a Spell or Enchantment's cost and effects sit directly on the
/// top-level entity instead, since neither wraps a nested entity of its own.
/// Multiple `Skill` components project into multiple `CardNode::Skill`
/// entries in the same authored order `entity.all::<Skill>()` returns them
/// in, so `Query::Skill(SkillIndex)` keeps addressing the same ability it
/// did before this projection existed.
fn project(entity: &Entity) -> CardDef {
    let kind = family(entity);
    let name = entity
        .get::<Name>()
        .map(|name| name.0.clone())
        .unwrap_or_default();

    let mut nodes = Vec::new();

    if let Some(Life(life)) = entity.get::<Life>() {
        nodes.push(CardNode::Life(*life));
    }
    if let Some(types) = entity.get::<ManaTypes>() {
        nodes.push(CardNode::Produces(types.0.clone()));
    }
    if let Some(RetreatCost(cost)) = entity.get::<RetreatCost>() {
        nodes.push(CardNode::RetreatCost(*cost));
    }
    if let Some(form) = entity.get::<Form>() {
        nodes.push(CardNode::Form(*form));
    }
    if let Some(attack) = entity.get::<Attack>() {
        nodes.push(CardNode::Attack {
            cost: attack.get::<Cost>().copied().unwrap_or_default(),
            effects: attack.all::<EffectLeaf>().into_iter().cloned().collect(),
        });
    }
    for skill in entity.all::<Skill>() {
        nodes.push(CardNode::Skill {
            cost: skill.get::<Cost>().copied().unwrap_or_default(),
            effects: skill.all::<EffectLeaf>().into_iter().cloned().collect(),
        });
    }
    for trigger in entity.all::<Trigger>() {
        if let Some(event) = trigger.get::<TriggerEvent>() {
            nodes.push(CardNode::Trigger {
                event: *event,
                respondable: trigger.get::<Respondable>().is_some(),
                effects: trigger.all::<EffectLeaf>().into_iter().cloned().collect(),
            });
        }
    }
    if let Some(modifier) = entity.get::<Modifier>() {
        nodes.push(CardNode::Passive(*modifier));
    }
    match kind {
        CardKind::Spell => {
            if let Some(timing) = entity.get::<SpellTiming>() {
                nodes.push(CardNode::Spell {
                    timing: *timing,
                    cost: entity.get::<Cost>().copied().unwrap_or_default(),
                    effects: entity.all::<EffectLeaf>().into_iter().cloned().collect(),
                });
            }
        }
        CardKind::Enchantment => {
            nodes.push(CardNode::Enchantment {
                cost: entity.get::<Cost>().copied().unwrap_or_default(),
                effects: entity.all::<EffectLeaf>().into_iter().cloned().collect(),
            });
        }
        CardKind::Summon => {}
    }

    CardDef {
        id: entity.id,
        name,
        kind,
        nodes,
    }
}

/// Look up one card by id and project it into a `CardDef`. `None` when
/// `cards` carries no entity under this id — the same shape `CardDef::find`
/// already returns for an absent node, kept at this layer too.
pub(crate) fn find_def(cards: &CardSet, id: EntityId) -> Option<CardDef> {
    cards.get(id).map(project)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::domain::actions::SkillIndex;
    use crate::domain::cards::fixtures;
    use crate::domain::cards::{Query, QueryResult};
    use crate::domain::ids::ManaType;

    fn def(slug: &str) -> CardDef {
        let cards = fixtures::card_set();
        find_def(&cards, fixtures::id(slug)).expect("fixture card is in the set")
    }

    #[test]
    fn find_def_locates_a_fixture_and_rejects_an_unknown_id() {
        let cards = fixtures::card_set();
        assert!(find_def(&cards, fixtures::id("quarry-whelp")).is_some());
        let unknown = EntityId::parse(&"f".repeat(32)).expect("valid probe id");
        assert_eq!(find_def(&cards, unknown), None);
    }

    #[test]
    fn find_reads_the_current_form() {
        assert_eq!(
            def("quarry-whelp").find(Query::CurrentForm),
            Some(QueryResult::CurrentForm(Form::Base))
        );
        assert_eq!(
            def("quarry-brute").find(Query::CurrentForm),
            Some(QueryResult::CurrentForm(Form::Enhanced))
        );
        assert_eq!(
            def("colossus-of-the-quarry").find(Query::CurrentForm),
            Some(QueryResult::CurrentForm(Form::Elite))
        );
    }

    #[test]
    fn find_reads_the_produced_mana_types() {
        assert_eq!(
            def("set-path-adept").find(Query::ProducedManaTypes),
            Some(QueryResult::ProducedManaTypes(vec![
                ManaType::Matter,
                ManaType::Mind
            ]))
        );
    }

    #[test]
    fn find_reads_the_life() {
        assert_eq!(
            def("quarry-whelp").find(Query::Life),
            Some(QueryResult::Life(40))
        );
    }

    #[test]
    fn find_reads_the_retreat_cost() {
        assert_eq!(
            def("colossus-of-the-quarry").find(Query::RetreatCost),
            Some(QueryResult::RetreatCost(3))
        );
    }

    #[test]
    fn find_reads_the_trigger_event_respondability_and_effects() {
        assert_eq!(
            def("spite-thorn").find(Query::Trigger),
            Some(QueryResult::Trigger {
                event: TriggerEvent::AnySummonDestroyed,
                respondable: true,
                effects: vec![EffectLeaf::DealDamage {
                    amount: 15,
                    immutable: false,
                }],
            })
        );
        assert_eq!(
            def("dawn-tender").find(Query::Trigger),
            Some(QueryResult::Trigger {
                event: TriggerEvent::YourUpkeep,
                respondable: false,
                effects: vec![EffectLeaf::Heal { amount: 10 }],
            })
        );
    }

    #[test]
    fn find_reads_one_skill_node_by_index() {
        assert_eq!(
            def("quarry-scout").find(Query::Skill(SkillIndex(0))),
            Some(QueryResult::Skill {
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::MoveSummon],
            })
        );
        assert_eq!(def("quarry-scout").find(Query::Skill(SkillIndex(1))), None);
    }

    #[test]
    fn find_reads_the_spell_timing_cost_and_effects() {
        assert_eq!(
            def("ember-lance").find(Query::Spell),
            Some(QueryResult::Spell {
                timing: SpellTiming::Attack,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            })
        );
    }

    #[test]
    fn find_reads_the_ready_effect_spell() {
        assert_eq!(
            def("second-wind").find(Query::Spell),
            Some(QueryResult::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::ReadySummon],
            })
        );
    }

    #[test]
    fn find_reads_the_enchantment_cost_and_effects() {
        assert_eq!(
            def("standing-ward").find(Query::Enchantment),
            Some(QueryResult::Enchantment {
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![],
            })
        );
    }

    #[test]
    fn find_reads_the_passive_modifier() {
        assert_eq!(
            def("warden-of-set-paths").find(Query::Passive),
            Some(QueryResult::Passive(Modifier::OpposingRetreatCostDelta(1)))
        );
    }

    #[test]
    fn every_summon_chain_climbs_base_enhanced_elite() {
        let chains = [
            ["quarry-whelp", "quarry-brute", "colossus-of-the-quarry"],
            [
                "warden-initiate",
                "warden-pathkeeper",
                "warden-of-set-paths",
            ],
            ["griefsinger-wisp", "griefsinger-mourner", "griefsinger"],
            ["sow-piglet", "sow-matriarch", "old-sow-of-the-barrow"],
        ];
        for [base, enhanced, elite] in chains {
            assert_eq!(
                def(base).find(Query::CurrentForm),
                Some(QueryResult::CurrentForm(Form::Base))
            );
            assert_eq!(
                def(enhanced).find(Query::CurrentForm),
                Some(QueryResult::CurrentForm(Form::Enhanced))
            );
            assert_eq!(
                def(elite).find(Query::CurrentForm),
                Some(QueryResult::CurrentForm(Form::Elite))
            );
        }
    }

    #[test]
    fn find_reads_the_attack_cost_and_effects() {
        assert_eq!(
            def("quarry-brute").find(Query::Attack),
            Some(QueryResult::Attack {
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::DealDamage {
                    amount: 20,
                    immutable: false,
                }],
            })
        );
    }

    #[test]
    fn find_reads_a_card_with_no_produced_mana_types_as_none() {
        // `ember-lance` is a Spell; it prints no `Produces` component at all.
        assert_eq!(def("ember-lance").find(Query::ProducedManaTypes), None);
    }
}
