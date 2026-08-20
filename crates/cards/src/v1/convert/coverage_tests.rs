#![allow(clippy::expect_used)]

use super::*;
use summoners_core::domain::cards::{Attack, Persistent, Respondable, Skill, Trigger};

fn code(value: &str) -> model::StableCode {
    model::StableCode::parse(value.to_string(), crate::StableKeyKind::Card, "code")
        .expect("test code is valid")
}

fn id(value: u8) -> EntityId {
    EntityId::parse(&format!("{value:02x}").repeat(16)).expect("test id is valid")
}

fn condition_spell() -> dto::Condition {
    dto::Condition::SpellPlayedThisTurn {
        player: dto::RelativePlayer::Controller,
    }
}

fn condition_defender() -> dto::Condition {
    dto::Condition::DefenderEnteredMainThisTurn {
        player: dto::RelativePlayer::Controller,
    }
}

fn damage(base: u32) -> model::Effect {
    model::Effect::Damage {
        target: model::DamageTarget::DefendingMain,
        base,
        constraints: Vec::new(),
        additions: Vec::new(),
    }
}

fn ability(code_value: &str, kind: model::AbilityKind) -> model::Ability {
    model::Ability {
        code: code(code_value),
        name: code_value.to_string(),
        text: "display text is not converted".to_string(),
        kind,
    }
}

fn set(card: model::Card) -> model::Set {
    model::Set {
        code: code("test-set"),
        revision: 3,
        name: "Test Set".to_string(),
        cards: vec![card],
    }
}

#[test]
fn closed_scalar_mappings_cover_every_variant() {
    for (source, expected) in [
        (dto::Form::Base, Form::Base),
        (dto::Form::Enhanced, Form::Enhanced),
        (dto::Form::Elite, Form::Elite),
    ] {
        assert_eq!(convert_form(source), expected);
    }
    for (source, expected) in [
        (dto::ManaType::Matter, ManaType::Matter),
        (dto::ManaType::Mind, ManaType::Mind),
        (dto::ManaType::Spirit, ManaType::Spirit),
    ] {
        assert_eq!(convert_mana_type(source), expected);
    }
    for (source, expected) in [
        (dto::SpellTiming::Support, SpellTiming::Support),
        (dto::SpellTiming::Attack, SpellTiming::Attack),
    ] {
        assert_eq!(convert_timing(source), expected);
    }
    for (source, expected) in [
        (model::OwnTarget::Selected, EffectTarget::Selected),
        (model::OwnTarget::Source, EffectTarget::Source),
    ] {
        assert_eq!(convert_own_target(source), expected);
    }
    for (source, expected) in [
        (
            dto::DamageConstraint::Unincreasable,
            DamageConstraint::Unincreasable,
        ),
        (
            dto::DamageConstraint::Unpreventable,
            DamageConstraint::Unpreventable,
        ),
    ] {
        assert_eq!(convert_damage_constraint(source), expected);
    }
}

#[test]
fn event_and_condition_mappings_cover_every_variant() {
    for (source, expected) in [
        (dto::TriggerEvent::YourUpkeep, TriggerEvent::YourUpkeep),
        (dto::TriggerEvent::EntersMain, TriggerEvent::EntersMain),
        (dto::TriggerEvent::EntersBench, TriggerEvent::EntersBench),
        (dto::TriggerEvent::LeavesMain, TriggerEvent::LeavesMain),
        (dto::TriggerEvent::LeavesBench, TriggerEvent::LeavesBench),
        (
            dto::TriggerEvent::AnySummonDestroyed,
            TriggerEvent::AnySummonDestroyed,
        ),
    ] {
        assert_eq!(convert_trigger_event(source), expected);
    }
    for (source, expected) in [
        (
            condition_defender(),
            EffectCondition::DefenderEnteredMainThisTurn,
        ),
        (condition_spell(), EffectCondition::SpellPlayedThisTurn),
    ] {
        assert_eq!(convert_condition(source), expected);
    }
}

