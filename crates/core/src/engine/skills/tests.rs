//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.
#![allow(clippy::expect_used)]

use super::*;
use crate::domain::actions::GameAction;
use crate::domain::cards::fixtures;
use crate::domain::cards::{CardSet, Component, DamageConstraints, DamageEffect, Entity};
use crate::domain::events::{DamageSource, GameEvent};
use crate::domain::ids::{BenchSlot, CardInstanceId};
use crate::domain::state::{
    CardRef, GameStatus, ManaBank, ManaSource, MovementStep, PerPlayer, TurnState, UpgradeChain,
    WorkItem,
};
use std::collections::VecDeque;
use std::sync::Arc;

fn summon_with_def(owner: PlayerId, def: EntityId, ready: bool) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def,
            },
            vec![],
        ),
        damage: 0,
        ready,
        owner,
        controller: owner,
        duration_markers: vec![],
        played_this_turn: false,
        upgraded_this_turn: false,
        entered_main_this_turn: false,
    }
}

fn summon(owner: PlayerId, def: &'static str, ready: bool) -> SummonInstance {
    summon_with_def(owner, fixtures::id(def), ready)
}

fn empty_player_with_main(main: SummonInstance) -> PlayerState {
    PlayerState {
        main: Some(main),
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

fn empty_player(owner: PlayerId, def: &'static str, ready: bool) -> PlayerState {
    empty_player_with_main(summon(owner, def, ready))
}

fn base_state_with_cards(cards: Arc<CardSet>, def: EntityId, ready: bool) -> GameState {
    GameState {
        players: PerPlayer::new(
            empty_player_with_main(summon_with_def(PlayerId::One, def, ready)),
            empty_player(PlayerId::Two, "quarry-whelp", true),
        ),
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
        cards,
    }
}

fn base_state(def: &'static str, ready: bool) -> GameState {
    base_state_with_cards(fixtures::card_set(), fixtures::id(def), ready)
}

/// An id no fixture card in `fixtures::entities()` uses.
fn probe_id(byte: u8) -> EntityId {
    EntityId::parse(&format!("{byte:02x}").repeat(16)).expect("valid probe id")
}

/// Every fixture entity plus `extra`, wrapped in a fresh `CardSet`/`Arc` of
/// its own — the shared fixture `Arc` (`fixtures::card_set()`) is never
/// touched.
fn cards_with(extra: Vec<Entity>) -> Arc<CardSet> {
    let mut entities = fixtures::entities();
    entities.extend(extra);
    Arc::new(CardSet::new(entities))
}

#[test]
fn damage_skill_retains_its_exact_source_and_ignores_wards() {
    let card_id = probe_id(0xd1);
    let ability_id = probe_id(0xd2);
    let skill = Entity {
        id: ability_id,
        components: vec![Component::Effect(EffectLeaf::DealDamage(DamageEffect {
            base: 20,
            constraints: DamageConstraints::new(),
            additions: vec![],
        }))],
    };
    let card = Entity {
        id: card_id,
        components: vec![Component::Skill(skill)],
    };
    let mut state = base_state_with_cards(cards_with(vec![card]), card_id, true);
    state.players.get_mut(PlayerId::Two).enchantments = vec![
        CardRef {
            instance: CardInstanceId(90),
            def: fixtures::id("standing-ward"),
        },
        CardRef {
            instance: CardInstanceId(91),
            def: fixtures::id("standing-ward"),
        },
    ];

    let outcome = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: ability_id,
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("free damage skill is legal");

    assert!(matches!(
        outcome.events.get(1),
        Some(GameEvent::DamageCalculationStarted {
            context,
            base: 20,
            ..
        }) if context.source == (DamageSource::Skill {
            controller: PlayerId::One,
            position: Position::Main,
            ability: ability_id,
        })
    ));
    assert_eq!(
        outcome
            .state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        20
    );
}

// --- a fixture fact, read straight off the container -----------------

/// Quarry Scout prints one Skill, a Generic-1 `MoveSummon`. Pinned here,
/// as a direct read off the container, so this printed fact stays covered
/// by a test independent of whichever engine rule happens to consume it.
#[test]
fn quarry_scouts_skill_prints_a_generic_cost_and_a_move_summon_effect() {
    let cards = fixtures::card_set();
    let scout = cards
        .get(fixtures::id("quarry-scout"))
        .expect("quarry-scout is a fixture");
    let skill = scout.get::<Skill>().expect("quarry-scout prints a Skill");
    assert_eq!(
        skill.get::<Cost>(),
        Some(&Cost {
            generic: 1,
            ..Cost::default()
        })
    );
    assert_eq!(skill.all::<EffectLeaf>(), vec![&EffectLeaf::MoveSummon]);
}

// --- Ready vs Exhausted gating --------------------------------------

#[test]
fn an_exhausted_summon_rejects_activation() {
    let mut state = base_state("quarry-scout", false);
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::SummonExhausted));
}

