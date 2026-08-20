#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::*;

fn stable_code(value: &str) -> StableCode {
    StableCode::parse(value.to_string(), StableKeyKind::Card, "code").unwrap()
}

fn condition() -> dto::Condition {
    dto::Condition::SpellPlayedThisTurn {
        player: dto::RelativePlayer::Controller,
    }
}

fn damage(target: dto::Target, base: u32) -> dto::Effect {
    dto::Effect::Damage {
        target,
        base,
        constraints: Vec::new(),
        additions: Vec::new(),
    }
}

fn attack_ability() -> dto::Ability {
    dto::Ability {
        code: "strike".to_string(),
        name: "Strike".to_string(),
        text: String::new(),
        kind: dto::AbilityKind::Attack,
        cost: Some(Vec::new()),
        event: None,
        response: None,
        effects: vec![damage(dto::Target::DefendingMain, 10)],
        modifier: None,
    }
}

fn summon_card() -> dto::Card {
    dto::Card {
        code: "test-summon".to_string(),
        name: "Test Summon".to_string(),
        text: String::new(),
        kind: dto::CardKind::Summon,
        form: Some(dto::Form::Base),
        life: Some(50),
        types: Some(vec![dto::ManaType::Matter]),
        retreat: Some(1),
        timing: None,
        persistence: None,
        cost: None,
        abilities: vec![attack_ability()],
        effects: Vec::new(),
        modifiers: Vec::new(),
    }
}

fn spell_card() -> dto::Card {
    dto::Card {
        code: "test-spell".to_string(),
        name: "Test Spell".to_string(),
        text: String::new(),
        kind: dto::CardKind::Spell,
        form: None,
        life: None,
        types: None,
        retreat: None,
        timing: Some(dto::SpellTiming::Support),
        persistence: Some(dto::Persistence::Discard),
        cost: Some(vec![dto::ManaSymbol::Generic]),
        abilities: Vec::new(),
        effects: vec![dto::Effect::DrawCards { amount: 1 }],
        modifiers: Vec::new(),
    }
}

fn enchantment_card() -> dto::Card {
    dto::Card {
        code: "test-enchantment".to_string(),
        name: "Test Enchantment".to_string(),
        text: String::new(),
        kind: dto::CardKind::Enchantment,
        form: None,
        life: None,
        types: None,
        retreat: None,
        timing: Some(dto::SpellTiming::Support),
        persistence: Some(dto::Persistence::Persistent),
        cost: Some(vec![dto::ManaSymbol::Generic]),
        abilities: Vec::new(),
        effects: vec![dto::Effect::Heal {
            target: dto::Target::SelectedOwnSummon,
            amount: 10,
        }],
        modifiers: vec![dto::Modifier::IncomingAttackDamageReduction { amount: 10 }],
    }
}

fn assert_rule(error: SetLoadError, path: &str, rule: SemanticRule) {
    let SetLoadError {
        phase,
        path: actual_path,
        cause,
        ..
    } = error;
    assert_eq!(phase, LoadPhase::Semantics);
    assert_eq!(actual_path, path);
    assert_eq!(cause, SetLoadCause::InvalidSemantics { rule });
}

#[test]
fn stable_codes_and_ability_roles_cover_all_closed_variants() {
    let code = stable_code("valid-code");
    assert_eq!(code.as_str(), "valid-code");
    let roles = [
        (AbilityRole::Attack, "attack"),
        (AbilityRole::Skill, "skill"),
        (AbilityRole::Passive, "passive"),
        (AbilityRole::Trigger, "trigger"),
    ];
    for (role, name) in roles {
        assert_eq!(role.identity_name(), name);
    }

    let kinds = [
        AbilityKind::Attack {
            cost: Vec::new(),
            effects: Vec::new(),
        },
        AbilityKind::Skill {
            cost: Vec::new(),
            effects: Vec::new(),
        },
        AbilityKind::Passive {
            modifier: dto::Modifier::OpposingRetreatCost { amount: 1 },
        },
        AbilityKind::Trigger {
            event: dto::TriggerEvent::YourUpkeep,
            response: dto::ResponseMode::Immediate,
            effects: Vec::new(),
        },
    ];
    for (kind, expected) in kinds.into_iter().zip(roles.map(|(role, _)| role)) {
        let ability = Ability {
            code: stable_code("ability"),
            name: String::new(),
            text: String::new(),
            kind,
        };
        assert_eq!(ability.role(), expected);
    }
}

