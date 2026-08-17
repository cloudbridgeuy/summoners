//! The fixture entities this crate's tests build against, plus the shared
//! `CardSet` and lookups that reach them. Test-only: nothing outside
//! `#[cfg(test)]` may depend on this data's shape.
//!
//! The same vanilla and signature cards `registry.rs` used to hold as
//! `CardDef` literals, now authored as `Entity` values instead: one Base
//! Summon three-step chain and one two-step chain; a Skill, a Trigger, and a
//! Spell fixture for each shape the engine reads; the four signature
//! Base/Enhanced/Elite chains; the one vanilla Enchantment. Fixture stats and
//! names are test data, not final card designs.
//!
//! `card_set` hands every caller the same `Arc<CardSet>` (one process-wide
//! `OnceLock`), so a hand-written `GameState::eq` can compare two
//! independently built states' card sets by handle (`Arc::ptr_eq`) and still
//! find them equal. `id` looks a card up by the same slug its old
//! `CardDefId` used to carry, so `CardDefId("some-slug")` converts
//! mechanically to `fixtures::id("some-slug")` at every call site.

use std::sync::{Arc, OnceLock};

use super::entity::{
    AccountingId, Component, Entity, EntityId, Life, ManaTypes, Name, RetreatCost, Skill, Tags,
    Trigger,
};
use super::{
    CardSet, Cost, EffectCondition, EffectLeaf, Form, Modifier, ResponseBlock, SpellTiming,
    TriggerEvent,
};
use crate::domain::ids::ManaType;

/// A deterministic, non-minting fixture id: `card` identifies the printed
/// card (1–25, matching the old registry's order), `ability` distinguishes
/// its nested Attack (1), Skill (2), and Trigger (3) entities from the card
/// itself (0) and from each other. No two fixture entities ever share a
/// `(card, ability)` pair, so every id this module builds is unique.
fn fid(card: u32, ability: u32) -> EntityId {
    EntityId::parse(&format!("{:032x}", card * 1000 + ability)).expect("valid fixture id")
}

fn attack(card: u32, cost: Cost, effects: Vec<EffectLeaf>) -> Component {
    let mut components = vec![Component::Cost(cost)];
    components.extend(effects.into_iter().map(Component::Effect));
    Component::Attack(Entity {
        id: fid(card, 1),
        components,
    })
}

fn skill(card: u32, cost: Cost, effects: Vec<EffectLeaf>) -> Component {
    let mut components = vec![Component::Cost(cost)];
    components.extend(effects.into_iter().map(Component::Effect));
    Component::Skill(Entity {
        id: fid(card, 2),
        components,
    })
}

fn trigger(
    card: u32,
    event: TriggerEvent,
    respondable: bool,
    effects: Vec<EffectLeaf>,
) -> Component {
    let mut components = vec![Component::Event(event)];
    if respondable {
        components.push(Component::Respondable);
    }
    components.extend(effects.into_iter().map(Component::Effect));
    Component::Trigger(Entity {
        id: fid(card, 3),
        components,
    })
}

/// A Summon fixture's fixed printed facts, grouped into one value so
/// `summon` stays under the argument-count lint (its nested Attack, Skill,
/// and Trigger entities vary in shape too much to join this struct, so they
/// stay `summon`'s own second, separate `abilities` argument).
struct SummonSpec {
    card: u32,
    name: &'static str,
    life: u32,
    produces: Vec<ManaType>,
    retreat: u32,
    form: Form,
}

fn summon(spec: SummonSpec, abilities: Vec<Component>) -> Entity {
    let mut components = vec![
        Component::Name(Name(spec.name.to_string())),
        Component::AccountingId(AccountingId {
            prefix: "CARD".to_string(),
            number: spec.card,
        }),
        Component::Life(Life(spec.life)),
        Component::Produces(ManaTypes(spec.produces)),
        Component::RetreatCost(RetreatCost(spec.retreat)),
        Component::Form(spec.form),
    ];
    components.extend(abilities);
    Entity {
        id: fid(spec.card, 0),
        components,
    }
}

fn spell(
    card: u32,
    name: &str,
    timing: SpellTiming,
    cost: Cost,
    effects: Vec<EffectLeaf>,
) -> Entity {
    let mut components = vec![
        Component::Name(Name(name.to_string())),
        Component::AccountingId(AccountingId {
            prefix: "CARD".to_string(),
            number: card,
        }),
        Component::Tags(Tags(vec!["spell".to_string()])),
        Component::Timing(timing),
        Component::Cost(cost),
    ];
    components.extend(effects.into_iter().map(Component::Effect));
    Entity {
        id: fid(card, 0),
        components,
    }
}

