use std::collections::{BTreeMap, HashMap};

use summoners_core::domain::{
    cards::{
        CardSet, Component, Cost, DamageAddition, DamageConstraint, DamageConstraints,
        DamageEffect, EffectCondition, EffectLeaf, Entity, EntityId, Form, Life, ManaTypes,
        Modifier, Name, ResponseBlock, RetreatCost, SpellTiming, Tags, TriggerEvent,
    },
    ids::ManaType,
};

use crate::{
    LoadPhase, LoadedSet, SemanticRule, SetLoadCause, SetLoadError, identity,
    v1::{dto, model},
};

struct AbilityConversion<'codes, 'ids> {
    set_code: &'codes model::StableCode,
    card_code: &'codes model::StableCode,
    card_index: usize,
    seen_ids: &'ids mut HashMap<EntityId, String>,
}

pub(crate) fn convert(set: model::Set) -> Result<LoadedSet, SetLoadError> {
    let model::Set {
        code,
        revision,
        name,
        cards,
    } = set;
    let set_id = identity::set_id(&code)?;
    let mut seen_ids = HashMap::from([(set_id, "id".to_string())]);
    let mut card_ids = BTreeMap::new();
    let mut entities = Vec::with_capacity(cards.len());

    for (card_index, card) in cards.into_iter().enumerate() {
        let code_path = format!("cards[{card_index}].code");
        let card_id = identity::card_id(&code, &card.code, &code_path)?;
        reject_duplicate_id(&mut seen_ids, card_id, &code_path)?;
        card_ids.insert(card.code.as_str().to_string(), card_id);
        entities.push(convert_card(
            &code,
            card,
            card_id,
            card_index,
            &mut seen_ids,
        )?);
    }

    Ok(LoadedSet {
        id: set_id,
        code: code.as_str().to_string(),
        revision,
        name,
        cards: CardSet::new(entities),
        card_ids,
    })
}

fn convert_card(
    set_code: &model::StableCode,
    card: model::Card,
    card_id: EntityId,
    card_index: usize,
    seen_ids: &mut HashMap<EntityId, String>,
) -> Result<Entity, SetLoadError> {
    let model::Card {
        code,
        name,
        text,
        kind,
    } = card;
    drop(text);
    let mut components = vec![Component::Name(Name(name))];
    match kind {
        model::CardKind::Summon {
            form,
            life,
            types,
            retreat,
            abilities,
        } => {
            components.push(Component::Form(convert_form(form)));
            components.push(Component::Life(Life(life)));
            components.push(Component::Produces(ManaTypes(
                types.into_iter().map(convert_mana_type).collect(),
            )));
            components.push(Component::RetreatCost(RetreatCost(retreat)));
            let mut context = AbilityConversion {
                set_code,
                card_code: &code,
                card_index,
                seen_ids,
            };
            for (ability_index, ability) in abilities.into_iter().enumerate() {
                components.push(convert_ability(ability, ability_index, &mut context)?);
            }
        }
        model::CardKind::Spell {
            timing,
            cost,
            effects,
        } => {
            components.push(Component::Tags(Tags(vec!["spell".to_string()])));
            components.push(Component::Timing(convert_timing(timing)));
            components.push(Component::Cost(convert_cost(&cost)));
            components.extend(
                effects
                    .into_iter()
                    .map(convert_effect)
                    .map(Component::Effect),
            );
        }
        model::CardKind::Enchantment {
            cost,
            effects,
            modifiers,
        } => {
            components.push(Component::Tags(Tags(vec!["enchantment".to_string()])));
            components.push(Component::Timing(SpellTiming::Support));
            components.push(Component::Cost(convert_cost(&cost)));
            components.extend(
                effects
                    .into_iter()
                    .map(convert_effect)
                    .map(Component::Effect),
            );
            for (index, modifier) in modifiers.into_iter().enumerate() {
                components.push(Component::Passive(convert_modifier(
                    modifier,
                    &format!("cards[{card_index}].modifiers[{index}]"),
                )?));
            }
            components.push(Component::Persistent);
        }
    }
    Ok(Entity {
        id: card_id,
        components,
    })
}