#[test]
fn set_policy_covers_header_and_collection_failures() {
    let make_set = || dto::Set {
        schema_version: 1,
        id: "test-set".to_string(),
        revision: 1,
        name: "Test Set".to_string(),
        cards: vec![summon_card()],
    };
    assert!(parse(make_set()).is_ok());

    let mut wrong_version = make_set();
    wrong_version.schema_version = 2;
    let error = parse(wrong_version).unwrap_err();
    assert_eq!(error.phase, LoadPhase::Version);
    assert_eq!(error.path, "schema_version");

    let mut zero_revision = make_set();
    zero_revision.revision = 0;
    assert_rule(
        parse(zero_revision).unwrap_err(),
        "revision",
        SemanticRule::RevisionMustBePositive,
    );

    let mut blank_name = make_set();
    blank_name.name = "  ".to_string();
    assert_rule(
        parse(blank_name).unwrap_err(),
        "name",
        SemanticRule::NameMustNotBeEmpty,
    );

    let mut empty = make_set();
    empty.cards.clear();
    assert_rule(
        parse(empty).unwrap_err(),
        "cards",
        SemanticRule::SetMustContainCards,
    );
}

#[test]
fn card_policy_accepts_all_three_families() {
    let summon = parse_card(summon_card(), stable_code("summon"), "cards[0]").unwrap();
    assert!(matches!(summon.kind, CardKind::Summon { .. }));

    let mut attack_spell = spell_card();
    attack_spell.timing = Some(dto::SpellTiming::Attack);
    attack_spell.effects = vec![damage(dto::Target::SelectedOpposingPosition, 10)];
    let spell = parse_card(attack_spell, stable_code("spell"), "cards[0]").unwrap();
    assert!(matches!(
        spell.kind,
        CardKind::Spell {
            timing: dto::SpellTiming::Attack,
            ..
        }
    ));

    let enchantment =
        parse_card(enchantment_card(), stable_code("enchantment"), "cards[0]").unwrap();
    assert!(matches!(enchantment.kind, CardKind::Enchantment { .. }));
}

#[test]
fn summon_card_policy_rejects_every_forbidden_and_required_shape_branch() {
    enum Fault {
        BlankName,
        Timing,
        Persistence,
        Cost,
        Effects,
        Modifiers,
        Form,
        Life,
        ZeroLife,
        Types,
        EmptyTypes,
        DuplicateTypes,
        Retreat,
        Abilities,
        TwoAttacks,
    }
    let cases = [
        (
            Fault::BlankName,
            "cards[0].name",
            SemanticRule::NameMustNotBeEmpty,
        ),
        (
            Fault::Timing,
            "cards[0].timing",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Persistence,
            "cards[0].persistence",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Cost,
            "cards[0].cost",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Effects,
            "cards[0].effects",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Modifiers,
            "cards[0].modifiers",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Form,
            "cards[0].form",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Life,
            "cards[0].life",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::ZeroLife,
            "cards[0].life",
            SemanticRule::SummonRequiresStatistics,
        ),
        (
            Fault::Types,
            "cards[0].types",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::EmptyTypes,
            "cards[0].types",
            SemanticRule::SummonRequiresStatistics,
        ),
        (
            Fault::DuplicateTypes,
            "cards[0].types[1]",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Retreat,
            "cards[0].retreat",
            SemanticRule::NonSummonForbidsStatistics,
        ),
        (
            Fault::Abilities,
            "cards[0].abilities",
            SemanticRule::SummonRequiresOneAttack,
        ),
        (
            Fault::TwoAttacks,
            "cards[0].abilities",
            SemanticRule::SummonRequiresOneAttack,
        ),
    ];

    for (fault, path, fallback_rule) in cases {
        let mut card = summon_card();
        match fault {
            Fault::BlankName => card.name = " ".to_string(),
            Fault::Timing => card.timing = Some(dto::SpellTiming::Support),
            Fault::Persistence => card.persistence = Some(dto::Persistence::Discard),
            Fault::Cost => card.cost = Some(Vec::new()),
            Fault::Effects => card.effects.push(dto::Effect::DrawCards { amount: 1 }),
            Fault::Modifiers => card
                .modifiers
                .push(dto::Modifier::IncomingAttackDamageReduction { amount: 1 }),
            Fault::Form => card.form = None,
            Fault::Life => card.life = None,
            Fault::ZeroLife => card.life = Some(0),
            Fault::Types => card.types = None,
            Fault::EmptyTypes => card.types = Some(Vec::new()),
            Fault::DuplicateTypes => {
                card.types = Some(vec![dto::ManaType::Matter, dto::ManaType::Matter]);
            }
            Fault::Retreat => card.retreat = None,
            Fault::Abilities => card.abilities.clear(),
            Fault::TwoAttacks => {
                let mut second = attack_ability();
                second.code = "second-strike".to_string();
                card.abilities.push(second);
            }
        }
        let error = parse_card(card, stable_code("summon"), "cards[0]").unwrap_err();
        assert_eq!(error.path, path);
        match fault {
            Fault::DuplicateTypes => {
                assert!(matches!(error.cause, SetLoadCause::DuplicateValue { .. }));
            }
            Fault::Form | Fault::Life | Fault::Types | Fault::Retreat => {
                assert!(matches!(
                    error.cause,
                    SetLoadCause::MissingRequiredField { .. }
                ));
            }
            Fault::Timing
            | Fault::Persistence
            | Fault::Cost
            | Fault::Effects
            | Fault::Modifiers => {
                assert!(matches!(error.cause, SetLoadCause::ForbiddenField { .. }));
            }
            _ => assert_eq!(
                error.cause,
                SetLoadCause::InvalidSemantics {
                    rule: fallback_rule,
                }
            ),
        }
    }
}