#[test]
fn a_missing_summon_rejects_activation() {
    let mut state = base_state("quarry-scout", true);
    state.players.get_mut(PlayerId::One).main = None;

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::EmptyPosition));
}

#[test]
fn an_unknown_ability_id_is_an_invalid_target() {
    // Naming an ability this card does not print is a bad action, not bad
    // data: it rejects with `InvalidTarget`, the same as any other action
    // naming a target the game does not recognize as legal — it never
    // reaches for `demand` or breaks the game.
    let state = base_state("quarry-scout", true);

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-warden-guard"),
            targets: vec![],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

#[test]
fn a_summon_with_no_skill_nodes_is_an_invalid_target() {
    let state = base_state("quarry-whelp", true);

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

// --- exhaustion ordering ---------------------------------------------

#[test]
fn exhaustion_happens_before_the_skills_effects_resolve() {
    // Rules §15's fixed order pays the cost and exhausts the Summon
    // before its Skill resolves. `MoveSummon` moving the activating
    // Summon itself makes that order observable: if exhaustion ran
    // after the move, the mutation would land on the now-empty
    // originating slot and silently miss, leaving the moved Summon
    // still Ready.
    let mut state = base_state("quarry-whelp", true);
    state.players.get_mut(PlayerId::One).bench[0] =
        Some(summon(PlayerId::One, "quarry-scout", true));
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let outcome = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            mana_hint: None,
        },
    )
    .expect("Quarry Scout is Ready and the Generic cost is covered");

    let moved = outcome.state.players.get(PlayerId::One).bench[1]
        .as_ref()
        .expect("the Scout relocated to the second Bench slot");
    assert!(
        !moved.ready,
        "exhaustion applied to the original slot before the move ran"
    );
    assert!(outcome.state.players.get(PlayerId::One).bench[0].is_none());
}

// --- movement leaves against occupied and empty positions ------------

#[test]
fn move_summon_onto_an_occupied_bench_slot_is_rejected_before_payment() {
    let mut state = base_state("quarry-whelp", true);
    state.players.get_mut(PlayerId::One).bench[0] =
        Some(summon(PlayerId::One, "quarry-scout", true));
    state.players.get_mut(PlayerId::One).bench[1] =
        Some(summon(PlayerId::One, "quarry-whelp", true));
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

#[test]
fn move_summon_from_an_empty_bench_slot_is_rejected() {
    let mut state = base_state("quarry-whelp", true);
    state.players.get_mut(PlayerId::One).bench[0] =
        Some(summon(PlayerId::One, "quarry-scout", true));
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![
                Position::Bench(BenchSlot::Second),
                Position::Bench(BenchSlot::Third),
            ],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

#[test]
fn swap_positions_against_an_empty_bench_slot_is_rejected() {
    let state = base_state("quarry-warden-guard", true);

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-warden-guard"),
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

// --- payment and event batches ----------------------------------------

#[test]
fn activate_skill_pays_a_nonzero_cost_before_skill_activated_and_the_move_leaf_emits_no_event() {
    let mut state = base_state("quarry-whelp", true);
    state.players.get_mut(PlayerId::One).bench[0] =
        Some(summon(PlayerId::One, "quarry-scout", true));
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let outcome = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            mana_hint: None,
        },
    )
    .expect("Quarry Scout is Ready and the Generic cost is covered");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                ability: fixtures::skill_id("quarry-scout"),
            },
        ],
        "MoveSummon itself emits no event"
    );
    assert_eq!(
        outcome.state.work,
        VecDeque::from(vec![
            WorkItem::MovementTrigger(
                MovementStep::LeavingBench,
                PlayerId::One,
                Position::Bench(BenchSlot::Second)
            ),
            WorkItem::MovementTrigger(
                MovementStep::EnteringBench,
                PlayerId::One,
                Position::Bench(BenchSlot::Second)
            ),
        ])
    );
}

#[test]
fn activate_skill_with_a_free_cost_skips_mana_deducted_and_emits_the_leafs_event() {
    let state = base_state("quarry-well-tender", true);

    let outcome = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-well-tender"),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("Quarry Well-Tender's Skill is free");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                ability: fixtures::skill_id("quarry-well-tender"),
            },
            GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Summon(Position::Main),
                mana_type: ManaType::Matter,
            },
        ]
    );
}