#[test]
fn cost_and_constraint_collections_preserve_all_authored_values() {
    assert_eq!(
        convert_cost(&[
            dto::ManaSymbol::Matter,
            dto::ManaSymbol::Mind,
            dto::ManaSymbol::Spirit,
            dto::ManaSymbol::Generic,
            dto::ManaSymbol::Generic,
        ]),
        Cost {
            matter: 1,
            mind: 1,
            spirit: 1,
            generic: 2,
        }
    );
    let constraints = convert_damage_constraints(vec![
        dto::DamageConstraint::Unpreventable,
        dto::DamageConstraint::Unincreasable,
    ]);
    assert!(constraints.contains(DamageConstraint::Unpreventable));
    assert!(constraints.contains(DamageConstraint::Unincreasable));
}

#[test]
fn effect_conversion_covers_every_leaf_and_selector_branch() {
    let damage_constraints = DamageConstraints::from([
        DamageConstraint::Unpreventable,
        DamageConstraint::Unincreasable,
    ]);
    let cases = vec![
        (
            model::Effect::Damage {
                target: model::DamageTarget::DefendingMain,
                base: 30,
                constraints: vec![
                    dto::DamageConstraint::Unpreventable,
                    dto::DamageConstraint::Unincreasable,
                ],
                additions: vec![model::DamageAddition {
                    amount: 20,
                    condition: condition_spell(),
                }],
            },
            EffectLeaf::DealDamage(DamageEffect {
                base: 30,
                constraints: damage_constraints,
                additions: vec![DamageAddition {
                    amount: 20,
                    condition: EffectCondition::SpellPlayedThisTurn,
                }],
            }),
        ),
        (
            model::Effect::Damage {
                target: model::DamageTarget::SelectedOpposingPosition,
                base: 10,
                constraints: Vec::new(),
                additions: Vec::new(),
            },
            EffectLeaf::DealDamage(DamageEffect {
                base: 10,
                constraints: DamageConstraints::new(),
                additions: Vec::new(),
            }),
        ),
        (
            model::Effect::Heal {
                target: model::OwnTarget::Selected,
                amount: 10,
            },
            EffectLeaf::Heal {
                amount: 10,
                target: EffectTarget::Selected,
            },
        ),
        (
            model::Effect::Heal {
                target: model::OwnTarget::Source,
                amount: 15,
            },
            EffectLeaf::Heal {
                amount: 15,
                target: EffectTarget::Source,
            },
        ),
        (
            model::Effect::MoveOwnBenchedToEmptyBench,
            EffectLeaf::MoveSummon,
        ),
        (
            model::Effect::SwapPositions {
                side: model::SwapSide::Own,
            },
            EffectLeaf::SwapPositions,
        ),
        (
            model::Effect::SwapPositions {
                side: model::SwapSide::Opposing,
            },
            EffectLeaf::SwapOpposingPositions,
        ),
        (
            model::Effect::BlockAttackSpells {
                condition: condition_defender(),
            },
            EffectLeaf::BlockResponses {
                condition: EffectCondition::DefenderEnteredMainThisTurn,
                block: ResponseBlock::AttackSpells,
            },
        ),
        (
            model::Effect::ReturnSpellFromDiscard,
            EffectLeaf::ReturnSpellFromDiscard,
        ),
        (model::Effect::LookAtPrizes, EffectLeaf::LookAtPrizes),
        (
            model::Effect::DrawCards { amount: 2 },
            EffectLeaf::DrawCards { amount: 2 },
        ),
        (
            model::Effect::ReturnSpellToDeckTop,
            EffectLeaf::ReturnSpellToDeckTop,
        ),
        (
            model::Effect::ProduceMana {
                target: model::OwnTarget::Selected,
            },
            EffectLeaf::ProduceMana {
                target: EffectTarget::Selected,
            },
        ),
        (
            model::Effect::ProduceMana {
                target: model::OwnTarget::Source,
            },
            EffectLeaf::ProduceMana {
                target: EffectTarget::Source,
            },
        ),
        (
            model::Effect::ProtectFromOpposingMovement {
                target: model::OwnTarget::Source,
            },
            EffectLeaf::CannotBeMovedByOpponent {
                target: EffectTarget::Source,
            },
        ),
        (model::Effect::ReadyOwnSummon, EffectLeaf::ReadySummon),
    ];
    for (source, expected) in cases {
        assert_eq!(convert_effect(source), expected);
    }
}

