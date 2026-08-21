use super::*;
use crate::domain::cards::{
    Attack, CardSet, Component, DamageAddition, DamageConstraint, DamageConstraints, DamageEffect,
    EffectCondition, EffectLeaf,
};
use crate::domain::events::{
    BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource, DamageStage,
};
use std::sync::Arc;

fn attack_ability(state: &GameState, player: PlayerId) -> crate::domain::cards::EntityId {
    let summon = state
        .players
        .get(player)
        .main
        .as_ref()
        .expect("main summon");
    state
        .cards
        .get(summon.chain.top().def)
        .and_then(|entity| entity.get::<Attack>())
        .map(|attack| attack.id)
        .expect("main summon prints an attack")
}

fn set_main(state: &mut GameState, player: PlayerId, instance: u32, card: &'static str) {
    state.players.get_mut(player).main = Some(SummonInstance {
        chain: UpgradeChain::new(card_ref(instance, card), vec![]),
        damage: 0,
        readiness: Readiness::Ready,
        owner: player,
        controller: player,
        duration_markers: vec![],
        turn: crate::domain::state::SummonTurnRecord::fresh(),
    });
}

fn add_two_wards(state: &mut GameState, player: PlayerId) {
    state.players.get_mut(player).enchantments = vec![
        card_ref(100, "standing-ward"),
        card_ref(101, "standing-ward"),
    ];
}

fn replace_attack_damage(state: &mut GameState, card: &'static str, damage: DamageEffect) {
    let card_id = fixtures::id(card);
    let mut entities = fixtures::entities();
    let entity = entities
        .iter_mut()
        .find(|entity| entity.id == card_id)
        .expect("fixture card");
    let attack = entity
        .components
        .iter_mut()
        .find_map(|component| match component {
            Component::Attack(attack) => Some(attack),
            _ => None,
        })
        .expect("fixture attack");
    let effect = attack
        .components
        .iter_mut()
        .find_map(|component| match component {
            Component::Effect(EffectLeaf::DealDamage(effect)) => Some(effect),
            _ => None,
        })
        .expect("fixture Damage effect");
    *effect = damage;
    state.cards = Arc::new(CardSet::new(entities));
}

fn resolve_attack(state: &GameState) -> ActionOutcome {
    let declared = apply(
        state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("attack declaration is legal");
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("defender holds priority");
    apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("attacker closes the window")
}

fn attack_context(ability: crate::domain::cards::EntityId) -> DamageContext {
    DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability,
        },
        target: BattlefieldTarget {
            controller: PlayerId::Two,
            position: Position::Main,
        },
    }
}

#[test]
fn normal_attack_emits_the_exact_flat_trace_and_commits_once() {
    let state = base_state();
    let ability = attack_ability(&state, PlayerId::One);
    let context = attack_context(ability);

    let resolved = resolve_attack(&state);

    assert_eq!(
        resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            GameEvent::DamageCalculationStarted {
                context,
                base: 10,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(ability),
                input: 10,
                output: 10,
            },
            GameEvent::DamageApplied {
                context,
                amount: 10,
                before: 0,
                after: 10,
            },
        ]
    );
    assert_eq!(
        resolved
            .state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        10
    );
    assert_eq!(
        state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        0,
        "apply leaves the caller's state unchanged"
    );
}

#[test]
fn conditional_attack_combines_before_two_ordered_ward_reductions() {
    let mut state = base_state();
    set_main(&mut state, PlayerId::One, 20, "warden-of-set-paths");
    set_main(&mut state, PlayerId::Two, 21, "old-sow-of-the-barrow");
    state
        .players
        .get_mut(PlayerId::Two)
        .main
        .as_mut()
        .expect("main")
        .turn
        .main_entry = Some(crate::domain::state::EnteredMain);
    state.players.get_mut(PlayerId::One).mana.matter = 3;
    add_two_wards(&mut state, PlayerId::Two);
    let ability = attack_ability(&state, PlayerId::One);
    let context = attack_context(ability);

    let resolved = resolve_attack(&state);
    let damage_events = &resolved.events[2..];

    assert_eq!(
        damage_events,
        &[
            GameEvent::DamageCalculationStarted {
                context,
                base: 50,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::Addition,
                operation: DamageOperation::Add(30),
                origin: DamageOrigin::PrintedAbility(ability),
                input: 50,
                output: 80,
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(CardInstanceId(100)),
                input: 80,
                output: 70,
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(CardInstanceId(101)),
                input: 70,
                output: 60,
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(ability),
                input: 60,
                output: 60,
            },
            GameEvent::DamageApplied {
                context,
                amount: 60,
                before: 0,
                after: 60,
            },
        ]
    );
    assert_eq!(
        resolved
            .state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        60
    );
}