fn convert_ability(
    ability: model::Ability,
    ability_index: usize,
    context: &mut AbilityConversion<'_, '_>,
) -> Result<Component, SetLoadError> {
    let path = format!("cards[{}].abilities[{ability_index}]", context.card_index);
    let id = identity::ability_id(
        context.set_code,
        context.card_code,
        ability.role(),
        &ability.code,
        &format!("{path}.code"),
    )?;
    reject_duplicate_id(context.seen_ids, id, &format!("{path}.code"))?;
    let model::Ability {
        code: _,
        name,
        text,
        kind,
    } = ability;
    drop(text);

    match kind {
        model::AbilityKind::Attack { cost, effects } => Ok(Component::Attack(Entity {
            id,
            components: ability_components(name, Some(cost), effects),
        })),
        model::AbilityKind::Skill { cost, effects } => Ok(Component::Skill(Entity {
            id,
            components: ability_components(name, Some(cost), effects),
        })),
        model::AbilityKind::Passive { modifier } => Ok(Component::Passive(convert_modifier(
            modifier,
            &format!("{path}.modifier"),
        )?)),
        model::AbilityKind::Trigger {
            event,
            response,
            effects,
        } => {
            let mut components = ability_components(name, None, effects);
            components.push(Component::Event(convert_trigger_event(event)));
            if response == dto::ResponseMode::Respondable {
                components.push(Component::Respondable);
            }
            Ok(Component::Trigger(Entity { id, components }))
        }
    }
}

fn ability_components(
    name: String,
    cost: Option<Vec<dto::ManaSymbol>>,
    effects: Vec<model::Effect>,
) -> Vec<Component> {
    let mut components = Vec::new();
    if !name.is_empty() {
        components.push(Component::Name(Name(name)));
    }
    if let Some(cost) = cost {
        components.push(Component::Cost(convert_cost(&cost)));
    }
    components.extend(
        effects
            .into_iter()
            .map(convert_effect)
            .map(Component::Effect),
    );
    components
}

/// Convert selectors only after semantic parsing has paired them with a legal
/// effect family. The current core leaf vocabulary infers these selectors
/// from the resolving source and effect kind, so selector values do not yet
/// appear in the constructed leaf.
fn convert_effect(effect: model::Effect) -> EffectLeaf {
    match effect {
        model::Effect::Damage {
            target,
            base,
            constraints,
            additions,
        } => {
            match target {
                model::DamageTarget::DefendingMain
                | model::DamageTarget::SelectedOpposingPosition => {}
            }
            EffectLeaf::DealDamage(DamageEffect {
                base,
                constraints: convert_damage_constraints(constraints),
                additions: additions
                    .into_iter()
                    .map(|addition| DamageAddition {
                        amount: addition.amount,
                        condition: convert_condition(addition.condition),
                    })
                    .collect(),
            })
        }
        model::Effect::Heal { target, amount } => {
            match target {
                model::OwnTarget::Source | model::OwnTarget::Selected => {}
            }
            EffectLeaf::Heal { amount }
        }
        model::Effect::MoveOwnBenchedToEmptyBench => EffectLeaf::MoveSummon,
        model::Effect::SwapPositions {
            side: model::SwapSide::Own,
        } => EffectLeaf::SwapPositions,
        model::Effect::SwapPositions {
            side: model::SwapSide::Opposing,
        } => EffectLeaf::SwapOpposingPositions,
        model::Effect::BlockAttackSpells { condition } => EffectLeaf::BlockResponses {
            condition: convert_condition(condition),
            block: ResponseBlock::AttackSpells,
        },
        model::Effect::ReturnSpellFromDiscard => EffectLeaf::ReturnSpellFromDiscard,
        model::Effect::LookAtPrizes => EffectLeaf::LookAtPrizes,
        model::Effect::DrawCards { amount } => EffectLeaf::DrawCards { amount },
        model::Effect::ReturnSpellToDeckTop => EffectLeaf::ReturnSpellToDeckTop,
        model::Effect::ProduceMana { target } => {
            match target {
                model::OwnTarget::Source | model::OwnTarget::Selected => {}
            }
            EffectLeaf::ProduceMana
        }
        model::Effect::ProtectSourceFromOpposingMovement => EffectLeaf::CannotBeMovedByOpponent,
        model::Effect::ReadyOwnSummon => EffectLeaf::ReadySummon,
    }
}