#[test]
fn card_conversion_covers_summon_and_every_ability_role() {
    let summon = model::Card {
        code: code("test-summon"),
        name: "Test Summon".to_string(),
        text: "not converted".to_string(),
        kind: model::CardKind::Summon {
            form: dto::Form::Elite,
            life: 90,
            types: vec![dto::ManaType::Matter, dto::ManaType::Spirit],
            retreat: 3,
            abilities: vec![
                ability(
                    "attack",
                    model::AbilityKind::Attack {
                        cost: vec![dto::ManaSymbol::Matter],
                        effects: vec![damage(20)],
                    },
                ),
                model::Ability {
                    code: code("skill"),
                    name: String::new(),
                    text: String::new(),
                    kind: model::AbilityKind::Skill {
                        cost: vec![dto::ManaSymbol::Generic],
                        effects: vec![model::Effect::MoveOwnBenchedToEmptyBench],
                    },
                },
                ability(
                    "passive",
                    model::AbilityKind::Passive {
                        modifier: dto::Modifier::OpposingRetreatCost { amount: 2 },
                    },
                ),
                ability(
                    "immediate-trigger",
                    model::AbilityKind::Trigger {
                        event: dto::TriggerEvent::EntersMain,
                        response: dto::ResponseMode::Immediate,
                        effects: vec![model::Effect::Heal {
                            target: model::OwnTarget::Source,
                            amount: 10,
                        }],
                    },
                ),
                ability(
                    "respondable-trigger",
                    model::AbilityKind::Trigger {
                        event: dto::TriggerEvent::AnySummonDestroyed,
                        response: dto::ResponseMode::Respondable,
                        effects: vec![model::Effect::ReturnSpellFromDiscard],
                    },
                ),
            ],
        },
    };
    let loaded = convert(set(summon)).expect("model converts");
    let entity = &loaded.cards().entities()[0];
    assert_eq!(entity.get::<Form>(), Some(&Form::Elite));
    assert_eq!(entity.get::<Life>(), Some(&Life(90)));
    assert_eq!(
        entity.get::<ManaTypes>(),
        Some(&ManaTypes(vec![ManaType::Matter, ManaType::Spirit]))
    );
    assert_eq!(entity.get::<RetreatCost>(), Some(&RetreatCost(3)));

    let attack = entity.all::<Attack>()[0];
    assert_eq!(attack.get::<Name>(), Some(&Name("attack".to_string())));
    assert_eq!(
        attack.get::<Cost>(),
        Some(&Cost {
            matter: 1,
            ..Cost::default()
        })
    );
    assert_eq!(
        attack.get::<EffectLeaf>(),
        Some(&convert_effect(damage(20)))
    );

    let skill = entity.all::<Skill>()[0];
    assert_eq!(skill.get::<Name>(), None);
    assert_eq!(
        skill.get::<Cost>(),
        Some(&Cost {
            generic: 1,
            ..Cost::default()
        })
    );
    assert_eq!(skill.get::<EffectLeaf>(), Some(&EffectLeaf::MoveSummon));
    assert_eq!(
        entity.get::<Modifier>(),
        Some(&Modifier::OpposingRetreatCostDelta(2))
    );

    let triggers = entity.all::<Trigger>();
    assert_eq!(triggers.len(), 2);
    assert_eq!(
        triggers[0].get::<TriggerEvent>(),
        Some(&TriggerEvent::EntersMain)
    );
    assert_eq!(triggers[0].get::<Respondable>(), None);
    assert_eq!(
        triggers[1].get::<TriggerEvent>(),
        Some(&TriggerEvent::AnySummonDestroyed)
    );
    assert!(triggers[1].get::<Respondable>().is_some());

    for ability_code in [
        "attack",
        "skill",
        "passive",
        "immediate-trigger",
        "respondable-trigger",
    ] {
        assert!(loaded.ability_id("test-summon", ability_code).is_some());
    }
}

