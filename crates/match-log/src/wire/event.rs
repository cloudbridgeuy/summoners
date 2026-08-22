use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use summoners_core::domain::{
    cards::{DamageConstraint, DamageConstraints},
    events::{
        BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource, DamageStage,
        GameEvent,
    },
    ids::{CardInstanceId, Position},
    state::StackItem,
};

use crate::error::WireConversionError;

use super::{
    BenchSlotV1, DamageConstraintsV1, EntityIdV1, LossReasonV1, ManaSourceV1, ManaTypeV1,
    PlayerIdV1, PositionV1, StackItemV1, TriggerEventV1, parse_entity_id,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DamageConstraintV1 {
    Unincreasable,
    Unpreventable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DamageStageV1 {
    Addition,
    PersistentReduction,
    Clamp,
    Commit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DamageOperationV1 {
    Add { amount: u32 },
    Reduce { amount: u32 },
    ClampToZero,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DamageSourceV1 {
    Attack {
        controller: PlayerIdV1,
        position: PositionV1,
        ability: EntityIdV1,
    },
    Spell {
        controller: PlayerIdV1,
        card: u32,
        definition: EntityIdV1,
    },
    Skill {
        controller: PlayerIdV1,
        position: PositionV1,
        ability: EntityIdV1,
    },
    Trigger {
        controller: PlayerIdV1,
        position: PositionV1,
        ability: EntityIdV1,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DamageOriginV1 {
    PrintedAbility { ability: EntityIdV1 },
    PersistentCard { card: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BattlefieldTargetV1 {
    pub controller: PlayerIdV1,
    pub position: PositionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DamageContextV1 {
    pub source: DamageSourceV1,
    pub target: BattlefieldTargetV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventV1 {
    TurnBegan {
        player: PlayerIdV1,
    },
    SummonsReadied {
        player: PlayerIdV1,
        positions: Vec<PositionV1>,
    },
    CardDrawn {
        player: PlayerIdV1,
        card: u32,
    },
    ManaProduced {
        player: PlayerIdV1,
        source: ManaSourceV1,
        mana_type: ManaTypeV1,
    },
    SummonPlayed {
        player: PlayerIdV1,
        card: u32,
        slot: BenchSlotV1,
    },
    SummonUpgraded {
        player: PlayerIdV1,
        card: u32,
        position: PositionV1,
    },
    SpellCast {
        player: PlayerIdV1,
        card: u32,
        targets: Vec<PositionV1>,
    },
    SkillActivated {
        player: PlayerIdV1,
        position: PositionV1,
        ability: EntityIdV1,
    },
    AttackDeclared {
        player: PlayerIdV1,
        target: PositionV1,
    },
    PriorityPassed {
        player: PlayerIdV1,
    },
    StackItemResolved {
        item: StackItemV1,
    },
    DamageCalculationStarted {
        context: DamageContextV1,
        base: u32,
        constraints: DamageConstraintsV1,
    },
    DamageAdjustmentApplied {
        context: DamageContextV1,
        stage: DamageStageV1,
        operation: DamageOperationV1,
        origin: DamageOriginV1,
        input: u32,
        output: u32,
    },
    DamageAdjustmentSkipped {
        context: DamageContextV1,
        stage: DamageStageV1,
        operation: DamageOperationV1,
        origin: DamageOriginV1,
        input: u32,
        constraint: DamageConstraintV1,
    },
    DamageApplied {
        context: DamageContextV1,
        amount: u32,
        before: u32,
        after: u32,
    },
    Healed {
        position: PositionV1,
        amount: u32,
    },
    SummonDestroyed {
        position: PositionV1,
        owner: PlayerIdV1,
    },
    PrizeRecovered {
        player: PlayerIdV1,
        card: u32,
    },
    PrizesViewed {
        player: PlayerIdV1,
        prizes: Vec<u32>,
    },
    SummonPromoted {
        player: PlayerIdV1,
        from: BenchSlotV1,
    },
    SummonsSwapped {
        player: PlayerIdV1,
        main: BenchSlotV1,
    },
    TriggerFired {
        controller: PlayerIdV1,
        position: PositionV1,
        event: TriggerEventV1,
        ability: EntityIdV1,
    },
    CoinConverted {
        player: PlayerIdV1,
        mana_type: ManaTypeV1,
    },
    ManaDeducted {
        player: PlayerIdV1,
        mana_type: ManaTypeV1,
        amount: u32,
    },
    GameEnded {
        winner: PlayerIdV1,
        reason: LossReasonV1,
    },
}

impl From<DamageConstraint> for DamageConstraintV1 {
    fn from(value: DamageConstraint) -> Self {
        match value {
            DamageConstraint::Unincreasable => Self::Unincreasable,
            DamageConstraint::Unpreventable => Self::Unpreventable,
        }
    }
}

impl From<DamageConstraintV1> for DamageConstraint {
    fn from(value: DamageConstraintV1) -> Self {
        match value {
            DamageConstraintV1::Unincreasable => Self::Unincreasable,
            DamageConstraintV1::Unpreventable => Self::Unpreventable,
        }
    }
}

impl From<DamageStage> for DamageStageV1 {
    fn from(value: DamageStage) -> Self {
        match value {
            DamageStage::Addition => Self::Addition,
            DamageStage::PersistentReduction => Self::PersistentReduction,
            DamageStage::Clamp => Self::Clamp,
            DamageStage::Commit => Self::Commit,
        }
    }
}

impl From<DamageStageV1> for DamageStage {
    fn from(value: DamageStageV1) -> Self {
        match value {
            DamageStageV1::Addition => Self::Addition,
            DamageStageV1::PersistentReduction => Self::PersistentReduction,
            DamageStageV1::Clamp => Self::Clamp,
            DamageStageV1::Commit => Self::Commit,
        }
    }
}

impl From<DamageOperation> for DamageOperationV1 {
    fn from(value: DamageOperation) -> Self {
        match value {
            DamageOperation::Add(amount) => Self::Add { amount },
            DamageOperation::Reduce(amount) => Self::Reduce { amount },
            DamageOperation::ClampToZero => Self::ClampToZero,
        }
    }
}

impl From<DamageOperationV1> for DamageOperation {
    fn from(value: DamageOperationV1) -> Self {
        match value {
            DamageOperationV1::Add { amount } => Self::Add(amount),
            DamageOperationV1::Reduce { amount } => Self::Reduce(amount),
            DamageOperationV1::ClampToZero => Self::ClampToZero,
        }
    }
}

impl From<DamageSource> for DamageSourceV1 {
    fn from(value: DamageSource) -> Self {
        match value {
            DamageSource::Attack {
                controller,
                position,
                ability,
            } => Self::Attack {
                controller: controller.into(),
                position: position.into(),
                ability: ability.into(),
            },
            DamageSource::Spell {
                controller,
                card,
                definition,
            } => Self::Spell {
                controller: controller.into(),
                card: card.0,
                definition: definition.into(),
            },
            DamageSource::Skill {
                controller,
                position,
                ability,
            } => Self::Skill {
                controller: controller.into(),
                position: position.into(),
                ability: ability.into(),
            },
            DamageSource::Trigger {
                controller,
                position,
                ability,
            } => Self::Trigger {
                controller: controller.into(),
                position: position.into(),
                ability: ability.into(),
            },
        }
    }
}

impl TryFrom<DamageSourceV1> for DamageSource {
    type Error = WireConversionError;

    fn try_from(value: DamageSourceV1) -> Result<Self, Self::Error> {
        match value {
            DamageSourceV1::Attack {
                controller,
                position,
                ability,
            } => Ok(Self::Attack {
                controller: controller.into(),
                position: position.into(),
                ability: parse_entity_id(ability)?,
            }),
            DamageSourceV1::Spell {
                controller,
                card,
                definition,
            } => Ok(Self::Spell {
                controller: controller.into(),
                card: CardInstanceId(card),
                definition: parse_entity_id(definition)?,
            }),
            DamageSourceV1::Skill {
                controller,
                position,
                ability,
            } => Ok(Self::Skill {
                controller: controller.into(),
                position: position.into(),
                ability: parse_entity_id(ability)?,
            }),
            DamageSourceV1::Trigger {
                controller,
                position,
                ability,
            } => Ok(Self::Trigger {
                controller: controller.into(),
                position: position.into(),
                ability: parse_entity_id(ability)?,
            }),
        }
    }
}

impl From<DamageOrigin> for DamageOriginV1 {
    fn from(value: DamageOrigin) -> Self {
        match value {
            DamageOrigin::PrintedAbility(ability) => Self::PrintedAbility {
                ability: ability.into(),
            },
            DamageOrigin::PersistentCard(card) => Self::PersistentCard { card: card.0 },
        }
    }
}

impl TryFrom<DamageOriginV1> for DamageOrigin {
    type Error = WireConversionError;

    fn try_from(value: DamageOriginV1) -> Result<Self, Self::Error> {
        match value {
            DamageOriginV1::PrintedAbility { ability } => {
                Ok(Self::PrintedAbility(parse_entity_id(ability)?))
            }
            DamageOriginV1::PersistentCard { card } => {
                Ok(Self::PersistentCard(CardInstanceId(card)))
            }
        }
    }
}

impl From<BattlefieldTarget> for BattlefieldTargetV1 {
    fn from(value: BattlefieldTarget) -> Self {
        Self {
            controller: value.controller.into(),
            position: value.position.into(),
        }
    }
}

impl From<BattlefieldTargetV1> for BattlefieldTarget {
    fn from(value: BattlefieldTargetV1) -> Self {
        Self {
            controller: value.controller.into(),
            position: value.position.into(),
        }
    }
}

impl From<DamageContext> for DamageContextV1 {
    fn from(value: DamageContext) -> Self {
        Self {
            source: value.source.into(),
            target: value.target.into(),
        }
    }
}

impl TryFrom<DamageContextV1> for DamageContext {
    type Error = WireConversionError;

    fn try_from(value: DamageContextV1) -> Result<Self, Self::Error> {
        Ok(Self {
            source: value.source.try_into()?,
            target: value.target.into(),
        })
    }
}

impl From<&GameEvent> for EventV1 {
    fn from(value: &GameEvent) -> Self {
        match value {
            GameEvent::TurnBegan { player } => Self::TurnBegan {
                player: (*player).into(),
            },
            GameEvent::SummonsReadied { player, positions } => Self::SummonsReadied {
                player: (*player).into(),
                positions: positions.iter().copied().map(PositionV1::from).collect(),
            },
            GameEvent::CardDrawn { player, card } => Self::CardDrawn {
                player: (*player).into(),
                card: card.0,
            },
            GameEvent::ManaProduced {
                player,
                source,
                mana_type,
            } => Self::ManaProduced {
                player: (*player).into(),
                source: (*source).into(),
                mana_type: (*mana_type).into(),
            },
            GameEvent::SummonPlayed { player, card, slot } => Self::SummonPlayed {
                player: (*player).into(),
                card: card.0,
                slot: (*slot).into(),
            },
            GameEvent::SummonUpgraded {
                player,
                card,
                position,
            } => Self::SummonUpgraded {
                player: (*player).into(),
                card: card.0,
                position: (*position).into(),
            },
            GameEvent::SpellCast {
                player,
                card,
                targets,
            } => Self::SpellCast {
                player: (*player).into(),
                card: card.0,
                targets: targets.iter().copied().map(PositionV1::from).collect(),
            },
            GameEvent::SkillActivated {
                player,
                position,
                ability,
            } => Self::SkillActivated {
                player: (*player).into(),
                position: (*position).into(),
                ability: (*ability).into(),
            },
            GameEvent::AttackDeclared { player, target } => Self::AttackDeclared {
                player: (*player).into(),
                target: (*target).into(),
            },
            GameEvent::PriorityPassed { player } => Self::PriorityPassed {
                player: (*player).into(),
            },
            GameEvent::StackItemResolved { item } => Self::StackItemResolved {
                item: StackItemV1::from(item),
            },
            GameEvent::DamageCalculationStarted {
                context,
                base,
                constraints,
            } => Self::DamageCalculationStarted {
                context: (*context).into(),
                base: *base,
                constraints: constraints_v1(*constraints),
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage,
                operation,
                origin,
                input,
                output,
            } => Self::DamageAdjustmentApplied {
                context: (*context).into(),
                stage: (*stage).into(),
                operation: (*operation).into(),
                origin: (*origin).into(),
                input: *input,
                output: *output,
            },
            GameEvent::DamageAdjustmentSkipped {
                context,
                stage,
                operation,
                origin,
                input,
                constraint,
            } => Self::DamageAdjustmentSkipped {
                context: (*context).into(),
                stage: (*stage).into(),
                operation: (*operation).into(),
                origin: (*origin).into(),
                input: *input,
                constraint: (*constraint).into(),
            },
            GameEvent::DamageApplied {
                context,
                amount,
                before,
                after,
            } => Self::DamageApplied {
                context: (*context).into(),
                amount: *amount,
                before: *before,
                after: *after,
            },
            GameEvent::Healed { position, amount } => Self::Healed {
                position: (*position).into(),
                amount: *amount,
            },
            GameEvent::SummonDestroyed { position, owner } => Self::SummonDestroyed {
                position: (*position).into(),
                owner: (*owner).into(),
            },
            GameEvent::PrizeRecovered { player, card } => Self::PrizeRecovered {
                player: (*player).into(),
                card: card.0,
            },
            GameEvent::PrizesViewed { player, prizes } => Self::PrizesViewed {
                player: (*player).into(),
                prizes: prizes.iter().map(|card| card.0).collect(),
            },
            GameEvent::SummonPromoted { player, from } => Self::SummonPromoted {
                player: (*player).into(),
                from: (*from).into(),
            },
            GameEvent::SummonsSwapped { player, main } => Self::SummonsSwapped {
                player: (*player).into(),
                main: (*main).into(),
            },
            GameEvent::TriggerFired {
                controller,
                position,
                event,
                ability,
            } => Self::TriggerFired {
                controller: (*controller).into(),
                position: (*position).into(),
                event: (*event).into(),
                ability: (*ability).into(),
            },
            GameEvent::CoinConverted { player, mana_type } => Self::CoinConverted {
                player: (*player).into(),
                mana_type: (*mana_type).into(),
            },
            GameEvent::ManaDeducted {
                player,
                mana_type,
                amount,
            } => Self::ManaDeducted {
                player: (*player).into(),
                mana_type: (*mana_type).into(),
                amount: *amount,
            },
            GameEvent::GameEnded { winner, reason } => Self::GameEnded {
                winner: (*winner).into(),
                reason: (*reason).into(),
            },
        }
    }
}

impl TryFrom<EventV1> for GameEvent {
    type Error = WireConversionError;

    fn try_from(value: EventV1) -> Result<Self, Self::Error> {
        match value {
            EventV1::TurnBegan { player } => Ok(Self::TurnBegan {
                player: player.into(),
            }),
            EventV1::SummonsReadied { player, positions } => Ok(Self::SummonsReadied {
                player: player.into(),
                positions: positions.into_iter().map(Position::from).collect(),
            }),
            EventV1::CardDrawn { player, card } => Ok(Self::CardDrawn {
                player: player.into(),
                card: CardInstanceId(card),
            }),
            EventV1::ManaProduced {
                player,
                source,
                mana_type,
            } => Ok(Self::ManaProduced {
                player: player.into(),
                source: source.into(),
                mana_type: mana_type.into(),
            }),
            EventV1::SummonPlayed { player, card, slot } => Ok(Self::SummonPlayed {
                player: player.into(),
                card: CardInstanceId(card),
                slot: slot.into(),
            }),
            EventV1::SummonUpgraded {
                player,
                card,
                position,
            } => Ok(Self::SummonUpgraded {
                player: player.into(),
                card: CardInstanceId(card),
                position: position.into(),
            }),
            EventV1::SpellCast {
                player,
                card,
                targets,
            } => Ok(Self::SpellCast {
                player: player.into(),
                card: CardInstanceId(card),
                targets: targets.into_iter().map(Position::from).collect(),
            }),
            EventV1::SkillActivated {
                player,
                position,
                ability,
            } => Ok(Self::SkillActivated {
                player: player.into(),
                position: position.into(),
                ability: parse_entity_id(ability)?,
            }),
            EventV1::AttackDeclared { player, target } => Ok(Self::AttackDeclared {
                player: player.into(),
                target: target.into(),
            }),
            EventV1::PriorityPassed { player } => Ok(Self::PriorityPassed {
                player: player.into(),
            }),
            EventV1::StackItemResolved { item } => Ok(Self::StackItemResolved {
                item: StackItem::try_from(item)?,
            }),
            EventV1::DamageCalculationStarted {
                context,
                base,
                constraints,
            } => Ok(Self::DamageCalculationStarted {
                context: context.try_into()?,
                base,
                constraints: constraints_core(constraints),
            }),
            EventV1::DamageAdjustmentApplied {
                context,
                stage,
                operation,
                origin,
                input,
                output,
            } => Ok(Self::DamageAdjustmentApplied {
                context: context.try_into()?,
                stage: stage.into(),
                operation: operation.into(),
                origin: origin.try_into()?,
                input,
                output,
            }),
            EventV1::DamageAdjustmentSkipped {
                context,
                stage,
                operation,
                origin,
                input,
                constraint,
            } => Ok(Self::DamageAdjustmentSkipped {
                context: context.try_into()?,
                stage: stage.into(),
                operation: operation.into(),
                origin: origin.try_into()?,
                input,
                constraint: constraint.into(),
            }),
            EventV1::DamageApplied {
                context,
                amount,
                before,
                after,
            } => Ok(Self::DamageApplied {
                context: context.try_into()?,
                amount,
                before,
                after,
            }),
            EventV1::Healed { position, amount } => Ok(Self::Healed {
                position: position.into(),
                amount,
            }),
            EventV1::SummonDestroyed { position, owner } => Ok(Self::SummonDestroyed {
                position: position.into(),
                owner: owner.into(),
            }),
            EventV1::PrizeRecovered { player, card } => Ok(Self::PrizeRecovered {
                player: player.into(),
                card: CardInstanceId(card),
            }),
            EventV1::PrizesViewed { player, prizes } => Ok(Self::PrizesViewed {
                player: player.into(),
                prizes: prizes.into_iter().map(CardInstanceId).collect(),
            }),
            EventV1::SummonPromoted { player, from } => Ok(Self::SummonPromoted {
                player: player.into(),
                from: from.into(),
            }),
            EventV1::SummonsSwapped { player, main } => Ok(Self::SummonsSwapped {
                player: player.into(),
                main: main.into(),
            }),
            EventV1::TriggerFired {
                controller,
                position,
                event,
                ability,
            } => Ok(Self::TriggerFired {
                controller: controller.into(),
                position: position.into(),
                event: event.into(),
                ability: parse_entity_id(ability)?,
            }),
            EventV1::CoinConverted { player, mana_type } => Ok(Self::CoinConverted {
                player: player.into(),
                mana_type: mana_type.into(),
            }),
            EventV1::ManaDeducted {
                player,
                mana_type,
                amount,
            } => Ok(Self::ManaDeducted {
                player: player.into(),
                mana_type: mana_type.into(),
                amount,
            }),
            EventV1::GameEnded { winner, reason } => Ok(Self::GameEnded {
                winner: winner.into(),
                reason: reason.into(),
            }),
        }
    }
}

fn constraints_v1(value: DamageConstraints) -> DamageConstraintsV1 {
    DamageConstraintsV1 {
        unincreasable: value.contains(DamageConstraint::Unincreasable),
        unpreventable: value.contains(DamageConstraint::Unpreventable),
    }
}

fn constraints_core(value: DamageConstraintsV1) -> DamageConstraints {
    let mut constraints = DamageConstraints::new();
    if value.unincreasable {
        constraints.insert(DamageConstraint::Unincreasable);
    }
    if value.unpreventable {
        constraints.insert(DamageConstraint::Unpreventable);
    }
    constraints
}

#[cfg(test)]
mod tests;
