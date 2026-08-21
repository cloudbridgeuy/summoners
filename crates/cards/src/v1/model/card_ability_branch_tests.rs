#![allow(clippy::unwrap_used)]

use super::*;

fn code(value: &str) -> StableCode {
    StableCode::parse(value.to_string(), StableKeyKind::Card, "code").unwrap()
}

fn heal() -> dto::Effect {
    dto::Effect::Heal {
        target: dto::Target::SelectedOwnSummon,
        amount: 10,
    }
}

fn enchantment() -> dto::Card {
    dto::Card {
        code: "enchantment".to_string(),
        name: "Enchantment".to_string(),
        text: String::new(),
        kind: dto::CardKind::Enchantment,
        form: None,
        life: None,
        types: None,
        retreat: None,
        timing: Some(dto::SpellTiming::Support),
        persistence: Some(dto::Persistence::Persistent),
        cost: Some(Vec::new()),
        abilities: Vec::new(),
        effects: vec![heal()],
        modifiers: Vec::new(),
    }
}

fn passive() -> dto::Ability {
    dto::Ability {
        code: "passive".to_string(),
        name: String::new(),
        text: String::new(),
        kind: dto::AbilityKind::Passive,
        cost: None,
        event: None,
        response: None,
        effects: Vec::new(),
        modifier: Some(dto::Modifier::OpposingRetreatCost { amount: 1 }),
    }
}

fn trigger() -> dto::Ability {
    dto::Ability {
        code: "trigger".to_string(),
        name: String::new(),
        text: String::new(),
        kind: dto::AbilityKind::Trigger,
        cost: None,
        event: Some(dto::TriggerEvent::YourUpkeep),
        response: Some(dto::ResponseMode::Immediate),
        effects: vec![dto::Effect::Heal {
            target: dto::Target::Source,
            amount: 10,
        }],
        modifier: None,
    }
}

#[test]
fn stable_code_errors_retain_each_key_kind() {
    for kind in [
        StableKeyKind::Set,
        StableKeyKind::Card,
        StableKeyKind::Ability,
    ] {
        let error = StableCode::parse("INVALID".to_string(), kind, "code").unwrap_err();
        assert_eq!(error.path, "code");
        assert_eq!(
            error.cause,
            SetLoadCause::InvalidStableKey {
                kind,
                value: "INVALID".to_string(),
            }
        );
    }
}

#[test]
fn enchantment_branch_checks_all_forbidden_and_required_fields() {
    for field in ["form", "life", "types", "retreat", "abilities"] {
        let mut card = enchantment();
        match field {
            "form" => card.form = Some(dto::Form::Base),
            "life" => card.life = Some(10),
            "types" => card.types = Some(vec![dto::ManaType::Matter]),
            "retreat" => card.retreat = Some(1),
            "abilities" => card.abilities.push(passive()),
            _ => unreachable!(),
        }
        let error = parse_card(card, code("enchantment"), "cards[0]").unwrap_err();
        assert_eq!(error.path, format!("cards[0].{field}"));
        assert!(matches!(error.cause, SetLoadCause::ForbiddenField { .. }));
    }
    for field in ["timing", "persistence", "cost"] {
        let mut card = enchantment();
        match field {
            "timing" => card.timing = None,
            "persistence" => card.persistence = None,
            "cost" => card.cost = None,
            _ => unreachable!(),
        }
        let error = parse_card(card, code("enchantment"), "cards[0]").unwrap_err();
        assert_eq!(error.path, format!("cards[0].{field}"));
        assert!(matches!(
            error.cause,
            SetLoadCause::MissingRequiredField { .. }
        ));
    }
    let mut no_effects = enchantment();
    no_effects.effects.clear();
    let error = parse_card(no_effects, code("enchantment"), "cards[0]").unwrap_err();
    assert_eq!(error.path, "cards[0].effects");
    assert_eq!(
        error.cause,
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::AbilityShape,
        }
    );
}

#[test]
fn passive_and_trigger_branches_check_all_required_and_forbidden_fields() {
    let mut no_modifier = passive();
    no_modifier.modifier = None;
    assert!(matches!(
        parse_ability(no_modifier, code("passive"), "ability")
            .unwrap_err()
            .cause,
        SetLoadCause::MissingRequiredField { field: "modifier" }
    ));

    let mut trigger_with_modifier = trigger();
    trigger_with_modifier.modifier = Some(dto::Modifier::OpposingRetreatCost { amount: 1 });
    assert!(matches!(
        parse_ability(trigger_with_modifier, code("trigger"), "ability")
            .unwrap_err()
            .cause,
        SetLoadCause::ForbiddenField { field: "modifier" }
    ));
    for field in ["event", "response"] {
        let mut ability = trigger();
        match field {
            "event" => ability.event = None,
            "response" => ability.response = None,
            _ => unreachable!(),
        }
        let error = parse_ability(ability, code("trigger"), "ability").unwrap_err();
        assert_eq!(error.path, format!("ability.{field}"));
        assert!(matches!(
            error.cause,
            SetLoadCause::MissingRequiredField { .. }
        ));
    }
    let mut no_effects = trigger();
    no_effects.effects.clear();
    let error = parse_ability(no_effects, code("trigger"), "ability").unwrap_err();
    assert_eq!(error.path, "ability.effects");
    assert_eq!(
        error.cause,
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::AbilityShape,
        }
    );
}

#[test]
fn draw_policy_accepts_both_skill_and_support_spell_contexts() {
    for context in [EffectContext::Skill, EffectContext::SupportSpell] {
        assert!(matches!(
            parse_effect(dto::Effect::DrawCards { amount: 2 }, context, "effect").unwrap(),
            Effect::DrawCards { amount: 2 }
        ));
    }
}