#[test]
fn activate_skill_reports_a_mana_shortfall() {
    let mut state = base_state("quarry-whelp", true);
    state.players.get_mut(PlayerId::One).bench[0] =
        Some(summon(PlayerId::One, "quarry-scout", true));

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            mana_hint: None,
        },
    );

    assert!(matches!(result, Err(ActionError::InsufficientMana { .. })));
}

#[test]
fn activate_skill_rejects_a_hint_naming_an_empty_pool() {
    // The binding mana_hint rule: a hint naming an empty pool rejects
    // with InvalidManaHint rather than falling back to another pool,
    // the same as `declare_attack` and `cast_spell`.
    let mut state = base_state("quarry-whelp", true);
    state.players.get_mut(PlayerId::One).bench[0] =
        Some(summon(PlayerId::One, "quarry-scout", true));
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let result = activate_skill(
        &state,
        SkillActivation {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: fixtures::skill_id("quarry-scout"),
            targets: vec![
                Position::Bench(BenchSlot::First),
                Position::Bench(BenchSlot::Second),
            ],
            mana_hint: Some(ManaType::Spirit),
        },
    );

    assert_eq!(result, Err(ActionError::InvalidManaHint));
}

// --- end-to-end acceptance through the real `apply` entry point -------

#[test]
fn apply_activates_a_ready_summons_skill_pays_exhausts_and_resolves_its_effects_end_to_end() {
    // Acceptance: a Ready Summon activates a Skill through the real
    // `apply` entry point — its cost is paid, it becomes Exhausted,
    // `SkillActivated` is emitted, and its effect resolves (rules
    // §15, §43).
    let state = base_state("quarry-well-tender", true);

    let outcome = crate::engine::apply::apply(
        &state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-well-tender"),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("a Ready Summon may activate its own free Skill");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                ability: fixtures::skill_id("quarry-well-tender"),
            },
            GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Summon(Position::Main),
                mana_type: ManaType::Matter,
            },
        ]
    );
    assert!(
        !outcome
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .ready
    );
    assert_eq!(outcome.state.players.get(PlayerId::One).mana.matter, 1);
}

#[test]
fn apply_rejects_activation_from_an_exhausted_summon() {
    let state = base_state("quarry-well-tender", false);

    let result = crate::engine::apply::apply(
        &state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-well-tender"),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::SummonExhausted));
}

#[test]
fn apply_lets_the_ready_effect_spell_reenable_a_skill_activation() {
    // Rules §53: "If a Summon activates a Skill, becomes Exhausted,
    // and is later Readied by an effect, it may activate another
    // Skill." Second Wind is this crate's Ready-effect Spell fixture.
    let mut state = base_state("quarry-well-tender", false);
    state.players.get_mut(PlayerId::One).hand = vec![CardRef {
        instance: CardInstanceId(50),
        def: fixtures::id("second-wind"),
    }];
    state.players.get_mut(PlayerId::One).mana.matter = 1;

    let cast = crate::engine::apply::apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(50),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("Second Wind is affordable and legal in the caster's own Main");

    let defender_pass = crate::engine::apply::apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Two holds Priority after the cast opens a window");

    let caster_pass = crate::engine::apply::apply(
        &defender_pass.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("One's second consecutive pass closes the window and resolves the Spell");

    assert!(
        caster_pass
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .ready,
        "Second Wind's ReadySummon leaf turned the Exhausted Summon Ready again"
    );

    let reactivation = crate::engine::apply::apply(
        &caster_pass.state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-well-tender"),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("rules §53: a Summon Readied by an effect may activate another Skill");

    assert!(
        !reactivation
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .ready
    );
}

#[test]
fn apply_activates_a_swap_positions_skill_and_fires_the_four_movement_triggers_end_to_end() {
    // Rules §15, §28, §43: activating a Skill that resolves
    // `SwapPositions` moves two Summons the same way a normal Retreat
    // does, and the same four movement-trigger steps fire in the same
    // fixed order — LeavingMain, EnteringBench, LeavingBench,
    // EnteringMain. Neither Quarry Warden-Guard nor its own Bench
    // neighbour carries a Trigger for the first three; Hearth Warden,
    // now entering Main, fires its immediate Heal on the last one,
    // proving the queued `WorkItem::MovementTrigger`s this Skill leaf
    // enqueues (`engine::effects::swap_positions`) reach the real
    // `engine::triggers::movement_trigger` handler through `apply`
    // exactly as `engine::board::retreat` does.
    let mut state = base_state("quarry-warden-guard", true);
    state.players.get_mut(PlayerId::One).bench[0] = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: fixtures::id("hearth-warden"),
            },
            vec![],
        ),
        damage: 20,
        ..summon(PlayerId::One, "hearth-warden", true)
    });

    let outcome = crate::engine::apply::apply(
        &state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("quarry-warden-guard"),
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    )
    .expect("Quarry Warden-Guard is Ready and its Skill is free");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                ability: fixtures::skill_id("quarry-warden-guard"),
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: crate::domain::cards::TriggerEvent::EntersMain,
                ability: fixtures::trigger_id("hearth-warden"),
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ],
        "EnteringMain fires last, in fixed §28 order, after the three \
         silent steps ahead of it"
    );
    let healed = outcome
        .state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("Hearth Warden landed on Main");
    assert_eq!(healed.damage, 5);
    assert!(healed.entered_main_this_turn);
    assert!(
        !outcome.state.players.get(PlayerId::One).bench[0]
            .as_ref()
            .expect("Quarry Warden-Guard landed on the Bench")
            .ready,
        "the activating Summon itself stays Exhausted after its own Skill resolves"
    );
}