#[test]
fn spell_and_enchantment_policy_reject_invalid_shapes() {
    let mut spell_with_form = spell_card();
    spell_with_form.form = Some(dto::Form::Base);
    let error = parse_card(spell_with_form, stable_code("spell"), "cards[0]").unwrap_err();
    assert_eq!(error.path, "cards[0].form");
    assert!(matches!(
        error.cause,
        SetLoadCause::ForbiddenField { field: "form" }
    ));

    for field in ["life", "types", "retreat"] {
        let mut card = spell_card();
        match field {
            "life" => card.life = Some(10),
            "types" => card.types = Some(vec![dto::ManaType::Matter]),
            "retreat" => card.retreat = Some(1),
            _ => unreachable!(),
        }
        let error = parse_card(card, stable_code("spell"), "cards[0]").unwrap_err();
        assert_eq!(error.path, format!("cards[0].{field}"));
        assert!(matches!(error.cause, SetLoadCause::ForbiddenField { .. }));
    }

    let mut spell_abilities = spell_card();
    spell_abilities.abilities.push(attack_ability());
    assert!(matches!(
        parse_card(spell_abilities, stable_code("spell"), "cards[0]")
            .unwrap_err()
            .cause,
        SetLoadCause::ForbiddenField { field: "abilities" }
    ));
    let mut spell_modifiers = spell_card();
    spell_modifiers
        .modifiers
        .push(dto::Modifier::IncomingAttackDamageReduction { amount: 1 });
    assert!(matches!(
        parse_card(spell_modifiers, stable_code("spell"), "cards[0]")
            .unwrap_err()
            .cause,
        SetLoadCause::ForbiddenField { field: "modifiers" }
    ));

    for field in ["timing", "persistence", "cost"] {
        let mut card = spell_card();
        match field {
            "timing" => card.timing = None,
            "persistence" => card.persistence = None,
            "cost" => card.cost = None,
            _ => unreachable!(),
        }
        let error = parse_card(card, stable_code("spell"), "cards[0]").unwrap_err();
        assert_eq!(error.path, format!("cards[0].{field}"));
        assert!(matches!(
            error.cause,
            SetLoadCause::MissingRequiredField { .. }
        ));
    }
    let mut persistent_spell = spell_card();
    persistent_spell.persistence = Some(dto::Persistence::Persistent);
    assert_rule(
        parse_card(persistent_spell, stable_code("spell"), "cards[0]").unwrap_err(),
        "cards[0].persistence",
        SemanticRule::SpellShape,
    );

    let mut bad_enchantment = enchantment_card();
    bad_enchantment.timing = Some(dto::SpellTiming::Attack);
    assert_rule(
        parse_card(bad_enchantment, stable_code("enchantment"), "cards[0]").unwrap_err(),
        "cards[0]",
        SemanticRule::EnchantmentShape,
    );
    let mut bad_enchantment = enchantment_card();
    bad_enchantment.persistence = Some(dto::Persistence::Discard);
    assert_rule(
        parse_card(bad_enchantment, stable_code("enchantment"), "cards[0]").unwrap_err(),
        "cards[0]",
        SemanticRule::EnchantmentShape,
    );
}

