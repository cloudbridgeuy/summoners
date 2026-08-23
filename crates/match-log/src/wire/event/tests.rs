#![allow(clippy::expect_used)]

use summoners_core::domain::{
    cards::{DamageConstraint, DamageConstraints, EntityId, TriggerEvent},
    events::{
        BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource, DamageStage,
        GameEvent,
    },
    ids::{BenchSlot, CardInstanceId, ManaType, PlayerId, Position},
    state::{LossReason, ManaSource, StackItem},
};

use super::*;

fn id(byte: &str) -> EntityId {
    EntityId::parse(&byte.repeat(16)).expect("valid test entity ID")
}

fn context() -> DamageContext {
    DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability: id("01"),
        },
        target: BattlefieldTarget {
            controller: PlayerId::Two,
            position: Position::Bench(BenchSlot::Second),
        },
    }
}

#[test]
fn every_event_variant_round_trips() {
    let events = vec![
        GameEvent::TurnBegan {
            player: PlayerId::One,
        },
        GameEvent::SummonsReadied {
            player: PlayerId::Two,
            positions: vec![Position::Main, Position::Bench(BenchSlot::First)],
        },
        GameEvent::CardDrawn {
            player: PlayerId::One,
            card: CardInstanceId(1),
        },
        GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Main),
            mana_type: ManaType::Mind,
        },
        GameEvent::SummonPlayed {
            player: PlayerId::One,
            card: CardInstanceId(2),
            slot: BenchSlot::First,
        },
        GameEvent::SummonUpgraded {
            player: PlayerId::Two,
            card: CardInstanceId(3),
            position: Position::Main,
        },
        GameEvent::SpellCast {
            player: PlayerId::One,
            card: CardInstanceId(4),
            targets: vec![Position::Main],
        },
        GameEvent::SkillActivated {
            player: PlayerId::One,
            position: Position::Main,
            ability: id("02"),
        },
        GameEvent::AttackDeclared {
            player: PlayerId::One,
            target: Position::Main,
        },
        GameEvent::PriorityPassed {
            player: PlayerId::Two,
        },
        GameEvent::StackItemResolved {
            item: StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            },
        },
        GameEvent::DamageCalculationStarted {
            context: context(),
            base: 30,
            constraints: DamageConstraints::from([
                DamageConstraint::Unincreasable,
                DamageConstraint::Unpreventable,
            ]),
        },
        GameEvent::DamageAdjustmentApplied {
            context: context(),
            stage: DamageStage::Addition,
            operation: DamageOperation::Add(10),
            origin: DamageOrigin::PrintedAbility(id("03")),
            input: 30,
            output: 40,
        },
        GameEvent::DamageAdjustmentSkipped {
            context: context(),
            stage: DamageStage::PersistentReduction,
            operation: DamageOperation::Reduce(10),
            origin: DamageOrigin::PersistentCard(CardInstanceId(8)),
            input: 40,
            constraint: DamageConstraint::Unpreventable,
        },
        GameEvent::DamageApplied {
            context: context(),
            amount: 40,
            before: 0,
            after: 40,
        },
        GameEvent::Healed {
            position: Position::Main,
            amount: 10,
        },
        GameEvent::SummonDestroyed {
            position: Position::Main,
            owner: PlayerId::Two,
        },
        GameEvent::PrizeRecovered {
            player: PlayerId::One,
            card: CardInstanceId(9),
        },
        GameEvent::PrizesViewed {
            player: PlayerId::Two,
            prizes: vec![CardInstanceId(10), CardInstanceId(11)],
        },
        GameEvent::SummonPromoted {
            player: PlayerId::One,
            from: BenchSlot::Third,
        },
        GameEvent::SummonsSwapped {
            player: PlayerId::Two,
            main: BenchSlot::Second,
        },
        GameEvent::TriggerFired {
            controller: PlayerId::One,
            position: Position::Main,
            event: TriggerEvent::EntersMain,
            ability: id("04"),
        },
        GameEvent::CoinConverted {
            player: PlayerId::Two,
            mana_type: ManaType::Spirit,
        },
        GameEvent::ManaDeducted {
            player: PlayerId::One,
            mana_type: ManaType::Matter,
            amount: 2,
        },
        GameEvent::GameEnded {
            winner: PlayerId::One,
            reason: LossReason::EmptyDeckDraw,
        },
    ];

    for event in events {
        let rebuilt = GameEvent::try_from(EventV1::from(&event)).expect("valid wire event");
        assert_eq!(rebuilt, event);
    }
}

#[test]
fn all_damage_source_variants_round_trip() {
    let sources = [
        DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability: id("05"),
        },
        DamageSource::Spell {
            controller: PlayerId::Two,
            card: CardInstanceId(12),
            definition: id("06"),
        },
        DamageSource::Skill {
            controller: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: id("07"),
        },
        DamageSource::Trigger {
            controller: PlayerId::Two,
            position: Position::Main,
            ability: id("08"),
        },
    ];

    for source in sources {
        assert_eq!(
            DamageSource::try_from(DamageSourceV1::from(source)).expect("valid source"),
            source
        );
    }
}

#[test]
fn damage_constraint_helpers_round_trip_both_flags() {
    let constraints = DamageConstraints::from([
        DamageConstraint::Unincreasable,
        DamageConstraint::Unpreventable,
    ]);

    assert_eq!(constraints_core(constraints_v1(constraints)), constraints);
}

#[test]
fn every_damage_stage_operation_origin_and_constraint_round_trips() {
    for stage in [
        DamageStage::Addition,
        DamageStage::PersistentReduction,
        DamageStage::Clamp,
        DamageStage::Commit,
    ] {
        assert_eq!(DamageStage::from(DamageStageV1::from(stage)), stage);
    }
    for operation in [
        DamageOperation::Add(1),
        DamageOperation::Reduce(2),
        DamageOperation::ClampToZero,
    ] {
        assert_eq!(
            DamageOperation::from(DamageOperationV1::from(operation)),
            operation
        );
    }
    for origin in [
        DamageOrigin::PrintedAbility(id("09")),
        DamageOrigin::PersistentCard(CardInstanceId(13)),
    ] {
        assert_eq!(
            DamageOrigin::try_from(DamageOriginV1::from(origin)).expect("valid origin"),
            origin
        );
    }
    for constraint in [
        DamageConstraint::Unincreasable,
        DamageConstraint::Unpreventable,
    ] {
        assert_eq!(
            DamageConstraint::from(DamageConstraintV1::from(constraint)),
            constraint
        );
    }
}

#[test]
fn invalid_event_entity_id_is_a_typed_conversion_error() {
    let wire = EventV1::SkillActivated {
        player: PlayerIdV1::One,
        position: PositionV1::Main,
        ability: EntityIdV1("invalid".to_string()),
    };

    assert_eq!(
        GameEvent::try_from(wire),
        Err(WireConversionError::InvalidEntityId {
            value: "invalid".to_string()
        })
    );
}
