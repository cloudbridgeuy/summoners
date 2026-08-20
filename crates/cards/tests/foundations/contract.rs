use super::*;
use summoners_core::domain::cards::{DamageAddition, DamageConstraints, Entity, ResponseBlock};

struct ExpectedCard {
    code: &'static str,
    name: &'static str,
    family_tag: Option<&'static str>,
}

struct ExpectedSkill {
    card: &'static str,
    ability: &'static str,
    cost: Cost,
    effects: Vec<EffectLeaf>,
}

struct ExpectedTrigger {
    card: &'static str,
    ability: &'static str,
    event: TriggerEvent,
    respondable: bool,
    effects: Vec<EffectLeaf>,
}

fn damage(base: u32) -> EffectLeaf {
    EffectLeaf::DealDamage(summoners_core::domain::cards::DamageEffect {
        base,
        constraints: DamageConstraints::new(),
        additions: Vec::new(),
    })
}

fn damage_with_addition(base: u32, amount: u32, condition: EffectCondition) -> EffectLeaf {
    EffectLeaf::DealDamage(summoners_core::domain::cards::DamageEffect {
        base,
        constraints: DamageConstraints::new(),
        additions: vec![DamageAddition { amount, condition }],
    })
}

fn constrained_damage(base: u32) -> EffectLeaf {
    EffectLeaf::DealDamage(summoners_core::domain::cards::DamageEffect {
        base,
        constraints: DamageConstraints::from([
            DamageConstraint::Unpreventable,
            DamageConstraint::Unincreasable,
        ]),
        additions: Vec::new(),
    })
}

fn ability<'a>(
    loaded: &'a summoners_cards::LoadedSet,
    card_code: &str,
    ability_code: &str,
) -> &'a Entity {
    let ability_id = loaded
        .ability_id(card_code, ability_code)
        .expect("approved ability code exists");
    let card = card(loaded, card_code);
    card.all::<Attack>()
        .into_iter()
        .chain(card.all::<Skill>())
        .chain(card.all::<Trigger>())
        .find(|ability| ability.id == ability_id)
        .expect("approved nested ability exists")
}

#[test]
fn all_twenty_cards_keep_their_names_and_family_tags() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    let expected = [
        ExpectedCard {
            code: "warden-initiate",
            name: "Warden Initiate",
            family_tag: None,
        },
        ExpectedCard {
            code: "warden-pathkeeper",
            name: "Warden Pathkeeper",
            family_tag: None,
        },
        ExpectedCard {
            code: "warden-of-set-paths",
            name: "Warden of Set Paths",
            family_tag: None,
        },
        ExpectedCard {
            code: "quarry-scout",
            name: "Quarry Scout",
            family_tag: None,
        },
        ExpectedCard {
            code: "quarry-warden-guard",
            name: "Quarry Warden-Guard",
            family_tag: None,
        },
        ExpectedCard {
            code: "quarry-well-tender",
            name: "Quarry Well-Tender",
            family_tag: None,
        },
        ExpectedCard {
            code: "set-path-adept",
            name: "Set-Path Adept",
            family_tag: None,
        },
        ExpectedCard {
            code: "ember-lance",
            name: "Ember Lance",
            family_tag: Some("spell"),
        },
        ExpectedCard {
            code: "scrying-glass",
            name: "Scrying Glass",
            family_tag: Some("spell"),
        },
        ExpectedCard {
            code: "second-wind",
            name: "Second Wind",
            family_tag: Some("spell"),
        },
        ExpectedCard {
            code: "standing-ward",
            name: "Standing Ward",
            family_tag: Some("enchantment"),
        },
        ExpectedCard {
            code: "sow-piglet",
            name: "Sow Piglet",
            family_tag: None,
        },
        ExpectedCard {
            code: "sow-matriarch",
            name: "Sow Matriarch",
            family_tag: None,
        },
        ExpectedCard {
            code: "old-sow-of-the-barrow",
            name: "Old Sow of the Barrow",
            family_tag: None,
        },
        ExpectedCard {
            code: "hearth-warden",
            name: "Hearth Warden",
            family_tag: None,
        },
        ExpectedCard {
            code: "dawn-tender",
            name: "Dawn Tender",
            family_tag: None,
        },
        ExpectedCard {
            code: "barrow-grazer",
            name: "Barrow Grazer",
            family_tag: None,
        },
        ExpectedCard {
            code: "ash-shepherd",
            name: "Ash Shepherd",
            family_tag: None,
        },
        ExpectedCard {
            code: "barrow-seer",
            name: "Barrow Seer",
            family_tag: None,
        },
        ExpectedCard {
            code: "renewing-balm",
            name: "Renewing Balm",
            family_tag: Some("spell"),
        },
    ];

    for expected in expected {
        let entity = card(&loaded, expected.code);
        assert_eq!(entity.get::<Name>(), Some(&Name(expected.name.to_string())));
        let expected_tags = expected.family_tag.map(|tag| Tags(vec![tag.to_string()]));
        assert_eq!(
            entity.get::<Tags>(),
            expected_tags.as_ref(),
            "{} family tag",
            expected.code
        );
        if expected.family_tag.is_none() {
            assert!(
                entity.get::<Form>().is_some(),
                "{} is a Summon",
                expected.code
            );
            assert_eq!(entity.get::<Cost>(), None, "Summon cards have no card cost");
        }
    }
}