#[test]
fn ability_policy_accepts_every_role_and_response_mode() {
    let attack = parse_ability(attack_ability(), stable_code("attack"), "ability").unwrap();
    assert!(matches!(attack.kind, AbilityKind::Attack { .. }));

    let mut skill = attack_ability();
    skill.kind = dto::AbilityKind::Skill;
    skill.effects = vec![dto::Effect::LookAtPrizes];
    assert!(matches!(
        parse_ability(skill, stable_code("skill"), "ability")
            .unwrap()
            .kind,
        AbilityKind::Skill { .. }
    ));

    let passive = dto::Ability {
        code: "passive".to_string(),
        name: String::new(),
        text: String::new(),
        kind: dto::AbilityKind::Passive,
        cost: None,
        event: None,
        response: None,
        effects: Vec::new(),
        modifier: Some(dto::Modifier::OpposingRetreatCost { amount: 1 }),
    };
    assert!(matches!(
        parse_ability(passive, stable_code("passive"), "ability")
            .unwrap()
            .kind,
        AbilityKind::Passive { .. }
    ));

    for response in [dto::ResponseMode::Immediate, dto::ResponseMode::Respondable] {
        let trigger = dto::Ability {
            code: "trigger".to_string(),
            name: String::new(),
            text: String::new(),
            kind: dto::AbilityKind::Trigger,
            cost: None,
            event: Some(dto::TriggerEvent::YourUpkeep),
            response: Some(response),
            effects: vec![dto::Effect::Heal {
                target: dto::Target::Source,
                amount: 1,
            }],
            modifier: None,
        };
        assert!(matches!(
            parse_ability(trigger, stable_code("trigger"), "ability")
                .unwrap()
                .kind,
            AbilityKind::Trigger { response: parsed, .. } if parsed == response
        ));
    }
}

#[test]
fn ability_policy_rejects_required_forbidden_and_duplicate_data() {
    let mut seen = HashMap::new();
    reject_duplicate(
        &mut seen,
        "same",
        "abilities[0].code",
        StableKeyKind::Ability,
    )
    .unwrap();
    let duplicate = reject_duplicate(
        &mut seen,
        "same",
        "abilities[1].code",
        StableKeyKind::Ability,
    )
    .unwrap_err();
    assert!(matches!(
        duplicate.cause,
        SetLoadCause::DuplicateStableKey { .. }
    ));

    for field in ["event", "response", "modifier"] {
        let mut ability = attack_ability();
        match field {
            "event" => ability.event = Some(dto::TriggerEvent::YourUpkeep),
            "response" => ability.response = Some(dto::ResponseMode::Immediate),
            "modifier" => {
                ability.modifier = Some(dto::Modifier::OpposingRetreatCost { amount: 1 });
            }
            _ => unreachable!(),
        }
        let error = parse_ability(ability, stable_code("attack"), "ability").unwrap_err();
        assert_eq!(error.path, format!("ability.{field}"));
        assert!(matches!(error.cause, SetLoadCause::ForbiddenField { .. }));
    }
    let mut attack = attack_ability();
    attack.cost = None;
    assert!(matches!(
        parse_ability(attack, stable_code("attack"), "ability")
            .unwrap_err()
            .cause,
        SetLoadCause::MissingRequiredField { field: "cost" }
    ));

    let mut passive = attack_ability();
    passive.kind = dto::AbilityKind::Passive;
    passive.effects.clear();
    for field in ["cost", "event", "response", "effects"] {
        let mut candidate = dto::Ability {
            code: "passive".to_string(),
            name: String::new(),
            text: String::new(),
            kind: dto::AbilityKind::Passive,
            cost: None,
            event: None,
            response: None,
            effects: Vec::new(),
            modifier: Some(dto::Modifier::OpposingRetreatCost { amount: 1 }),
        };
        match field {
            "cost" => candidate.cost = Some(Vec::new()),
            "event" => candidate.event = Some(dto::TriggerEvent::YourUpkeep),
            "response" => candidate.response = Some(dto::ResponseMode::Immediate),
            "effects" => candidate.effects.push(dto::Effect::LookAtPrizes),
            _ => unreachable!(),
        }
        assert!(matches!(
            parse_ability(candidate, stable_code("passive"), "ability")
                .unwrap_err()
                .cause,
            SetLoadCause::ForbiddenField { .. }
        ));
    }

    let mut trigger = attack_ability();
    trigger.kind = dto::AbilityKind::Trigger;
    trigger.cost = None;
    trigger.effects = vec![dto::Effect::Heal {
        target: dto::Target::Source,
        amount: 1,
    }];
    trigger.event = Some(dto::TriggerEvent::YourUpkeep);
    trigger.response = Some(dto::ResponseMode::Immediate);
    trigger.cost = Some(Vec::new());
    assert!(matches!(
        parse_ability(trigger, stable_code("trigger"), "ability")
            .unwrap_err()
            .cause,
        SetLoadCause::ForbiddenField { field: "cost" }
    ));
}