fn convert_form(form: dto::Form) -> Form {
    match form {
        dto::Form::Base => Form::Base,
        dto::Form::Enhanced => Form::Enhanced,
        dto::Form::Elite => Form::Elite,
    }
}

fn convert_mana_type(mana: dto::ManaType) -> ManaType {
    match mana {
        dto::ManaType::Matter => ManaType::Matter,
        dto::ManaType::Mind => ManaType::Mind,
        dto::ManaType::Spirit => ManaType::Spirit,
    }
}

fn convert_cost(symbols: &[dto::ManaSymbol]) -> Cost {
    symbols.iter().fold(Cost::default(), |mut cost, symbol| {
        match symbol {
            dto::ManaSymbol::Matter => cost.matter += 1,
            dto::ManaSymbol::Mind => cost.mind += 1,
            dto::ManaSymbol::Spirit => cost.spirit += 1,
            dto::ManaSymbol::Generic => cost.generic += 1,
        }
        cost
    })
}

fn convert_timing(timing: dto::SpellTiming) -> SpellTiming {
    match timing {
        dto::SpellTiming::Support => SpellTiming::Support,
        dto::SpellTiming::Attack => SpellTiming::Attack,
    }
}

fn convert_trigger_event(event: dto::TriggerEvent) -> TriggerEvent {
    match event {
        dto::TriggerEvent::YourUpkeep => TriggerEvent::YourUpkeep,
        dto::TriggerEvent::EntersMain => TriggerEvent::EntersMain,
        dto::TriggerEvent::EntersBench => TriggerEvent::EntersBench,
        dto::TriggerEvent::LeavesMain => TriggerEvent::LeavesMain,
        dto::TriggerEvent::LeavesBench => TriggerEvent::LeavesBench,
        dto::TriggerEvent::AnySummonDestroyed => TriggerEvent::AnySummonDestroyed,
    }
}

fn convert_condition(condition: dto::Condition) -> EffectCondition {
    match condition {
        dto::Condition::DefenderEnteredMainThisTurn {
            player: dto::RelativePlayer::Controller,
        } => EffectCondition::DefenderEnteredMainThisTurn,
        dto::Condition::SpellPlayedThisTurn {
            player: dto::RelativePlayer::Controller,
        } => EffectCondition::SpellPlayedThisTurn,
    }
}

fn convert_damage_constraint(constraint: dto::DamageConstraint) -> DamageConstraint {
    match constraint {
        dto::DamageConstraint::Unincreasable => DamageConstraint::Unincreasable,
        dto::DamageConstraint::Unpreventable => DamageConstraint::Unpreventable,
    }
}

fn convert_damage_constraints(constraints: Vec<dto::DamageConstraint>) -> DamageConstraints {
    constraints.into_iter().map(convert_damage_constraint).fold(
        DamageConstraints::new(),
        |mut converted, constraint| {
            converted.insert(constraint);
            converted
        },
    )
}

