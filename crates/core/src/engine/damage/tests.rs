#![allow(clippy::expect_used)]

use super::*;
use crate::domain::cards::{Attack, DamageAddition, DamageConstraints, EntityId, fixtures};
use crate::domain::events::{BattlefieldTarget, DamageSource};
use crate::domain::ids::CardInstanceId;
use crate::domain::state::{
    CardRef, GameStatus, ManaBank, PerPlayer, Phase, TurnState, UpgradeChain,
};
use std::collections::VecDeque;

fn summon(owner: PlayerId) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(if owner == PlayerId::One { 1 } else { 2 }),
                def: fixtures::id("quarry-whelp"),
            },
            vec![],
        ),
        damage: 0,
        ready: true,
        owner,
        controller: owner,
        duration_markers: vec![],
        played_this_turn: false,
        upgraded_this_turn: false,
        entered_main_this_turn: false,
    }
}

fn player(owner: PlayerId) -> PlayerState {
    PlayerState {
        main: Some(summon(owner)),
        bench: [None, None, None],
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        has_coin: false,
        enchantments: vec![],
    }
}

fn state() -> GameState {
    GameState {
        players: PerPlayer::new(player(PlayerId::One), player(PlayerId::Two)),
        turn: TurnState {
            active_player: PlayerId::One,
            phase: Phase::Main,
            window: None,
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: false,
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: VecDeque::new(),
        pending: None,
        status: GameStatus::Playing,
        cards: fixtures::card_set(),
    }
}

fn attack_ability() -> EntityId {
    fixtures::card_set()
        .get(fixtures::id("quarry-whelp"))
        .and_then(|entity| entity.get::<Attack>())
        .map(|attack| attack.id)
        .expect("quarry-whelp prints an attack")
}

fn source(kind: &str) -> DamageSource {
    let ability = attack_ability();
    match kind {
        "attack" => DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability,
        },
        "spell" => DamageSource::Spell {
            controller: PlayerId::One,
            card: CardInstanceId(9),
            definition: fixtures::id("ember-lance"),
        },
        "skill" => DamageSource::Skill {
            controller: PlayerId::One,
            position: Position::Main,
            ability,
        },
        "trigger" => DamageSource::Trigger {
            controller: PlayerId::One,
            position: Position::Main,
            ability,
        },
        _ => panic!("unknown source kind"),
    }
}

fn intent(source: DamageSource, effect: DamageEffect) -> DamageIntent {
    DamageIntent {
        context: DamageContext {
            source,
            target: BattlefieldTarget {
                controller: PlayerId::Two,
                position: Position::Main,
            },
        },
        effect,
    }
}

fn effect(base: u32) -> DamageEffect {
    DamageEffect {
        base,
        constraints: DamageConstraints::new(),
        additions: vec![],
    }
}

fn add_two_wards(state: &mut GameState) {
    state.players.get_mut(PlayerId::Two).enchantments = vec![
        CardRef {
            instance: CardInstanceId(10),
            def: fixtures::id("standing-ward"),
        },
        CardRef {
            instance: CardInstanceId(11),
            def: fixtures::id("standing-ward"),
        },
    ];
}

#[test]
fn evaluate_applies_a_true_conditional_addition_before_two_wards() {
    let mut state = state();
    state.turn.spell_played_this_turn = true;
    add_two_wards(&mut state);
    let intent = intent(
        source("attack"),
        DamageEffect {
            additions: vec![DamageAddition {
                amount: 30,
                condition: EffectCondition::SpellPlayedThisTurn,
            }],
            ..effect(50)
        },
    );

    let resolution = evaluate(&state, &intent);

    assert_eq!(resolution.amount, 60);
    assert_eq!(resolution.lines.len(), 4);
    assert!(matches!(
        resolution.lines[0],
        DamageLine::Applied {
            stage: DamageStage::Addition,
            input: 50,
            output: 80,
            ..
        }
    ));
    assert!(matches!(
        resolution.lines[1],
        DamageLine::Applied {
            stage: DamageStage::PersistentReduction,
            input: 80,
            output: 70,
            ..
        }
    ));
    assert!(matches!(
        resolution.lines[2],
        DamageLine::Applied {
            stage: DamageStage::PersistentReduction,
            input: 70,
            output: 60,
            ..
        }
    ));
}

