use std::collections::{HashMap, HashSet};

use crate::{
    LoadPhase, SemanticRule, SetLoadCause, SetLoadError, StableKeyKind,
    v1::dto,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StableCode(String);

impl StableCode {
    pub(crate) fn parse(
        value: String,
        kind: StableKeyKind,
        path: &str,
    ) -> Result<Self, SetLoadError> {
        if stable_key_syntax(&value) {
            Ok(Self(value))
        } else {
            Err(semantic_error(
                path,
                SetLoadCause::InvalidStableKey { kind, value },
            ))
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

fn stable_key_syntax(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && bytes.last() != Some(&b'-')
        && !bytes.windows(2).any(|pair| pair == b"--")
}

#[derive(Debug)]
pub(crate) struct Set {
    pub(crate) code: StableCode,
    pub(crate) revision: u32,
    pub(crate) name: String,
    pub(crate) cards: Vec<Card>,
}

#[derive(Debug)]
pub(crate) struct Card {
    pub(crate) code: StableCode,
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) kind: CardKind,
}

#[derive(Debug)]
pub(crate) enum CardKind {
    Summon {
        form: dto::Form,
        life: u32,
        types: Vec<dto::ManaType>,
        retreat: u32,
        abilities: Vec<Ability>,
    },
    Spell {
        timing: dto::SpellTiming,
        cost: Vec<dto::ManaSymbol>,
        effects: Vec<Effect>,
    },
    Enchantment {
        cost: Vec<dto::ManaSymbol>,
        effects: Vec<Effect>,
        modifiers: Vec<dto::Modifier>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AbilityRole {
    Attack,
    Skill,
    Passive,
    Trigger,
}

impl AbilityRole {
    pub(crate) const fn identity_name(self) -> &'static str {
        match self {
            Self::Attack => "attack",
            Self::Skill => "skill",
            Self::Passive => "passive",
            Self::Trigger => "trigger",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Ability {
    pub(crate) code: StableCode,
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) kind: AbilityKind,
}

impl Ability {
    pub(crate) const fn role(&self) -> AbilityRole {
        match self.kind {
            AbilityKind::Attack { .. } => AbilityRole::Attack,
            AbilityKind::Skill { .. } => AbilityRole::Skill,
            AbilityKind::Passive { .. } => AbilityRole::Passive,
            AbilityKind::Trigger { .. } => AbilityRole::Trigger,
        }
    }
}

#[derive(Debug)]
pub(crate) enum AbilityKind {
    Attack {
        cost: Vec<dto::ManaSymbol>,
        effects: Vec<Effect>,
    },
    Skill {
        cost: Vec<dto::ManaSymbol>,
        effects: Vec<Effect>,
    },
    Passive {
        modifier: dto::Modifier,
    },
    Trigger {
        event: dto::TriggerEvent,
        response: dto::ResponseMode,
        effects: Vec<Effect>,
    },
}

#[derive(Debug)]
pub(crate) enum Effect {
    Damage {
        target: DamageTarget,
        base: u32,
        constraints: Vec<dto::DamageConstraint>,
        additions: Vec<DamageAddition>,
    },
    Heal { target: OwnTarget, amount: u32 },
    MoveOwnBenchedToEmptyBench,
    SwapPositions { side: SwapSide },
    BlockAttackSpells { condition: dto::Condition },
    ReturnSpellFromDiscard,
    LookAtPrizes,
    DrawCards { amount: u32 },
    ReturnSpellToDeckTop,
    ProduceMana { target: OwnTarget },
    ProtectSourceFromOpposingMovement,
    ReadyOwnSummon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DamageTarget {
    DefendingMain,
    SelectedOpposingPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnTarget {
    Source,
    Selected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SwapSide {
    Own,
    Opposing,
}

#[derive(Debug)]
pub(crate) struct DamageAddition {
    pub(crate) amount: u32,
    pub(crate) condition: dto::Condition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectContext {
    Attack,
    Skill,
    Trigger,
    AttackSpell,
    SupportSpell,
    Enchantment,
}

pub(crate) fn parse(decoded: dto::Set) -> Result<Set, SetLoadError> {
    let dto::Set {
        schema_version: _,
        id,
        revision,
        name,
        cards,
    } = decoded;
    let code = StableCode::parse(id, StableKeyKind::Set, "id")?;
    require_positive(revision, "revision", SemanticRule::RevisionMustBePositive)?;
    require_name(&name, "name")?;
    if cards.is_empty() {
        return Err(rule_error("cards", SemanticRule::SetMustContainCards));
    }

    let mut seen = HashMap::new();
    let mut parsed = Vec::with_capacity(cards.len());
    for (index, card) in cards.into_iter().enumerate() {
        let path = format!("cards[{index}]");
        let card_code = StableCode::parse(
            card.code.clone(),
            StableKeyKind::Card,
            &format!("{path}.code"),
        )?;
        reject_duplicate(
            &mut seen,
            card_code.as_str(),
            &format!("{path}.code"),
            StableKeyKind::Card,
        )?;
        parsed.push(parse_card(card, card_code, &path)?);
    }
    Ok(Set {
        code,
        revision,
        name,
        cards: parsed,
    })
}

fn parse_card(raw: dto::Card, code: StableCode, path: &str) -> Result<Card, SetLoadError> {
    let dto::Card {
        code: _,
        name,
        text,
        kind,
        form,
        life,
        types,
        retreat,
        timing,
        persistence,
        cost,
        abilities,
        effects,
        modifiers,
    } = raw;
    require_name(&name, &format!("{path}.name"))?;

    let kind = match kind {
        dto::CardKind::Summon => {
            forbid_option(&timing, path, "timing")?;
            forbid_option(&persistence, path, "persistence")?;
            forbid_option(&cost, path, "cost")?;
            forbid_list(&effects, path, "effects")?;
            forbid_list(&modifiers, path, "modifiers")?;
            let form = required(form, path, "form")?;
            let life = required(life, path, "life")?;
            require_positive(life, &format!("{path}.life"), SemanticRule::SummonRequiresStatistics)?;
            let types = required(types, path, "types")?;
            let types = parse_mana_types(types, &format!("{path}.types"))?;
            let retreat = required(retreat, path, "retreat")?;
            let abilities = parse_abilities(abilities, path)?;
            let attacks = abilities
                .iter()
                .filter(|ability| matches!(ability.kind, AbilityKind::Attack { .. }))
                .count();
            if attacks != 1 {
                return Err(rule_error(
                    format!("{path}.abilities"),
                    SemanticRule::SummonRequiresOneAttack,
                ));
            }
            CardKind::Summon {
                form,
                life,
                types,
                retreat,
                abilities,
            }
        }
        dto::CardKind::Spell => {
            forbid_summon_fields(&form, &life, &types, &retreat, path)?;
            forbid_list(&abilities, path, "abilities")?;
            forbid_list(&modifiers, path, "modifiers")?;
            let timing = required(timing, path, "timing")?;
            let persistence = required(persistence, path, "persistence")?;
            if persistence != dto::Persistence::Discard {
                return Err(rule_error(
                    format!("{path}.persistence"),
                    SemanticRule::SpellShape,
                ));
            }
            let cost = required(cost, path, "cost")?;
            let context = match timing {
                dto::SpellTiming::Attack => EffectContext::AttackSpell,
                dto::SpellTiming::Support => EffectContext::SupportSpell,
            };
            let effects = parse_effects(effects, context, &format!("{path}.effects"))?;
            CardKind::Spell {
                timing,
                cost,
                effects,
            }
        }
        dto::CardKind::Enchantment => {
            forbid_summon_fields(&form, &life, &types, &retreat, path)?;
            forbid_list(&abilities, path, "abilities")?;
            let timing = required(timing, path, "timing")?;
            let persistence = required(persistence, path, "persistence")?;
            if timing != dto::SpellTiming::Support
                || persistence != dto::Persistence::Persistent
            {
                return Err(rule_error(path, SemanticRule::EnchantmentShape));
            }
            let cost = required(cost, path, "cost")?;
            let effects = parse_effects(effects, EffectContext::Enchantment, &format!("{path}.effects"))?;
            let modifiers = parse_modifiers(modifiers, &format!("{path}.modifiers"))?;
            CardKind::Enchantment {
                cost,
                effects,
                modifiers,
            }
        }
    };
    Ok(Card {
        code,
        name,
        text,
        kind,
    })
}

fn parse_abilities(raw: Vec<dto::Ability>, card_path: &str) -> Result<Vec<Ability>, SetLoadError> {
    if raw.is_empty() {
        return Err(rule_error(
            format!("{card_path}.abilities"),
            SemanticRule::SummonRequiresOneAttack,
        ));
    }
    let mut seen = HashMap::new();
    raw.into_iter()
        .enumerate()
        .map(|(index, ability)| {
            let path = format!("{card_path}.abilities[{index}]");
            let code = StableCode::parse(
                ability.code.clone(),
                StableKeyKind::Ability,
                &format!("{path}.code"),
            )?;
            reject_duplicate(
                &mut seen,
                code.as_str(),
                &format!("{path}.code"),
                StableKeyKind::Ability,
            )?;
            parse_ability(ability, code, &path)
        })
        .collect()
}

fn parse_ability(raw: dto::Ability, code: StableCode, path: &str) -> Result<Ability, SetLoadError> {
    let dto::Ability {
        code: _,
        name,
        text,
        kind,
        cost,
        event,
        response,
        effects,
        modifier,
    } = raw;
    let parsed = match kind {
        dto::AbilityKind::Attack | dto::AbilityKind::Skill => {
            forbid_option(&event, path, "event")?;
            forbid_option(&response, path, "response")?;
            forbid_option(&modifier, path, "modifier")?;
            let cost = required(cost, path, "cost")?;
            let context = if kind == dto::AbilityKind::Attack {
                EffectContext::Attack
            } else {
                EffectContext::Skill
            };
            let effects = parse_effects(effects, context, &format!("{path}.effects"))?;
            if kind == dto::AbilityKind::Attack {
                AbilityKind::Attack { cost, effects }
            } else {
                AbilityKind::Skill { cost, effects }
            }
        }
        dto::AbilityKind::Passive => {
            forbid_option(&cost, path, "cost")?;
            forbid_option(&event, path, "event")?;
            forbid_option(&response, path, "response")?;
            forbid_list(&effects, path, "effects")?;
            let modifier = required(modifier, path, "modifier")?;
            parse_modifier(modifier, &format!("{path}.modifier"))?;
            AbilityKind::Passive { modifier }
        }
        dto::AbilityKind::Trigger => {
            forbid_option(&cost, path, "cost")?;
            forbid_option(&modifier, path, "modifier")?;
            let event = required(event, path, "event")?;
            let response = required(response, path, "response")?;
            let effects = parse_effects(effects, EffectContext::Trigger, &format!("{path}.effects"))?;
            AbilityKind::Trigger {
                event,
                response,
                effects,
            }
        }
    };
    Ok(Ability {
        code,
        name,
        text,
        kind: parsed,
    })
}

fn parse_effects(
    raw: Vec<dto::Effect>,
    context: EffectContext,
    path: &str,
) -> Result<Vec<Effect>, SetLoadError> {
    if raw.is_empty() {
        return Err(rule_error(path, SemanticRule::AbilityShape));
    }
    raw.into_iter()
        .enumerate()
        .map(|(index, effect)| parse_effect(effect, context, &format!("{path}[{index}]")))
        .collect()
}

fn parse_effect(raw: dto::Effect, context: EffectContext, path: &str) -> Result<Effect, SetLoadError> {
    match raw {
        dto::Effect::Damage {
            target,
            base,
            constraints,
            additions,
        } => {
            require_positive(base, &format!("{path}.base"), SemanticRule::AmountMustBePositive)?;
            let target = match (context, target) {
                (EffectContext::Attack, dto::Target::DefendingMain) => DamageTarget::DefendingMain,
                (EffectContext::AttackSpell, dto::Target::SelectedOpposingPosition) => {
                    DamageTarget::SelectedOpposingPosition
                }
                _ => return Err(rule_error(format!("{path}.target"), SemanticRule::EffectTarget)),
            };
            reject_duplicate_constraints(&constraints, &format!("{path}.constraints"))?;
            let additions = additions
                .into_iter()
                .enumerate()
                .map(|(index, addition)| {
                    require_positive(
                        addition.amount,
                        &format!("{path}.additions[{index}].amount"),
                        SemanticRule::AmountMustBePositive,
                    )?;
                    Ok(DamageAddition {
                        amount: addition.amount,
                        condition: addition.condition,
                    })
                })
                .collect::<Result<Vec<_>, SetLoadError>>()?;
            Ok(Effect::Damage {
                target,
                base,
                constraints,
                additions,
            })
        }
        dto::Effect::Heal { target, amount } => {
            require_positive(amount, &format!("{path}.amount"), SemanticRule::AmountMustBePositive)?;
            let target = match (context, target) {
                (EffectContext::Skill | EffectContext::Trigger, dto::Target::Source) => OwnTarget::Source,
                (
                    EffectContext::SupportSpell | EffectContext::Enchantment,
                    dto::Target::SelectedOwnSummon,
                ) => OwnTarget::Selected,
                _ => return Err(rule_error(format!("{path}.target"), SemanticRule::EffectTarget)),
            };
            Ok(Effect::Heal { target, amount })
        }
        dto::Effect::MoveSummon { target, destination } => {
            if context == EffectContext::Skill
                && target == dto::Target::SelectedOwnBenchedSummon
                && destination == dto::Destination::EmptyOwnBench
            {
                Ok(Effect::MoveOwnBenchedToEmptyBench)
            } else {
                Err(rule_error(path, SemanticRule::EffectCombination))
            }
        }
        dto::Effect::SwapPositions { target } => match (context, target) {
            (EffectContext::Skill, dto::Target::OwnMainWithSelectedBench) => {
                Ok(Effect::SwapPositions { side: SwapSide::Own })
            }
            (EffectContext::Skill, dto::Target::OpposingMainWithSelectedBench) => {
                Ok(Effect::SwapPositions { side: SwapSide::Opposing })
            }
            _ => Err(rule_error(format!("{path}.target"), SemanticRule::EffectTarget)),
        },
        dto::Effect::BlockResponses { condition, response } => {
            if context == EffectContext::Attack && response == dto::ResponseBlock::AttackSpells {
                Ok(Effect::BlockAttackSpells { condition })
            } else {
                Err(rule_error(path, SemanticRule::EffectCombination))
            }
        }
        dto::Effect::ReturnSpellFromDiscard if context == EffectContext::Trigger => {
            Ok(Effect::ReturnSpellFromDiscard)
        }
        dto::Effect::LookAtPrizes if context == EffectContext::Skill => Ok(Effect::LookAtPrizes),
        dto::Effect::DrawCards { amount } if context == EffectContext::Skill || context == EffectContext::SupportSpell => {
            require_positive(amount, &format!("{path}.amount"), SemanticRule::AmountMustBePositive)?;
            Ok(Effect::DrawCards { amount })
        }
        dto::Effect::ReturnSpellToDeckTop if context == EffectContext::Skill => {
            Ok(Effect::ReturnSpellToDeckTop)
        }
        dto::Effect::ProduceMana { target } => match (context, target) {
            (EffectContext::Skill, dto::Target::SelectedOwnSummon) => {
                Ok(Effect::ProduceMana { target: OwnTarget::Selected })
            }
            (EffectContext::Trigger, dto::Target::Source) => {
                Ok(Effect::ProduceMana { target: OwnTarget::Source })
            }
            _ => Err(rule_error(format!("{path}.target"), SemanticRule::EffectTarget)),
        },
        dto::Effect::CannotBeMovedByOpponent { target, duration }
            if context == EffectContext::Skill
                && target == dto::Target::Source
                && duration == dto::Duration::UntilYourNextTurn =>
        {
            Ok(Effect::ProtectSourceFromOpposingMovement)
        }
        dto::Effect::ReadySummon { target }
            if context == EffectContext::SupportSpell && target == dto::Target::SelectedOwnSummon =>
        {
            Ok(Effect::ReadyOwnSummon)
        }
        _ => Err(rule_error(path, SemanticRule::EffectCombination)),
    }
}

fn parse_mana_types(raw: Vec<dto::ManaType>, path: &str) -> Result<Vec<dto::ManaType>, SetLoadError> {
    if raw.is_empty() {
        return Err(rule_error(path, SemanticRule::SummonRequiresStatistics));
    }
    let mut seen = HashSet::new();
    for (index, mana) in raw.iter().copied().enumerate() {
        let name = format!("{mana:?}");
        if !seen.insert(name.clone()) {
            return Err(semantic_error(
                format!("{path}[{index}]"),
                SetLoadCause::DuplicateValue {
                    value: name,
                    first_path: path.to_string(),
                },
            ));
        }
    }
    Ok(raw)
}

fn parse_modifiers(raw: Vec<dto::Modifier>, path: &str) -> Result<Vec<dto::Modifier>, SetLoadError> {
    raw.into_iter()
        .enumerate()
        .map(|(index, modifier)| {
            parse_modifier(modifier, &format!("{path}[{index}]"))?;
            Ok(modifier)
        })
        .collect()
}

fn parse_modifier(modifier: dto::Modifier, path: &str) -> Result<(), SetLoadError> {
    let amount = match modifier {
        dto::Modifier::OpposingRetreatCost { amount }
        | dto::Modifier::IncomingAttackDamageReduction { amount } => amount,
    };
    require_positive(amount, &format!("{path}.amount"), SemanticRule::AmountMustBePositive)?;
    if matches!(modifier, dto::Modifier::OpposingRetreatCost { .. }) && i32::try_from(amount).is_err() {
        return Err(rule_error(
            format!("{path}.amount"),
            SemanticRule::ModifierAmountOutOfRange,
        ));
    }
    Ok(())
}

fn reject_duplicate_constraints(raw: &[dto::DamageConstraint], path: &str) -> Result<(), SetLoadError> {
    let mut seen = HashMap::new();
    for (index, constraint) in raw.iter().copied().enumerate() {
        let value = format!("{constraint:?}");
        let item_path = format!("{path}[{index}]");
        if let Some(first_path) = seen.insert(value.clone(), item_path.clone()) {
            return Err(semantic_error(
                item_path,
                SetLoadCause::DuplicateValue { value, first_path },
            ));
        }
    }
    Ok(())
}

fn reject_duplicate(
    seen: &mut HashMap<String, String>,
    value: &str,
    path: &str,
    kind: StableKeyKind,
) -> Result<(), SetLoadError> {
    if let Some(first_path) = seen.insert(value.to_string(), path.to_string()) {
        Err(semantic_error(
            path,
            SetLoadCause::DuplicateStableKey {
                kind,
                value: value.to_string(),
                first_path,
            },
        ))
    } else {
        Ok(())
    }
}

fn forbid_summon_fields(
    form: &Option<dto::Form>,
    life: &Option<u32>,
    types: &Option<Vec<dto::ManaType>>,
    retreat: &Option<u32>,
    path: &str,
) -> Result<(), SetLoadError> {
    forbid_option(form, path, "form")?;
    forbid_option(life, path, "life")?;
    forbid_option(types, path, "types")?;
    forbid_option(retreat, path, "retreat")
}

fn required<T>(value: Option<T>, path: &str, field: &'static str) -> Result<T, SetLoadError> {
    value.ok_or_else(|| {
        semantic_error(
            format!("{path}.{field}"),
            SetLoadCause::MissingRequiredField { field },
        )
    })
}

fn forbid_option<T>(value: &Option<T>, path: &str, field: &'static str) -> Result<(), SetLoadError> {
    if value.is_some() {
        Err(semantic_error(
            format!("{path}.{field}"),
            SetLoadCause::ForbiddenField { field },
        ))
    } else {
        Ok(())
    }
}

fn forbid_list<T>(value: &[T], path: &str, field: &'static str) -> Result<(), SetLoadError> {
    if value.is_empty() {
        Ok(())
    } else {
        Err(semantic_error(
            format!("{path}.{field}"),
            SetLoadCause::ForbiddenField { field },
        ))
    }
}

fn require_name(value: &str, path: &str) -> Result<(), SetLoadError> {
    if value.trim().is_empty() {
        Err(rule_error(path, SemanticRule::NameMustNotBeEmpty))
    } else {
        Ok(())
    }
}

fn require_positive(value: u32, path: &str, rule: SemanticRule) -> Result<(), SetLoadError> {
    if value == 0 {
        Err(rule_error(path, rule))
    } else {
        Ok(())
    }
}

fn rule_error(path: impl Into<String>, rule: SemanticRule) -> SetLoadError {
    semantic_error(path, SetLoadCause::InvalidSemantics { rule })
}

fn semantic_error(path: impl Into<String>, cause: SetLoadCause) -> SetLoadError {
    SetLoadError::new(LoadPhase::Semantics, Some(1), path, cause)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use crate::document;

    fn decode(source: &str) -> dto::Set {
        document::decode(source.as_bytes()).expect("fixture is valid TOML")
    }

    fn valid_summon(card_code: &str, ability_code: &str) -> String {
        format!(r#"
schema_version = 1
id = "test-set"
revision = 1
name = "Test Set"

[[cards]]
code = "{card_code}"
name = "Test Summon"
kind = "summon"
form = "base"
life = 10
types = ["matter"]
retreat = 1

[[cards.abilities]]
code = "{ability_code}"
kind = "attack"
cost = []

[[cards.abilities.effects]]
kind = "damage"
target = "defending-main"
base = 10
"#)
    }

    #[test]
    fn stable_key_parser_accepts_the_precise_syntax() {
        for value in ["a", "set-path-1", "a1"] {
            assert!(StableCode::parse(value.to_string(), StableKeyKind::Card, "code").is_ok());
        }
    }

    #[test]
    fn stable_key_parser_rejects_invalid_syntax() {
        for value in ["", "Upper", "1-first", "trailing-", "two--hyphens", "under_score"] {
            let error = StableCode::parse(value.to_string(), StableKeyKind::Card, "cards[0].code")
                .unwrap_err();
            assert_eq!(error.path, "cards[0].code");
            assert!(matches!(error.cause, SetLoadCause::InvalidStableKey { .. }));
        }
    }

    #[test]
    fn set_parser_accepts_a_well_formed_summon() {
        let parsed = parse(decode(&valid_summon("test-card", "strike"))).unwrap();
        assert_eq!(parsed.code.as_str(), "test-set");
        assert_eq!(parsed.cards.len(), 1);
    }

    #[test]
    fn set_parser_rejects_duplicate_card_codes_with_both_paths() {
        let first = valid_summon("same-card", "strike");
        let second = first.replacen("[[cards]]", "[[cards]]", 1);
        let card_section = second.split("[[cards]]").nth(1).unwrap();
        let source = format!("{first}\n[[cards]]{card_section}");
        let error = parse(decode(&source)).unwrap_err();
        assert_eq!(error.path, "cards[1].code");
        assert!(matches!(
            error.cause,
            SetLoadCause::DuplicateStableKey {
                kind: StableKeyKind::Card,
                first_path,
                ..
            } if first_path == "cards[0].code"
        ));
    }

    #[test]
    fn summon_requires_its_statistics() {
        let source = valid_summon("test-card", "strike").replace("life = 10\n", "");
        let error = parse(decode(&source)).unwrap_err();
        assert_eq!(error.path, "cards[0].life");
        assert_eq!(
            error.cause,
            SetLoadCause::MissingRequiredField { field: "life" }
        );
    }

    #[test]
    fn ability_parser_rejects_a_damage_selector_for_the_wrong_role() {
        let source = valid_summon("test-card", "strike")
            .replace("kind = \"attack\"\ncost", "kind = \"skill\"\ncost");
        let error = parse(decode(&source)).unwrap_err();
        assert_eq!(error.path, "cards[0].abilities[0].effects[0].target");
        assert_eq!(
            error.cause,
            SetLoadCause::InvalidSemantics {
                rule: SemanticRule::EffectTarget,
            }
        );
    }

    #[test]
    fn effect_parser_rejects_duplicate_damage_constraints() {
        let source = valid_summon("test-card", "strike").replace(
            "base = 10",
            "base = 10\nconstraints = [\"unpreventable\", \"unpreventable\"]",
        );
        let error = parse(decode(&source)).unwrap_err();
        assert_eq!(error.path, "cards[0].abilities[0].effects[0].constraints[1]");
        assert!(matches!(error.cause, SetLoadCause::DuplicateValue { .. }));
    }
}