#[test]
fn every_attack_keeps_its_cost_and_ordered_effects() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    let expected = [
        (
            "warden-initiate",
            "steady-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "warden-pathkeeper",
            "path-strike",
            generic_cost(1),
            vec![damage(20)],
        ),
        (
            "warden-of-set-paths",
            "closed-path-strike",
            Cost {
                matter: 1,
                generic: 2,
                ..Cost::default()
            },
            vec![damage_with_addition(
                50,
                30,
                EffectCondition::DefenderEnteredMainThisTurn,
            )],
        ),
        (
            "quarry-scout",
            "quarry-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "quarry-warden-guard",
            "guard-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "quarry-well-tender",
            "tender-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "set-path-adept",
            "spell-fed-strike",
            Cost {
                mind: 1,
                ..Cost::default()
            },
            vec![
                damage_with_addition(20, 20, EffectCondition::SpellPlayedThisTurn),
                EffectLeaf::BlockResponses {
                    condition: EffectCondition::SpellPlayedThisTurn,
                    block: ResponseBlock::AttackSpells,
                },
            ],
        ),
        (
            "sow-piglet",
            "piglet-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "sow-matriarch",
            "matriarch-strike",
            generic_cost(1),
            vec![damage(20)],
        ),
        (
            "old-sow-of-the-barrow",
            "ancient-charge",
            Cost {
                matter: 1,
                spirit: 1,
                generic: 2,
                ..Cost::default()
            },
            vec![constrained_damage(70)],
        ),
        (
            "hearth-warden",
            "hearth-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "dawn-tender",
            "dawn-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "barrow-grazer",
            "grazer-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "ash-shepherd",
            "shepherd-strike",
            Cost::default(),
            vec![damage(10)],
        ),
        (
            "barrow-seer",
            "seer-strike",
            Cost::default(),
            vec![damage(10)],
        ),
    ];

    for (card_code, ability_code, cost, effects) in expected {
        let attack = ability(&loaded, card_code, ability_code);
        assert_eq!(
            attack.get::<Cost>(),
            Some(&cost),
            "{card_code}.{ability_code} cost"
        );
        assert_eq!(
            attack.all::<EffectLeaf>(),
            effects.iter().collect::<Vec<_>>(),
            "{card_code}.{ability_code} effects",
        );
    }
}

