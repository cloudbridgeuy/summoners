//! `GameEvent`: past-tense facts describing what changed. Events observe
//! the resulting `GameState`; they never drive it (decision 3). `apply`
//! returns the full ordered batch produced by one accepted action, including
//! every step the resolution loop drained automatically.

use crate::domain::cards::{
    DamageConstraint, DamageConstraints, EffectSource, EntityId, TriggerEvent,
};
use crate::domain::ids::{BenchSlot, CardInstanceId, ManaType, PlayerId, Position};
use crate::domain::state::{LossReason, ManaSource, StackItem};

/// The printed effect that initiated one Damage calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Attack {
        controller: PlayerId,
        position: Position,
        ability: EntityId,
    },
    Spell {
        controller: PlayerId,
        card: CardInstanceId,
        definition: EntityId,
    },
    Skill {
        controller: PlayerId,
        position: Position,
        ability: EntityId,
    },
    Trigger {
        controller: PlayerId,
        position: Position,
        ability: EntityId,
    },
}

impl DamageSource {
    #[must_use]
    pub const fn controller(self) -> PlayerId {
        match self {
            DamageSource::Attack { controller, .. }
            | DamageSource::Spell { controller, .. }
            | DamageSource::Skill { controller, .. }
            | DamageSource::Trigger { controller, .. } => controller,
        }
    }

    #[must_use]
    pub const fn origin(self) -> DamageOrigin {
        let ability = match self {
            DamageSource::Attack { ability, .. }
            | DamageSource::Skill { ability, .. }
            | DamageSource::Trigger { ability, .. } => ability,
            DamageSource::Spell { definition, .. } => definition,
        };
        DamageOrigin::PrintedAbility(ability)
    }
}

impl From<EffectSource> for DamageSource {
    fn from(source: EffectSource) -> Self {
        match source {
            EffectSource::Attack {
                controller,
                position,
                ability,
            } => DamageSource::Attack {
                controller,
                position,
                ability,
            },
            EffectSource::Spell {
                controller,
                card,
                definition,
            } => DamageSource::Spell {
                controller,
                card,
                definition,
            },
            EffectSource::Skill {
                controller,
                position,
                ability,
            } => DamageSource::Skill {
                controller,
                position,
                ability,
            },
            EffectSource::Trigger {
                controller,
                position,
                ability,
            } => DamageSource::Trigger {
                controller,
                position,
                ability,
            },
        }
    }
}

/// One battlefield position, qualified by the player whose board contains
/// it so the target stays unambiguous outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattlefieldTarget {
    pub controller: PlayerId,
    pub position: Position,
}

/// Source and target repeated on every Damage event line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageContext {
    pub source: DamageSource,
    pub target: BattlefieldTarget,
}

/// The fixed stages used by the current Damage pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageStage {
    Addition,
    PersistentReduction,
    Clamp,
    Commit,
}

/// The numeric operation represented by one Damage adjustment line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageOperation {
    Add(u32),
    Reduce(u32),
    ClampToZero,
}

/// The printed ability or persistent card instance that supplied a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageOrigin {
    PrintedAbility(EntityId),
    PersistentCard(CardInstanceId),
}