#[test]
fn effect_policy_accepts_every_family_and_preserves_ordered_data() {
    let parsed = parse_effect(
        dto::Effect::Damage {
            target: dto::Target::DefendingMain,
            base: 10,
            constraints: vec![dto::DamageConstraint::Unpreventable],
            additions: vec![dto::DamageAddition {
                amount: 20,
                condition: condition(),
            }],
        },
        EffectContext::Attack,
        "effect",
    )
    .unwrap();
    assert!(matches!(
        parsed,
        Effect::Damage {
            target: DamageTarget::DefendingMain,
            base: 10,
            constraints,
            additions,
        } if constraints == [dto::DamageConstraint::Unpreventable]
            && additions[0].amount == 20
    ));
    assert!(matches!(
        parse_effect(
            damage(dto::Target::SelectedOpposingPosition, 10),
            EffectContext::AttackSpell,
            "effect"
        )
        .unwrap(),
        Effect::Damage {
            target: DamageTarget::SelectedOpposingPosition,
            ..
        }
    ));

    for (context, target, expected) in [
        (EffectContext::Skill, dto::Target::Source, OwnTarget::Source),
        (
            EffectContext::Trigger,
            dto::Target::Source,
            OwnTarget::Source,
        ),
        (
            EffectContext::SupportSpell,
            dto::Target::SelectedOwnSummon,
            OwnTarget::Selected,
        ),
        (
            EffectContext::Enchantment,
            dto::Target::SelectedOwnSummon,
            OwnTarget::Selected,
        ),
    ] {
        assert!(matches!(
            parse_effect(dto::Effect::Heal { target, amount: 5 }, context, "effect").unwrap(),
            Effect::Heal { target: parsed, amount: 5 } if parsed == expected
        ));
    }

    assert!(matches!(
        parse_effect(
            dto::Effect::MoveSummon {
                target: dto::Target::SelectedOwnBenchedSummon,
                destination: dto::Destination::EmptyOwnBench,
            },
            EffectContext::Skill,
            "effect",
        )
        .unwrap(),
        Effect::MoveOwnBenchedToEmptyBench
    ));
    for (target, side) in [
        (dto::Target::OwnMainWithSelectedBench, SwapSide::Own),
        (
            dto::Target::OpposingMainWithSelectedBench,
            SwapSide::Opposing,
        ),
    ] {
        assert!(matches!(
            parse_effect(
                dto::Effect::SwapPositions { target },
                EffectContext::Skill,
                "effect"
            )
            .unwrap(),
            Effect::SwapPositions { side: parsed } if parsed == side
        ));
    }
    assert!(matches!(
        parse_effect(
            dto::Effect::BlockResponses {
                condition: condition(),
                response: dto::ResponseBlock::AttackSpells,
            },
            EffectContext::Attack,
            "effect",
        )
        .unwrap(),
        Effect::BlockAttackSpells { .. }
    ));

    let simple = [
        (
            dto::Effect::ReturnSpellFromDiscard,
            EffectContext::Trigger,
            "discard",
        ),
        (dto::Effect::LookAtPrizes, EffectContext::Skill, "prizes"),
        (
            dto::Effect::DrawCards { amount: 2 },
            EffectContext::SupportSpell,
            "draw",
        ),
        (
            dto::Effect::ReturnSpellToDeckTop,
            EffectContext::Skill,
            "deck-top",
        ),
    ];
    for (effect, context, expected) in simple {
        let parsed = parse_effect(effect, context, "effect").unwrap();
        assert!(matches!(
            (expected, parsed),
            ("discard", Effect::ReturnSpellFromDiscard)
                | ("prizes", Effect::LookAtPrizes)
                | ("draw", Effect::DrawCards { amount: 2 })
                | ("deck-top", Effect::ReturnSpellToDeckTop)
        ));
    }

    for (context, target, expected) in [
        (
            EffectContext::Skill,
            dto::Target::SelectedOwnSummon,
            OwnTarget::Selected,
        ),
        (
            EffectContext::Trigger,
            dto::Target::Source,
            OwnTarget::Source,
        ),
    ] {
        assert!(matches!(
            parse_effect(dto::Effect::ProduceMana { target }, context, "effect").unwrap(),
            Effect::ProduceMana { target: parsed } if parsed == expected
        ));
    }
    assert!(matches!(
        parse_effect(
            dto::Effect::CannotBeMovedByOpponent {
                target: dto::Target::Source,
                duration: dto::Duration::UntilYourNextTurn,
            },
            EffectContext::Skill,
            "effect",
        )
        .unwrap(),
        Effect::ProtectFromOpposingMovement {
            target: OwnTarget::Source,
        }
    ));
    assert!(matches!(
        parse_effect(
            dto::Effect::ReadySummon {
                target: dto::Target::SelectedOwnSummon,
            },
            EffectContext::SupportSpell,
            "effect",
        )
        .unwrap(),
        Effect::ReadyOwnSummon
    ));
}