#[test]
fn every_skill_keeps_its_cost_and_ordered_effects() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    let expected = [
        ExpectedSkill {
            card: "warden-of-set-paths",
            ability: "rearrange",
            cost: Cost {
                matter: 1,
                mind: 1,
                ..Cost::default()
            },
            effects: vec![EffectLeaf::SwapOpposingPositions],
        },
        ExpectedSkill {
            card: "quarry-scout",
            ability: "scout-the-bench",
            cost: generic_cost(1),
            effects: vec![EffectLeaf::MoveSummon],
        },
        ExpectedSkill {
            card: "quarry-warden-guard",
            ability: "guard-exchange",
            cost: Cost::default(),
            effects: vec![EffectLeaf::SwapPositions],
        },
        ExpectedSkill {
            card: "quarry-well-tender",
            ability: "tend-the-well",
            cost: Cost::default(),
            effects: vec![EffectLeaf::ProduceMana {
                target: EffectTarget::Selected,
            }],
        },
        ExpectedSkill {
            card: "old-sow-of-the-barrow",
            ability: "barrow-bulwark",
            cost: Cost {
                matter: 1,
                spirit: 1,
                ..Cost::default()
            },
            effects: vec![
                EffectLeaf::Heal {
                    amount: 30,
                    target: EffectTarget::Source,
                },
                EffectLeaf::CannotBeMovedByOpponent {
                    target: EffectTarget::Source,
                },
            ],
        },
        ExpectedSkill {
            card: "barrow-seer",
            ability: "barrow-vision",
            cost: Cost {
                spirit: 1,
                generic: 1,
                ..Cost::default()
            },
            effects: vec![
                EffectLeaf::LookAtPrizes,
                EffectLeaf::DrawCards { amount: 1 },
                EffectLeaf::ReturnSpellToDeckTop,
            ],
        },
    ];

    for expected in expected {
        let skill = ability(&loaded, expected.card, expected.ability);
        assert_eq!(skill.get::<Cost>(), Some(&expected.cost));
        assert_eq!(
            skill.all::<EffectLeaf>(),
            expected.effects.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn every_trigger_keeps_event_response_cost_absence_and_ordered_effects() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    let expected = [
        ExpectedTrigger {
            card: "quarry-warden-guard",
            ability: "leave-main-mend",
            event: TriggerEvent::LeavesMain,
            respondable: false,
            effects: vec![EffectLeaf::Heal {
                amount: 10,
                target: EffectTarget::Source,
            }],
        },
        ExpectedTrigger {
            card: "quarry-well-tender",
            ability: "leave-bench-produce",
            event: TriggerEvent::LeavesBench,
            respondable: false,
            effects: vec![EffectLeaf::ProduceMana {
                target: EffectTarget::Source,
            }],
        },
        ExpectedTrigger {
            card: "old-sow-of-the-barrow",
            ability: "upkeep-mend",
            event: TriggerEvent::YourUpkeep,
            respondable: false,
            effects: vec![EffectLeaf::Heal {
                amount: 10,
                target: EffectTarget::Source,
            }],
        },
        ExpectedTrigger {
            card: "hearth-warden",
            ability: "enter-main-mend",
            event: TriggerEvent::EntersMain,
            respondable: false,
            effects: vec![EffectLeaf::Heal {
                amount: 15,
                target: EffectTarget::Source,
            }],
        },
        ExpectedTrigger {
            card: "dawn-tender",
            ability: "dawn-mend",
            event: TriggerEvent::YourUpkeep,
            respondable: false,
            effects: vec![EffectLeaf::Heal {
                amount: 10,
                target: EffectTarget::Source,
            }],
        },
        ExpectedTrigger {
            card: "barrow-grazer",
            ability: "enter-bench-mend",
            event: TriggerEvent::EntersBench,
            respondable: false,
            effects: vec![EffectLeaf::Heal {
                amount: 10,
                target: EffectTarget::Source,
            }],
        },
        ExpectedTrigger {
            card: "ash-shepherd",
            ability: "gather-from-ashes",
            event: TriggerEvent::AnySummonDestroyed,
            respondable: true,
            effects: vec![EffectLeaf::ReturnSpellFromDiscard],
        },
    ];

    for expected in expected {
        let trigger = ability(&loaded, expected.card, expected.ability);
        assert_eq!(trigger.get::<Cost>(), None);
        assert_eq!(trigger.get::<TriggerEvent>(), Some(&expected.event));
        assert_eq!(trigger.get::<Respondable>().is_some(), expected.respondable);
        assert_eq!(
            trigger.all::<EffectLeaf>(),
            expected.effects.iter().collect::<Vec<_>>()
        );
    }
}

#[test]
fn every_spell_and_enchantment_keeps_cost_timing_persistence_and_effect_order() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    let expected = [
        (
            "ember-lance",
            SpellTiming::Attack,
            false,
            vec![damage(10)],
            None,
        ),
        (
            "scrying-glass",
            SpellTiming::Support,
            false,
            vec![EffectLeaf::DrawCards { amount: 1 }],
            None,
        ),
        (
            "second-wind",
            SpellTiming::Support,
            false,
            vec![EffectLeaf::ReadySummon],
            None,
        ),
        (
            "standing-ward",
            SpellTiming::Support,
            true,
            vec![EffectLeaf::Heal {
                amount: 10,
                target: EffectTarget::Selected,
            }],
            Some(Modifier::IncomingAttackDamageReduction(10)),
        ),
        (
            "renewing-balm",
            SpellTiming::Support,
            false,
            vec![EffectLeaf::Heal {
                amount: 20,
                target: EffectTarget::Selected,
            }],
            None,
        ),
    ];

    for (code, timing, persistent, effects, modifier) in expected {
        let entity = card(&loaded, code);
        assert_eq!(entity.get::<Cost>(), Some(&generic_cost(1)), "{code} cost");
        assert_eq!(entity.get::<SpellTiming>(), Some(&timing), "{code} timing");
        assert_eq!(entity.get::<Persistent>().is_some(), persistent);
        assert_eq!(
            entity.all::<EffectLeaf>(),
            effects.iter().collect::<Vec<_>>()
        );
        assert_eq!(entity.get::<Modifier>().copied(), modifier);
    }
    assert_eq!(
        card(&loaded, "warden-of-set-paths").get::<Modifier>(),
        Some(&Modifier::OpposingRetreatCostDelta(1))
    );
}