/// One past-tense fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    /// Rules §9: a turn began for `player`.
    TurnBegan { player: PlayerId },
    /// Rules §10 step 1: every Summon `player` controls became Ready.
    SummonsReadied {
        player: PlayerId,
        positions: Vec<Position>,
    },
    /// Rules §10 step 2.
    CardDrawn {
        player: PlayerId,
        card: CardInstanceId,
    },
    /// Rules §10 step 3, §11.
    ManaProduced {
        player: PlayerId,
        source: ManaSource,
        mana_type: ManaType,
    },
    /// Rules §17.
    SummonPlayed {
        player: PlayerId,
        card: CardInstanceId,
        slot: BenchSlot,
    },
    /// Rules §18–20.
    SummonUpgraded {
        player: PlayerId,
        card: CardInstanceId,
        position: Position,
    },
    /// Rules §34.
    SpellCast {
        player: PlayerId,
        card: CardInstanceId,
        targets: Vec<Position>,
    },
    /// Rules §15.
    SkillActivated {
        player: PlayerId,
        position: Position,
        ability: EntityId,
    },
    /// Rules §29–31.
    AttackDeclared { player: PlayerId, target: Position },
    /// Rules §32–33.
    PriorityPassed { player: PlayerId },
    /// Rules §35.
    StackItemResolved { item: StackItem },
    /// The immutable input to one Damage calculation.
    DamageCalculationStarted {
        context: DamageContext,
        base: u32,
        constraints: DamageConstraints,
    },
    /// One adjustment that changed, or deliberately retained, the running
    /// Damage total.
    DamageAdjustmentApplied {
        context: DamageContext,
        stage: DamageStage,
        operation: DamageOperation,
        origin: DamageOrigin,
        input: u32,
        output: u32,
    },
    /// One adjustment a semantic constraint blocked.
    DamageAdjustmentSkipped {
        context: DamageContext,
        stage: DamageStage,
        operation: DamageOperation,
        origin: DamageOrigin,
        input: u32,
        constraint: DamageConstraint,
    },
    /// Damage before and after the pipeline's single commit.
    DamageApplied {
        context: DamageContext,
        amount: u32,
        before: u32,
        after: u32,
    },
    /// Rules §22.
    Healed { position: Position, amount: u32 },
    /// Rules §23.
    SummonDestroyed { position: Position, owner: PlayerId },
    /// Rules §24 step 3, §25.
    PrizeRecovered {
        player: PlayerId,
        card: CardInstanceId,
    },
    /// Rules §25, §44: `player` looked at their own face-down Prizes — the
    /// Griefsinger's `Foresee`. Names which cards, the same as `CardDrawn`
    /// and `PrizeRecovered` already do: this crate models no hidden
    /// information (`GameState` is one fully visible value, decision 3), so
    /// naming the cards leaks nothing a reader could not already see by
    /// reading `PlayerState::prizes` directly. This event's only job is
    /// marking that the look happened.
    PrizesViewed {
        player: PlayerId,
        prizes: Vec<CardInstanceId>,
    },
    /// Rules §24 step 4.
    SummonPromoted { player: PlayerId, from: BenchSlot },
    /// Rules §26: a normal Retreat exchanged Main and one Bench slot.
    SummonsSwapped { player: PlayerId, main: BenchSlot },
    /// Rules §28, §36–39. Names the ability that fired by its `EntityId`,
    /// the same way `SkillActivated` names the Skill it activated — a card
    /// may print more than one Trigger matching the same `TriggerEvent`, so
    /// the event must say which one this is.
    TriggerFired {
        controller: PlayerId,
        position: Position,
        event: TriggerEvent,
        ability: EntityId,
    },
    /// Rules §7.
    CoinConverted {
        player: PlayerId,
        mana_type: ManaType,
    },
    /// Rules §10 (payment policy, decision 14): Mana left the bank to pay a
    /// cost.
    ManaDeducted {
        player: PlayerId,
        mana_type: ManaType,
        amount: u32,
    },
    /// Rules §2: the match ended.
    GameEnded {
        winner: PlayerId,
        reason: LossReason,
    },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn every_damage_contract_variant_constructs() {
        let ability = EntityId::parse(&"0".repeat(32)).expect("valid probe id");
        let sources = [
            DamageSource::Attack {
                controller: PlayerId::One,
                position: Position::Main,
                ability,
            },
            DamageSource::Spell {
                controller: PlayerId::One,
                card: CardInstanceId(1),
                definition: ability,
            },
            DamageSource::Skill {
                controller: PlayerId::One,
                position: Position::Main,
                ability,
            },
            DamageSource::Trigger {
                controller: PlayerId::One,
                position: Position::Main,
                ability,
            },
        ];
        let context = DamageContext {
            source: sources[0],
            target: BattlefieldTarget {
                controller: PlayerId::Two,
                position: Position::Main,
            },
        };
        let stages = [
            DamageStage::Addition,
            DamageStage::PersistentReduction,
            DamageStage::Clamp,
            DamageStage::Commit,
        ];
        let operations = [
            DamageOperation::Add(20),
            DamageOperation::Reduce(10),
            DamageOperation::ClampToZero,
        ];
        let origins = [
            DamageOrigin::PrintedAbility(ability),
            DamageOrigin::PersistentCard(CardInstanceId(1)),
        ];

        assert_eq!(stages.len(), 4);
        assert_eq!(operations.len(), 3);
        assert_eq!(origins.len(), 2);
        assert!(
            sources
                .iter()
                .all(|source| source.controller() == PlayerId::One)
        );
        assert!(
            sources
                .iter()
                .all(|source| source.origin() == DamageOrigin::PrintedAbility(ability))
        );
        assert_eq!(sources.len(), 4);
        assert_eq!(context.target.controller, PlayerId::Two);
    }

    #[test]
    fn every_game_event_variant_constructs() {
        let events = vec![
            GameEvent::TurnBegan {
                player: PlayerId::One,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: CardInstanceId(1),
            },
            GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Player,
                mana_type: ManaType::Matter,
            },
            GameEvent::SummonPlayed {
                player: PlayerId::One,
                card: CardInstanceId(1),
                slot: BenchSlot::First,
            },
            GameEvent::SummonUpgraded {
                player: PlayerId::One,
                card: CardInstanceId(1),
                position: Position::Main,
            },
            GameEvent::SpellCast {
                player: PlayerId::One,
                card: CardInstanceId(1),
                targets: vec![Position::Main],
            },
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                ability: EntityId::parse(&"0".repeat(32)).expect("valid probe id"),
            },
            GameEvent::AttackDeclared {
                player: PlayerId::One,
                target: Position::Main,
            },
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            GameEvent::DamageApplied {
                context: DamageContext {
                    source: DamageSource::Spell {
                        controller: PlayerId::One,
                        card: CardInstanceId(1),
                        definition: EntityId::parse(&"0".repeat(32)).expect("valid probe id"),
                    },
                    target: BattlefieldTarget {
                        controller: PlayerId::Two,
                        position: Position::Main,
                    },
                },
                amount: 10,
                before: 0,
                after: 10,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 10,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::One,
            },
            GameEvent::PrizeRecovered {
                player: PlayerId::One,
                card: CardInstanceId(1),
            },
            GameEvent::PrizesViewed {
                player: PlayerId::One,
                prizes: vec![CardInstanceId(1), CardInstanceId(2)],
            },
            GameEvent::SummonPromoted {
                player: PlayerId::One,
                from: BenchSlot::First,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::YourUpkeep,
                ability: EntityId::parse(&"0".repeat(32)).expect("valid probe id"),
            },
            GameEvent::CoinConverted {
                player: PlayerId::Two,
                mana_type: ManaType::Spirit,
            },
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: LossReason::ThirdMainLoss,
            },
        ];
        assert_eq!(events.len(), 22);
    }
}