#[test]
fn effect_policy_rejects_every_invalid_context_target_and_amount_branch() {
    let invalid = vec![
        (
            damage(dto::Target::DefendingMain, 0),
            EffectContext::Attack,
            "effect.base",
            SemanticRule::AmountMustBePositive,
        ),
        (
            damage(dto::Target::DefendingMain, 10),
            EffectContext::Skill,
            "effect.target",
            SemanticRule::EffectTarget,
        ),
        (
            dto::Effect::Heal {
                target: dto::Target::Source,
                amount: 0,
            },
            EffectContext::Skill,
            "effect.amount",
            SemanticRule::AmountMustBePositive,
        ),
        (
            dto::Effect::Heal {
                target: dto::Target::SelectedOwnSummon,
                amount: 1,
            },
            EffectContext::Skill,
            "effect.target",
            SemanticRule::EffectTarget,
        ),
        (
            dto::Effect::SwapPositions {
                target: dto::Target::OwnMainWithSelectedBench,
            },
            EffectContext::Trigger,
            "effect.target",
            SemanticRule::EffectTarget,
        ),
        (
            dto::Effect::ProduceMana {
                target: dto::Target::Source,
            },
            EffectContext::Skill,
            "effect.target",
            SemanticRule::EffectTarget,
        ),
    ];
    for (effect, context, path, rule) in invalid {
        assert_rule(
            parse_effect(effect, context, "effect").unwrap_err(),
            path,
            rule,
        );
    }

    let combinations = [
        (
            dto::Effect::MoveSummon {
                target: dto::Target::SelectedOwnBenchedSummon,
                destination: dto::Destination::EmptyOwnBench,
            },
            EffectContext::Trigger,
        ),
        (
            dto::Effect::BlockResponses {
                condition: condition(),
                response: dto::ResponseBlock::AttackSpells,
            },
            EffectContext::Skill,
        ),
        (dto::Effect::ReturnSpellFromDiscard, EffectContext::Skill),
        (dto::Effect::LookAtPrizes, EffectContext::Trigger),
        (dto::Effect::DrawCards { amount: 1 }, EffectContext::Trigger),
        (dto::Effect::ReturnSpellToDeckTop, EffectContext::Trigger),
        (
            dto::Effect::CannotBeMovedByOpponent {
                target: dto::Target::SelectedOwnSummon,
                duration: dto::Duration::UntilYourNextTurn,
            },
            EffectContext::Skill,
        ),
        (
            dto::Effect::ReadySummon {
                target: dto::Target::SelectedOwnSummon,
            },
            EffectContext::Skill,
        ),
    ];
    for (effect, context) in combinations {
        assert_rule(
            parse_effect(effect, context, "effect").unwrap_err(),
            "effect",
            SemanticRule::EffectCombination,
        );
    }

    let zero_draw = parse_effect(
        dto::Effect::DrawCards { amount: 0 },
        EffectContext::Skill,
        "effect",
    )
    .unwrap_err();
    assert_rule(
        zero_draw,
        "effect.amount",
        SemanticRule::AmountMustBePositive,
    );
    let zero_addition = parse_effect(
        dto::Effect::Damage {
            target: dto::Target::DefendingMain,
            base: 10,
            constraints: Vec::new(),
            additions: vec![dto::DamageAddition {
                amount: 0,
                condition: condition(),
            }],
        },
        EffectContext::Attack,
        "effect",
    )
    .unwrap_err();
    assert_rule(
        zero_addition,
        "effect.additions[0].amount",
        SemanticRule::AmountMustBePositive,
    );
}