#[test]
fn unpreventable_attack_emits_one_skipped_line_per_ward() {
    let mut state = base_state();
    set_main(&mut state, PlayerId::One, 30, "old-sow-of-the-barrow");
    set_main(&mut state, PlayerId::Two, 31, "old-sow-of-the-barrow");
    state.players.get_mut(PlayerId::One).mana.matter = 3;
    state.players.get_mut(PlayerId::One).mana.spirit = 1;
    add_two_wards(&mut state, PlayerId::Two);
    let ability = attack_ability(&state, PlayerId::One);
    let context = attack_context(ability);

    let resolved = resolve_attack(&state);
    let skipped: Vec<&GameEvent> = resolved
        .events
        .iter()
        .filter(|event| matches!(event, GameEvent::DamageAdjustmentSkipped { .. }))
        .collect();

    assert_eq!(skipped.len(), 2);
    assert!(matches!(
        skipped[0],
        GameEvent::DamageAdjustmentSkipped {
            context: line_context,
            stage: DamageStage::PersistentReduction,
            operation: DamageOperation::Reduce(10),
            origin: DamageOrigin::PersistentCard(CardInstanceId(100)),
            input: 70,
            constraint: DamageConstraint::Unpreventable,
        } if *line_context == context
    ));
    assert!(matches!(
        skipped[1],
        GameEvent::DamageAdjustmentSkipped {
            context: line_context,
            stage: DamageStage::PersistentReduction,
            operation: DamageOperation::Reduce(10),
            origin: DamageOrigin::PersistentCard(CardInstanceId(101)),
            input: 70,
            constraint: DamageConstraint::Unpreventable,
        } if *line_context == context
    ));
    assert!(matches!(
        resolved.events.last(),
        Some(GameEvent::DamageApplied {
            context: line_context,
            amount: 70,
            before: 0,
            after: 70,
        }) if *line_context == context
    ));
}

#[test]
fn two_wards_clamp_five_damage_at_zero_through_apply() {
    let mut state = base_state();
    replace_attack_damage(
        &mut state,
        "quarry-whelp",
        DamageEffect {
            base: 5,
            constraints: DamageConstraints::new(),
            additions: vec![],
        },
    );
    add_two_wards(&mut state, PlayerId::Two);
    let ability = attack_ability(&state, PlayerId::One);
    let context = attack_context(ability);

    let resolved = resolve_attack(&state);

    assert_eq!(
        &resolved.events[2..],
        &[
            GameEvent::DamageCalculationStarted {
                context,
                base: 5,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(CardInstanceId(100)),
                input: 5,
                output: 0,
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(CardInstanceId(101)),
                input: 0,
                output: 0,
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(ability),
                input: 0,
                output: 0,
            },
            GameEvent::DamageApplied {
                context,
                amount: 0,
                before: 0,
                after: 0,
            },
        ]
    );
}

#[test]
fn unincreasable_attack_skips_a_true_addition_through_apply() {
    let mut state = base_state();
    set_main(&mut state, PlayerId::One, 40, "warden-of-set-paths");
    set_main(&mut state, PlayerId::Two, 41, "old-sow-of-the-barrow");
    replace_attack_damage(
        &mut state,
        "warden-of-set-paths",
        DamageEffect {
            base: 50,
            constraints: DamageConstraints::from([DamageConstraint::Unincreasable]),
            additions: vec![DamageAddition {
                amount: 30,
                condition: EffectCondition::DefenderEnteredMainThisTurn,
            }],
        },
    );
    state.players.get_mut(PlayerId::One).mana.matter = 3;
    state
        .players
        .get_mut(PlayerId::Two)
        .main
        .as_mut()
        .expect("main")
        .turn
        .main_entry = Some(crate::domain::state::EnteredMain);
    let ability = attack_ability(&state, PlayerId::One);
    let context = attack_context(ability);

    let resolved = resolve_attack(&state);

    assert_eq!(
        &resolved.events[2..],
        &[
            GameEvent::DamageCalculationStarted {
                context,
                base: 50,
                constraints: DamageConstraints::from([DamageConstraint::Unincreasable]),
            },
            GameEvent::DamageAdjustmentSkipped {
                context,
                stage: DamageStage::Addition,
                operation: DamageOperation::Add(30),
                origin: DamageOrigin::PrintedAbility(ability),
                input: 50,
                constraint: DamageConstraint::Unincreasable,
            },
            GameEvent::DamageAdjustmentApplied {
                context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(ability),
                input: 50,
                output: 50,
            },
            GameEvent::DamageApplied {
                context,
                amount: 50,
                before: 0,
                after: 50,
            },
        ]
    );
}