#[test]
fn card_conversion_covers_spell_and_enchantment_families() {
    let spell = model::Card {
        code: code("test-spell"),
        name: "Test Spell".to_string(),
        text: String::new(),
        kind: model::CardKind::Spell {
            timing: dto::SpellTiming::Attack,
            cost: vec![dto::ManaSymbol::Mind, dto::ManaSymbol::Generic],
            effects: vec![damage(10), model::Effect::DrawCards { amount: 1 }],
        },
    };
    let loaded = convert(set(spell)).expect("Spell converts");
    let entity = &loaded.cards().entities()[0];
    assert_eq!(entity.get::<Tags>(), Some(&Tags(vec!["spell".to_string()])));
    assert_eq!(entity.get::<SpellTiming>(), Some(&SpellTiming::Attack));
    assert_eq!(
        entity.get::<Cost>(),
        Some(&Cost {
            mind: 1,
            generic: 1,
            ..Cost::default()
        })
    );
    assert_eq!(
        entity.all::<EffectLeaf>(),
        vec![
            &convert_effect(damage(10)),
            &EffectLeaf::DrawCards { amount: 1 }
        ]
    );
    assert_eq!(entity.get::<Persistent>(), None);

    let enchantment = model::Card {
        code: code("test-enchantment"),
        name: "Test Enchantment".to_string(),
        text: String::new(),
        kind: model::CardKind::Enchantment {
            cost: vec![dto::ManaSymbol::Spirit],
            effects: vec![model::Effect::Heal {
                target: model::OwnTarget::Selected,
                amount: 10,
            }],
            modifiers: vec![dto::Modifier::IncomingAttackDamageReduction { amount: 5 }],
        },
    };
    let loaded = convert(set(enchantment)).expect("Enchantment converts");
    let entity = &loaded.cards().entities()[0];
    assert_eq!(
        entity.get::<Tags>(),
        Some(&Tags(vec!["enchantment".to_string()]))
    );
    assert_eq!(entity.get::<SpellTiming>(), Some(&SpellTiming::Support));
    assert_eq!(
        entity.get::<Modifier>(),
        Some(&Modifier::IncomingAttackDamageReduction(5))
    );
    assert!(entity.get::<Persistent>().is_some());
}

#[test]
fn modifier_conversion_covers_both_variants_and_range_failure() {
    assert_eq!(
        convert_modifier(dto::Modifier::OpposingRetreatCost { amount: 3 }, "modifier")
            .expect("retreat modifier converts"),
        Modifier::OpposingRetreatCostDelta(3)
    );
    assert_eq!(
        convert_modifier(
            dto::Modifier::IncomingAttackDamageReduction { amount: 10 },
            "modifier"
        )
        .expect("reduction modifier converts"),
        Modifier::IncomingAttackDamageReduction(10)
    );
    let error = convert_modifier(
        dto::Modifier::OpposingRetreatCost {
            amount: i32::MAX as u32 + 1,
        },
        "modifier",
    )
    .expect_err("out-of-range modifier must fail conversion");
    assert_eq!(error.phase, LoadPhase::Conversion);
    assert_eq!(error.path, "modifier.amount");
    assert_eq!(
        error.cause,
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::ModifierAmountOutOfRange,
        }
    );
}

#[test]
fn direct_card_and_ability_conversion_use_the_given_identity_registry() {
    let card_id = id(7);
    let set_code = code("test-set");
    let mut registry = IdentityRegistry {
        seen_ids: HashMap::from([(id(1), "id".to_string())]),
        ability_ids: BTreeMap::new(),
    };
    let entity = convert_card(
        &set_code,
        model::Card {
            code: code("direct-card"),
            name: "Direct Card".to_string(),
            text: String::new(),
            kind: model::CardKind::Spell {
                timing: dto::SpellTiming::Support,
                cost: Vec::new(),
                effects: vec![model::Effect::DrawCards { amount: 1 }],
            },
        },
        card_id,
        0,
        &mut registry,
    )
    .expect("card converts");
    assert_eq!(entity.id, card_id);
}