fn enchantment(card: u32, name: &str, cost: Cost, effects: Vec<EffectLeaf>) -> Entity {
    let mut components = vec![
        Component::Name(Name(name.to_string())),
        Component::AccountingId(AccountingId {
            prefix: "CARD".to_string(),
            number: card,
        }),
        Component::Tags(Tags(vec!["enchantment".to_string()])),
        Component::Cost(cost),
        // Rules §44: an Enchantment stays in play after resolving. This is
        // a printed fact, not a consequence of the "enchantment" tag — see
        // `Persistent`'s own doc comment.
        Component::Persistent,
    ];
    components.extend(effects.into_iter().map(Component::Effect));
    Entity {
        id: fid(card, 0),
        components,
    }
}

/// Every fixture entity, in the old registry's order.
pub(crate) fn entities() -> Vec<Entity> {
    vec![
        // Chain 1 — three steps.
        summon(
            SummonSpec {
                card: 1,
                name: "Quarry Whelp",
                life: 40,
                produces: vec![ManaType::Matter],
                retreat: 1,
                form: Form::Base,
            },
            vec![attack(
                1,
                Cost::default(),
                vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 2,
                name: "Quarry Brute",
                life: 90,
                produces: vec![ManaType::Matter],
                retreat: 2,
                form: Form::Enhanced,
            },
            vec![attack(
                2,
                Cost {
                    generic: 1,
                    ..Cost::default()
                },
                vec![EffectLeaf::DealDamage {
                    amount: 20,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 3,
                name: "Colossus of the Quarry",
                life: 180,
                produces: vec![ManaType::Matter],
                retreat: 3,
                form: Form::Elite,
            },
            vec![attack(
                3,
                Cost {
                    matter: 2,
                    generic: 1,
                    ..Cost::default()
                },
                vec![EffectLeaf::DealDamage {
                    amount: 90,
                    immutable: false,
                }],
            )],
        ),
        // Chain 2 — two steps.
        summon(
            SummonSpec {
                card: 4,
                name: "Set-Path Adept",
                life: 50,
                produces: vec![ManaType::Matter, ManaType::Mind],
                retreat: 2,
                form: Form::Base,
            },
            vec![attack(
                4,
                Cost::default(),
                vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 5,
                name: "Set-Path Warden",
                life: 100,
                produces: vec![ManaType::Matter, ManaType::Mind],
                retreat: 2,
                form: Form::Enhanced,
            },
            vec![attack(
                5,
                Cost {
                    generic: 1,
                    ..Cost::default()
                },
                vec![EffectLeaf::DealDamage {
                    amount: 20,
                    immutable: false,
                }],
            )],
        ),
        // Skill fixtures.
        summon(
            SummonSpec {
                card: 6,
                name: "Quarry Scout",
                life: 30,
                produces: vec![ManaType::Matter],
                retreat: 1,
                form: Form::Base,
            },
            vec![
                attack(
                    6,
                    Cost::default(),
                    vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                ),
                skill(
                    6,
                    Cost {
                        generic: 1,
                        ..Cost::default()
                    },
                    vec![EffectLeaf::MoveSummon],
                ),
            ],
        ),
        summon(
            SummonSpec {
                card: 7,
                name: "Quarry Warden-Guard",
                life: 30,
                produces: vec![ManaType::Matter],
                retreat: 1,
                form: Form::Base,
            },
            vec![
                attack(
                    7,
                    Cost::default(),
                    vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                ),
                skill(7, Cost::default(), vec![EffectLeaf::SwapPositions]),
            ],
        ),
        summon(
            SummonSpec {
                card: 8,
                name: "Quarry Well-Tender",
                life: 30,
                produces: vec![ManaType::Matter],
                retreat: 1,
                form: Form::Base,
            },
            vec![
                attack(
                    8,
                    Cost::default(),
                    vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                ),
                skill(8, Cost::default(), vec![EffectLeaf::ProduceMana]),
            ],
        ),
        // Trigger fixtures.
        summon(
            SummonSpec {
                card: 9,
                name: "Hearth Warden",
                life: 60,
                produces: vec![ManaType::Spirit],
                retreat: 1,
                form: Form::Base,
            },
            vec![
                attack(
                    9,
                    Cost::default(),
                    vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                ),
                trigger(
                    9,
                    TriggerEvent::EntersMain,
                    false,
                    vec![EffectLeaf::Heal { amount: 15 }],
                ),
            ],
        ),
        summon(
            SummonSpec {
                card: 10,
                name: "Spite Thorn",
                life: 50,
                produces: vec![ManaType::Mind],
                retreat: 1,
                form: Form::Base,
            },
            vec![
                attack(
                    10,
                    Cost::default(),
                    vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                ),
                trigger(
                    10,
                    TriggerEvent::AnySummonDestroyed,
                    true,
                    vec![EffectLeaf::DealDamage {
                        amount: 15,
                        immutable: false,
                    }],
                ),
            ],
        ),
        summon(
            SummonSpec {
                card: 11,
                name: "Dawn Tender",
                life: 40,
                produces: vec![ManaType::Spirit],
                retreat: 1,
                form: Form::Base,
            },
            vec![
                attack(
                    11,
                    Cost::default(),
                    vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                ),
                trigger(
                    11,
                    TriggerEvent::YourUpkeep,
                    false,
                    vec![EffectLeaf::Heal { amount: 10 }],
                ),
            ],
        ),
        // Spells.
        spell(
            12,
            "Ember Lance",
            SpellTiming::Attack,
            Cost {
                generic: 1,
                ..Cost::default()
            },
            vec![EffectLeaf::DealDamage {
                amount: 10,
                immutable: false,
            }],
        ),
        spell(
            13,
            "Renewing Balm",
            SpellTiming::Support,
            Cost {
                generic: 1,
                ..Cost::default()
            },
            vec![EffectLeaf::Heal { amount: 20 }],
        ),
        spell(
            14,
            "Scrying Glass",
            SpellTiming::Support,
            Cost {
                generic: 1,
                ..Cost::default()
            },
            vec![EffectLeaf::DrawCards { amount: 1 }],
        ),
        spell(
            15,
            "Second Wind",
            SpellTiming::Support,
            Cost {
                generic: 1,
                ..Cost::default()
            },
            vec![EffectLeaf::ReadySummon],
        ),
        // Matter/Mind — the Warden of Set Paths.
        summon(
            SummonSpec {
                card: 16,
                name: "Warden Initiate",
                life: 50,
                produces: vec![ManaType::Matter],
                retreat: 2,
                form: Form::Base,
            },
            vec![attack(
                16,
                Cost::default(),
                vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 17,
                name: "Warden Pathkeeper",
                life: 100,
                produces: vec![ManaType::Matter, ManaType::Mind],
                retreat: 2,
                form: Form::Enhanced,
            },
            vec![attack(
                17,
                Cost {
                    generic: 1,
                    ..Cost::default()
                },
                vec![EffectLeaf::DealDamage {
                    amount: 20,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 18,
                name: "Warden of Set Paths",
                life: 150,
                produces: vec![ManaType::Matter, ManaType::Mind],
                retreat: 3,
                form: Form::Elite,
            },
            vec![
                skill(
                    18,
                    Cost {
                        matter: 1,
                        mind: 1,
                        ..Cost::default()
                    },
                    vec![EffectLeaf::SwapOpposingPositions],
                ),
                Component::Passive(Modifier::OpposingRetreatCostDelta(1)),
                attack(
                    18,
                    Cost {
                        matter: 1,
                        generic: 2,
                        ..Cost::default()
                    },
                    vec![
                        EffectLeaf::DealDamage {
                            amount: 50,
                            immutable: false,
                        },
                        EffectLeaf::ConditionalBonus {
                            condition: EffectCondition::DefenderEnteredMainThisTurn,
                            amount: 30,
                        },
                    ],
                ),
            ],
        ),
        // Mind/Spirit — the Griefsinger.
        summon(
            SummonSpec {
                card: 19,
                name: "Griefsinger Wisp",
                life: 40,
                produces: vec![ManaType::Mind],
                retreat: 1,
                form: Form::Base,
            },
            vec![attack(
                19,
                Cost::default(),
                vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 20,
                name: "Griefsinger Mourner",
                life: 80,
                produces: vec![ManaType::Mind, ManaType::Spirit],
                retreat: 1,
                form: Form::Enhanced,
            },
            vec![attack(
                20,
                Cost {
                    generic: 1,
                    ..Cost::default()
                },
                vec![EffectLeaf::DealDamage {
                    amount: 20,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 21,
                name: "Griefsinger",
                life: 110,
                produces: vec![ManaType::Mind, ManaType::Spirit],
                retreat: 1,
                form: Form::Elite,
            },
            vec![
                trigger(
                    21,
                    TriggerEvent::AnySummonDestroyed,
                    true,
                    vec![EffectLeaf::ReturnSpellFromDiscard],
                ),
                skill(
                    21,
                    Cost {
                        mind: 1,
                        spirit: 1,
                        ..Cost::default()
                    },
                    vec![
                        EffectLeaf::LookAtPrizes,
                        EffectLeaf::DrawCards { amount: 1 },
                        EffectLeaf::ReturnSpellToDeckTop,
                    ],
                ),
                attack(
                    21,
                    Cost {
                        mind: 1,
                        spirit: 1,
                        generic: 1,
                        ..Cost::default()
                    },
                    vec![
                        EffectLeaf::DealDamage {
                            amount: 40,
                            immutable: false,
                        },
                        EffectLeaf::ConditionalBonus {
                            condition: EffectCondition::SpellPlayedThisTurn,
                            amount: 40,
                        },
                        EffectLeaf::BlockResponses {
                            condition: EffectCondition::SpellPlayedThisTurn,
                            block: ResponseBlock::AttackSpells,
                        },
                    ],
                ),
            ],
        ),
        // Matter/Spirit — the Old Sow of the Barrow.
        summon(
            SummonSpec {
                card: 22,
                name: "Sow Piglet",
                life: 50,
                produces: vec![ManaType::Spirit],
                retreat: 2,
                form: Form::Base,
            },
            vec![attack(
                22,
                Cost::default(),
                vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 23,
                name: "Sow Matriarch",
                life: 110,
                produces: vec![ManaType::Matter, ManaType::Spirit],
                retreat: 3,
                form: Form::Enhanced,
            },
            vec![attack(
                23,
                Cost {
                    generic: 1,
                    ..Cost::default()
                },
                vec![EffectLeaf::DealDamage {
                    amount: 20,
                    immutable: false,
                }],
            )],
        ),
        summon(
            SummonSpec {
                card: 24,
                name: "Old Sow of the Barrow",
                life: 170,
                produces: vec![ManaType::Matter, ManaType::Spirit],
                retreat: 4,
                form: Form::Elite,
            },
            vec![
                trigger(
                    24,
                    TriggerEvent::YourUpkeep,
                    false,
                    vec![EffectLeaf::Heal { amount: 10 }],
                ),
                skill(
                    24,
                    Cost {
                        matter: 1,
                        spirit: 1,
                        ..Cost::default()
                    },
                    vec![
                        EffectLeaf::Heal { amount: 30 },
                        EffectLeaf::CannotBeMovedByOpponent,
                    ],
                ),
                attack(
                    24,
                    Cost {
                        matter: 1,
                        spirit: 1,
                        generic: 2,
                        ..Cost::default()
                    },
                    vec![EffectLeaf::DealDamage {
                        amount: 70,
                        immutable: true,
                    }],
                ),
            ],
        ),
        // The vanilla Enchantment.
        enchantment(
            25,
            "Standing Ward",
            Cost {
                generic: 1,
                ..Cost::default()
            },
            vec![],
        ),
    ]
}

/// The one shared `Arc<CardSet>` every test in this crate builds against.
/// One process-wide `OnceLock`, not a `thread_local!`: every test in the
/// binary, on whichever thread the harness schedules it, sees the same
/// handle, so a hand-written `GameState::eq`'s `Arc::ptr_eq` on `cards`
/// finds two independently constructed states equal.
pub(crate) fn card_set() -> Arc<CardSet> {
    static SET: OnceLock<Arc<CardSet>> = OnceLock::new();
    Arc::clone(SET.get_or_init(|| Arc::new(CardSet::new(entities()))))
}

/// The same slug a fixture's old `CardDefId` carried, e.g. `"quarry-whelp"`:
/// its printed name, lowercased, with spaces turned to hyphens. Every
/// fixture name in this module slugifies to the string its old `CardDefId`
/// held, so `CardDefId("some-slug")` converts mechanically to
/// `fixtures::id("some-slug")`.
fn slugify(name: &str) -> String {
    name.to_lowercase().replace(' ', "-")
}

/// The id of the fixture card whose name slugifies to `slug`. Panics if no
/// fixture matches: a test naming a slug that is not in this module is a
/// test-authoring mistake, not a runtime condition production code must
/// handle.
pub(crate) fn id(slug: &str) -> EntityId {
    card_set()
        .entities()
        .iter()
        .find(|entity| entity.get::<Name>().map(|name| slugify(&name.0)).as_deref() == Some(slug))
        .map(|entity| entity.id)
        .unwrap_or_else(|| panic!("no fixture card named {slug}"))
}

/// The id of the first Skill printed on the fixture card whose name
/// slugifies to `slug`. Panics if that card prints no Skill: a test naming
/// one that has none is a test-authoring mistake, not a runtime condition
/// production code must handle.
pub(crate) fn skill_id(slug: &str) -> EntityId {
    card_set()
        .get(id(slug))
        .and_then(|entity| entity.get::<Skill>())
        .map(|skill| skill.id)
        .unwrap_or_else(|| panic!("fixture card {slug} prints no Skill"))
}

/// The id of the first Trigger printed on the fixture card whose name
/// slugifies to `slug`. Panics if that card prints no Trigger: a test
/// naming one that has none is a test-authoring mistake, not a runtime
/// condition production code must handle.
pub(crate) fn trigger_id(slug: &str) -> EntityId {
    card_set()
        .get(id(slug))
        .and_then(|entity| entity.get::<Trigger>())
        .map(|trigger| trigger.id)
        .unwrap_or_else(|| panic!("fixture card {slug} prints no Trigger"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_holds_all_twenty_five_fixture_cards() {
        assert_eq!(entities().len(), 25);
    }

    #[test]
    fn card_set_hands_back_the_same_handle_every_call() {
        let first = card_set();
        let second = card_set();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn every_signature_chain_climbs_base_enhanced_elite() {
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
        let cards = card_set();
        for [base, enhanced, elite] in chains {
            let form_of = |slug: &str| {
                cards
                    .get(id(slug))
                    .and_then(|entity| entity.get::<Form>())
                    .copied()
            };
            assert_eq!(form_of(base), Some(Form::Base));
            assert_eq!(form_of(enhanced), Some(Form::Enhanced));
            assert_eq!(form_of(elite), Some(Form::Elite));
        }
    }

    #[test]
    fn id_finds_a_card_by_its_slug() {
        let cards = card_set();
        let whelp = cards
            .get(id("quarry-whelp"))
            .expect("quarry-whelp is a fixture");
        assert_eq!(whelp.get::<Name>(), Some(&Name("Quarry Whelp".to_string())));
    }

    #[test]
    fn id_finds_every_fixture_by_its_slugified_name() {
        let slugs = [
            "quarry-whelp",
            "quarry-brute",
            "colossus-of-the-quarry",
            "set-path-adept",
            "set-path-warden",
            "quarry-scout",
            "quarry-warden-guard",
            "quarry-well-tender",
            "hearth-warden",
            "spite-thorn",
            "dawn-tender",
            "ember-lance",
            "renewing-balm",
            "scrying-glass",
            "second-wind",
            "warden-initiate",
            "warden-pathkeeper",
            "warden-of-set-paths",
            "griefsinger-wisp",
            "griefsinger-mourner",
            "griefsinger",
            "sow-piglet",
            "sow-matriarch",
            "old-sow-of-the-barrow",
            "standing-ward",
        ];
        let cards = card_set();
        for slug in slugs {
            assert!(
                cards.get(id(slug)).is_some(),
                "slug {slug} should resolve to a fixture card"
            );
        }
    }

    #[test]
    #[should_panic(expected = "no fixture card named nonexistent-card")]
    fn id_panics_on_an_unknown_slug() {
        let _ = id("nonexistent-card");
    }

    #[test]
    fn skill_id_finds_the_first_skill_printed_on_a_fixture_card() {
        let cards = card_set();
        let scout = cards
            .get(id("quarry-scout"))
            .expect("quarry-scout is a fixture");
        let scouts_skill = scout.get::<Skill>().expect("quarry-scout prints a Skill");
        assert_eq!(skill_id("quarry-scout"), scouts_skill.id);
    }

    #[test]
    #[should_panic(expected = "fixture card quarry-whelp prints no Skill")]
    fn skill_id_panics_on_a_card_with_no_skill() {
        let _ = skill_id("quarry-whelp");
    }

    #[test]
    fn trigger_id_finds_the_first_trigger_printed_on_a_fixture_card() {
        let cards = card_set();
        let spite_thorn = cards
            .get(id("spite-thorn"))
            .expect("spite-thorn is a fixture");
        let its_trigger = spite_thorn
            .get::<Trigger>()
            .expect("spite-thorn prints a Trigger");
        assert_eq!(trigger_id("spite-thorn"), its_trigger.id);
    }

    #[test]
    #[should_panic(expected = "fixture card quarry-whelp prints no Trigger")]
    fn trigger_id_panics_on_a_card_with_no_trigger() {
        let _ = trigger_id("quarry-whelp");
    }
}