fn convert_modifier(modifier: dto::Modifier, path: &str) -> Result<Modifier, SetLoadError> {
    match modifier {
        dto::Modifier::OpposingRetreatCost { amount } => i32::try_from(amount)
            .map(Modifier::OpposingRetreatCostDelta)
            .map_err(|_| {
                SetLoadError::new(
                    LoadPhase::Conversion,
                    Some(1),
                    format!("{path}.amount"),
                    SetLoadCause::InvalidSemantics {
                        rule: SemanticRule::ModifierAmountOutOfRange,
                    },
                )
            }),
        dto::Modifier::IncomingAttackDamageReduction { amount } => {
            Ok(Modifier::IncomingAttackDamageReduction(amount))
        }
    }
}

fn reject_duplicate_id(
    seen: &mut HashMap<EntityId, String>,
    id: EntityId,
    path: &str,
) -> Result<(), SetLoadError> {
    if let Some(first_path) = seen.insert(id, path.to_string()) {
        Err(SetLoadError::new(
            LoadPhase::Identity,
            Some(1),
            path,
            SetLoadCause::DuplicateGeneratedId { id, first_path },
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::document;

    const SUMMON: &str = r#"
schema_version = 1
id = "test-set"
revision = 2
name = "Test Set"

[[cards]]
code = "test-summon"
name = "Test Summon"
kind = "summon"
form = "base"
life = 50
types = ["matter", "mind"]
retreat = 2

[[cards.abilities]]
code = "strike"
name = "Strike"
kind = "attack"
cost = ["matter", "generic"]

[[cards.abilities.effects]]
kind = "damage"
target = "defending-main"
base = 20
"#;

    #[test]
    fn cost_conversion_counts_each_symbol() {
        let converted = convert_cost(&[
            dto::ManaSymbol::Matter,
            dto::ManaSymbol::Mind,
            dto::ManaSymbol::Spirit,
            dto::ManaSymbol::Generic,
            dto::ManaSymbol::Generic,
        ]);
        assert_eq!(
            converted,
            Cost {
                matter: 1,
                mind: 1,
                spirit: 1,
                generic: 2,
            }
        );
    }

    #[test]
    fn closed_leaf_conversions_cover_every_simple_variant() {
        assert_eq!(convert_form(dto::Form::Elite), Form::Elite);
        assert_eq!(convert_mana_type(dto::ManaType::Spirit), ManaType::Spirit);
        assert_eq!(
            convert_timing(dto::SpellTiming::Attack),
            SpellTiming::Attack
        );
        assert_eq!(
            convert_trigger_event(dto::TriggerEvent::LeavesBench),
            TriggerEvent::LeavesBench
        );
        assert_eq!(
            convert_damage_constraint(dto::DamageConstraint::Unpreventable),
            DamageConstraint::Unpreventable
        );
    }

    #[test]
    fn valid_model_converts_to_a_core_card_set() {
        let decoded = document::decode(SUMMON.as_bytes()).expect("document decodes");
        let validated = model::parse(decoded).expect("document is semantic");
        let loaded = convert(validated).expect("model converts");
        assert_eq!(loaded.code(), "test-set");
        assert_eq!(loaded.revision(), 2);
        assert_eq!(loaded.cards().entities().len(), 1);
        let entity = &loaded.cards().entities()[0];
        assert_eq!(entity.get::<Life>(), Some(&Life(50)));
        assert_eq!(
            entity.all::<summoners_core::domain::cards::Attack>().len(),
            1
        );
    }

    #[test]
    fn duplicate_generated_id_reports_both_paths_before_card_set_construction() {
        let id =
            EntityId::parse("00000000-0000-0000-0000-000000000001").expect("fixture id is valid");
        let mut seen = HashMap::from([(id, "cards[0].code".to_string())]);
        let error = reject_duplicate_id(&mut seen, id, "cards[1].code")
            .expect_err("duplicate id must fail");
        assert_eq!(error.phase, LoadPhase::Identity);
        assert_eq!(error.path, "cards[1].code");
        assert_eq!(
            error.cause,
            SetLoadCause::DuplicateGeneratedId {
                id,
                first_path: "cards[0].code".to_string(),
            }
        );
    }
}