#[test]
fn evaluate_omits_a_false_conditional_addition() {
    let state = state();
    let intent = intent(
        source("attack"),
        DamageEffect {
            additions: vec![DamageAddition {
                amount: 30,
                condition: EffectCondition::SpellPlayedThisTurn,
            }],
            ..effect(50)
        },
    );

    let resolution = evaluate(&state, &intent);

    assert_eq!(resolution.amount, 50);
    assert_eq!(resolution.lines.len(), 1);
    assert!(matches!(
        resolution.lines[0],
        DamageLine::Applied {
            stage: DamageStage::Clamp,
            input: 50,
            output: 50,
            ..
        }
    ));
}

#[test]
fn evaluate_skips_addition_when_damage_is_unincreasable() {
    let mut state = state();
    state.turn.spell_played_this_turn = true;
    let intent = intent(
        source("attack"),
        DamageEffect {
            constraints: DamageConstraints::from([DamageConstraint::Unincreasable]),
            additions: vec![DamageAddition {
                amount: 30,
                condition: EffectCondition::SpellPlayedThisTurn,
            }],
            ..effect(70)
        },
    );

    let resolution = evaluate(&state, &intent);

    assert_eq!(resolution.amount, 70);
    assert!(matches!(
        resolution.lines[0],
        DamageLine::Skipped {
            stage: DamageStage::Addition,
            input: 70,
            constraint: DamageConstraint::Unincreasable,
            ..
        }
    ));
}

#[test]
fn evaluate_skips_each_ward_when_damage_is_unpreventable() {
    let mut state = state();
    add_two_wards(&mut state);
    let intent = intent(
        source("attack"),
        DamageEffect {
            constraints: DamageConstraints::from([DamageConstraint::Unpreventable]),
            ..effect(70)
        },
    );

    let resolution = evaluate(&state, &intent);

    assert_eq!(resolution.amount, 70);
    assert_eq!(
        resolution
            .lines
            .iter()
            .filter(|line| matches!(line, DamageLine::Skipped { .. }))
            .count(),
        2
    );
}

#[test]
fn evaluate_clamps_each_reduction_at_zero() {
    let mut state = state();
    add_two_wards(&mut state);

    let resolution = evaluate(&state, &intent(source("attack"), effect(5)));

    assert_eq!(resolution.amount, 0);
    assert!(matches!(
        resolution.lines[1],
        DamageLine::Applied {
            operation: DamageOperation::Reduce(10),
            input: 0,
            output: 0,
            ..
        }
    ));
}

#[test]
fn evaluate_does_not_collect_wards_for_spell_skill_or_trigger_damage() {
    let mut state = state();
    add_two_wards(&mut state);

    for kind in ["spell", "skill", "trigger"] {
        let resolution = evaluate(&state, &intent(source(kind), effect(20)));
        assert_eq!(resolution.amount, 20);
        assert_eq!(resolution.lines.len(), 1);
    }
}

#[test]
fn condition_holds_reads_each_supported_condition() {
    let mut state = state();
    assert!(!condition_holds(
        &state,
        PlayerId::One,
        &[Position::Main],
        EffectCondition::SpellPlayedThisTurn,
    ));
    state.turn.spell_played_this_turn = true;
    assert!(condition_holds(
        &state,
        PlayerId::One,
        &[Position::Main],
        EffectCondition::SpellPlayedThisTurn,
    ));
    state
        .players
        .get_mut(PlayerId::Two)
        .main
        .as_mut()
        .expect("main")
        .entered_main_this_turn = true;
    assert!(condition_holds(
        &state,
        PlayerId::One,
        &[Position::Main],
        EffectCondition::DefenderEnteredMainThisTurn,
    ));
}

#[test]
fn commit_mutates_the_target_once_and_emits_the_flat_trace() {
    let state = state();
    let resolution = evaluate(&state, &intent(source("attack"), effect(10)));

    let (next, events) = commit(&state, &resolution);

    assert_eq!(
        next.players
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
        0
    );
    assert_eq!(
        next.work,
        VecDeque::from([WorkItem::DestructionCheck(Position::Main)])
    );
    assert!(matches!(
        events[0],
        GameEvent::DamageCalculationStarted { .. }
    ));
    assert!(matches!(
        events[1],
        GameEvent::DamageAdjustmentApplied {
            stage: DamageStage::Clamp,
            ..
        }
    ));
    assert!(matches!(
        events[2],
        GameEvent::DamageApplied {
            amount: 10,
            before: 0,
            after: 10,
            ..
        }
    ));
}

#[test]
fn commit_is_a_silent_miss_when_the_target_is_empty() {
    let mut state = state();
    state.players.get_mut(PlayerId::Two).main = None;
    let resolution = evaluate(&state, &intent(source("attack"), effect(10)));

    let (next, events) = commit(&state, &resolution);

    assert_eq!(next, state);
    assert!(events.is_empty());
}