#[test]
fn collection_and_scalar_helpers_cover_success_and_failure() {
    assert_rule(
        parse_effects(Vec::new(), EffectContext::Skill, "effects").unwrap_err(),
        "effects",
        SemanticRule::AbilityShape,
    );
    assert_eq!(
        parse_effects(
            vec![dto::Effect::LookAtPrizes, dto::Effect::ReturnSpellToDeckTop],
            EffectContext::Skill,
            "effects",
        )
        .unwrap()
        .len(),
        2
    );

    assert_eq!(
        parse_mana_types(vec![dto::ManaType::Matter, dto::ManaType::Mind], "types")
            .unwrap()
            .len(),
        2
    );
    assert_rule(
        parse_mana_types(Vec::new(), "types").unwrap_err(),
        "types",
        SemanticRule::SummonRequiresStatistics,
    );
    assert!(matches!(
        parse_mana_types(vec![dto::ManaType::Spirit, dto::ManaType::Spirit], "types")
            .unwrap_err()
            .cause,
        SetLoadCause::DuplicateValue { .. }
    ));

    let constraints = [
        dto::DamageConstraint::Unpreventable,
        dto::DamageConstraint::Unincreasable,
    ];
    assert!(reject_duplicate_constraints(&constraints, "constraints").is_ok());
    let duplicate = [
        dto::DamageConstraint::Unpreventable,
        dto::DamageConstraint::Unpreventable,
    ];
    assert!(matches!(
        reject_duplicate_constraints(&duplicate, "constraints")
            .unwrap_err()
            .cause,
        SetLoadCause::DuplicateValue { .. }
    ));

    assert_eq!(required(Some(3), "card", "life").unwrap(), 3);
    assert!(matches!(
        required::<u32>(None, "card", "life").unwrap_err().cause,
        SetLoadCause::MissingRequiredField { field: "life" }
    ));
    assert!(forbid_option(&None::<u32>, "card", "life").is_ok());
    assert!(matches!(
        forbid_option(&Some(1), "card", "life").unwrap_err().cause,
        SetLoadCause::ForbiddenField { field: "life" }
    ));
    assert!(forbid_list::<u32>(&[], "card", "effects").is_ok());
    assert!(matches!(
        forbid_list(&[1], "card", "effects").unwrap_err().cause,
        SetLoadCause::ForbiddenField { field: "effects" }
    ));
    assert!(require_name("Name", "name").is_ok());
    assert_rule(
        require_name(" ", "name").unwrap_err(),
        "name",
        SemanticRule::NameMustNotBeEmpty,
    );
    assert!(require_positive(1, "amount", SemanticRule::AmountMustBePositive).is_ok());
    assert_rule(
        require_positive(0, "amount", SemanticRule::AmountMustBePositive).unwrap_err(),
        "amount",
        SemanticRule::AmountMustBePositive,
    );
}