// --- naming an ability by id, independent of print order ---------------

#[test]
fn activating_by_id_finds_the_same_ability_no_matter_where_it_sits_in_print_order() {
    // The whole point of naming an ability by id: a positional lookup and
    // an id lookup agree on a card whose Skills happen to sit in one
    // printed order, and disagree the moment that order changes. This
    // probe card prints two Skills — Ember Vent, which this test never
    // activates, and Steady Draft, which it does — once with Steady Draft
    // printed second and once with it printed first. The identical
    // `ActivateSkill { ability }` action must produce the identical
    // outcome either way: the same cost paid, the same effect resolved,
    // the same events, in the same order.
    let probe = probe_id(0x01);
    let ember_vent = Entity {
        id: probe_id(0xa1),
        components: vec![
            Component::Cost(Cost {
                mind: 1,
                ..Cost::default()
            }),
            Component::Effect(EffectLeaf::Heal { amount: 1 }),
        ],
    };
    let steady_draft = Entity {
        id: probe_id(0xb2),
        components: vec![
            Component::Cost(Cost {
                generic: 1,
                ..Cost::default()
            }),
            Component::Effect(EffectLeaf::Heal { amount: 5 }),
        ],
    };

    let steady_draft_printed_second = Entity {
        id: probe,
        components: vec![
            Component::Skill(ember_vent.clone()),
            Component::Skill(steady_draft.clone()),
        ],
    };
    let steady_draft_printed_first = Entity {
        id: probe,
        components: vec![
            Component::Skill(steady_draft.clone()),
            Component::Skill(ember_vent.clone()),
        ],
    };

    let activate = |card: Entity| {
        let mut state = base_state_with_cards(cards_with(vec![card]), probe, true);
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            damage: 5,
            ..summon_with_def(PlayerId::One, probe, true)
        });
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        crate::engine::apply::apply(
            &state,
            &GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
                ability: steady_draft.id,
                targets: vec![Position::Main],
                mana_hint: None,
            },
        )
        .expect("Steady Draft's Generic cost is covered")
    };

    let printed_second = activate(steady_draft_printed_second);
    let printed_first = activate(steady_draft_printed_first);

    assert_eq!(
        printed_second.events, printed_first.events,
        "reordering the card's Skills does not change which ability the same id activates"
    );
    assert_eq!(
        printed_second.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                ability: steady_draft.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 5,
            },
        ]
    );
    assert!(
        !printed_second
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .ready,
        "activating Steady Draft exhausts the Summon regardless of print order"
    );
}

#[test]
fn activating_an_id_the_card_does_not_print_is_an_invalid_target_end_to_end() {
    // The other half of the Demo: an action naming an ability id the
    // card does not print is a bad action, not bad data, so the game
    // does not break — `apply` returns `ActionError::InvalidTarget`.
    let probe = probe_id(0x02);
    let steady_draft = Entity {
        id: probe_id(0xc3),
        components: vec![Component::Effect(EffectLeaf::Heal { amount: 5 })],
    };
    let card = Entity {
        id: probe,
        components: vec![Component::Skill(steady_draft)],
    };
    let state = base_state_with_cards(cards_with(vec![card]), probe, true);

    let result = crate::engine::apply::apply(
        &state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: probe_id(0xff),
            targets: vec![],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}
