//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.

use super::*;
use crate::domain::cards::{
    DamageConstraint, DamageConstraints, DamageEffect, EffectSource, fixtures,
};
use crate::domain::ids::{BenchSlot, CardInstanceId};
use crate::domain::state::{
    CardRef, GameStatus, ManaBank, PendingInput, PerPlayer, Phase, PlayerState, TurnState,
    UpgradeChain,
};
use std::collections::VecDeque;

fn whelp(owner: PlayerId) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def: fixtures::id("quarry-whelp"),
            },
            vec![],
        ),
        damage: 0,
        ready: false,
        owner,
        controller: owner,
        duration_markers: vec![],
        played_this_turn: false,
        upgraded_this_turn: false,
        entered_main_this_turn: false,
    }
}

fn player_state(owner: PlayerId) -> PlayerState {
    PlayerState {
        main: Some(whelp(owner)),
        bench: [None, None, None],
        deck: vec![
            CardRef {
                instance: CardInstanceId(10),
                def: fixtures::id("quarry-whelp"),
            },
            CardRef {
                instance: CardInstanceId(11),
                def: fixtures::id("quarry-whelp"),
            },
        ],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        has_coin: false,
        enchantments: vec![],
    }
}

fn base_state() -> GameState {
    GameState {
        players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
        turn: TurnState {
            active_player: PlayerId::One,
            phase: Phase::Main,
            window: None,
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: PerPlayer::new(false, false),
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: VecDeque::new(),
        pending: None,
        status: GameStatus::Playing,
        cards: fixtures::card_set(),
    }
}

fn source(controller: PlayerId) -> EffectSource {
    EffectSource::Spell {
        controller,
        card: CardInstanceId(999),
        definition: fixtures::id("ember-lance"),
    }
}

#[test]
fn effect_position_selects_the_authored_target_kind() {
    let skill_source = EffectSource::Skill {
        controller: PlayerId::One,
        position: Position::Main,
        ability: fixtures::skill_id("quarry-scout"),
    };
    assert_eq!(
        effect_position(
            skill_source,
            &[Position::Bench(BenchSlot::First)],
            EffectTarget::Selected,
        ),
        Some(Position::Bench(BenchSlot::First))
    );
    assert_eq!(
        effect_position(
            skill_source,
            &[Position::Bench(BenchSlot::First)],
            EffectTarget::Source,
        ),
        Some(Position::Main)
    );
    assert_eq!(
        effect_position(source(PlayerId::One), &[], EffectTarget::Source),
        None
    );
}

#[test]
fn deal_damage_hits_the_defenders_main_and_enqueues_a_destruction_check() {
    let state = base_state();
    let leaf = EffectLeaf::DealDamage(DamageEffect {
        base: 10,
        constraints: DamageConstraints::new(),
        additions: vec![],
    });

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert_eq!(events.len(), 3);
    assert!(matches!(
        events.last(),
        Some(GameEvent::DamageApplied {
            amount: 10,
            before: 0,
            after: 10,
            ..
        })
    ));
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::DestructionCheck(Position::Main)])
    );
}

#[test]
fn constraints_do_not_change_unadjusted_damage() {
    let unconstrained = EffectLeaf::DealDamage(DamageEffect {
        base: 70,
        constraints: DamageConstraints::new(),
        additions: vec![],
    });
    let constrained = EffectLeaf::DealDamage(DamageEffect {
        base: 70,
        constraints: DamageConstraints::from([
            DamageConstraint::Unincreasable,
            DamageConstraint::Unpreventable,
        ]),
        additions: vec![],
    });

    let (unconstrained_state, _) = apply_leaf(
        &base_state(),
        source(PlayerId::One),
        &[Position::Main],
        &unconstrained,
    );
    let (constrained_state, _) = apply_leaf(
        &base_state(),
        source(PlayerId::One),
        &[Position::Main],
        &constrained,
    );

    assert_eq!(
        unconstrained_state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        70
    );
    assert_eq!(
        constrained_state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        70
    );
}

#[test]
fn deal_damage_on_an_empty_position_is_a_silent_miss() {
    let state = base_state();
    let leaf = EffectLeaf::DealDamage(DamageEffect {
        base: 10,
        constraints: DamageConstraints::new(),
        additions: vec![],
    });

    let (state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert!(events.is_empty());
    assert!(state.work.is_empty());
}

#[test]
fn deal_damage_with_no_target_is_a_silent_miss() {
    let state = base_state();
    let leaf = EffectLeaf::DealDamage(DamageEffect {
        base: 10,
        constraints: DamageConstraints::new(),
        additions: vec![],
    });

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert!(events.is_empty());
    assert!(state.work.is_empty());
}

#[test]
fn heal_removes_damage_from_the_casters_own_main() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 15,
        ..whelp(PlayerId::One)
    });
    let leaf = EffectLeaf::Heal {
        amount: 20,
        target: EffectTarget::Selected,
    };

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::Healed {
            position: Position::Main,
            amount: 15,
        }]
    );
    assert_eq!(
        state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .damage,
        0
    );
}

#[test]
fn heal_cannot_reduce_damage_below_zero() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 5,
        ..whelp(PlayerId::One)
    });
    let leaf = EffectLeaf::Heal {
        amount: 20,
        target: EffectTarget::Selected,
    };

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::Healed {
            position: Position::Main,
            amount: 5,
        }]
    );
    assert_eq!(
        state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .damage,
        0
    );
}

#[test]
fn heal_on_an_empty_position_is_a_silent_miss() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = None;
    let leaf = EffectLeaf::Heal {
        amount: 20,
        target: EffectTarget::Selected,
    };

    let (_state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert!(events.is_empty());
}

#[test]
fn draw_cards_draws_for_the_controller_without_deferred_loss_work() {
    let state = base_state();
    let leaf = EffectLeaf::DrawCards { amount: 1 };

    let (state, events) = apply_leaf(&state, source(PlayerId::Two), &[], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::CardDrawn {
            player: PlayerId::Two,
            card: CardInstanceId(10),
        }]
    );
    assert_eq!(state.players.get(PlayerId::Two).hand.len(), 1);
    assert_eq!(state.players.get(PlayerId::Two).deck.len(), 1);
    assert!(state.work.is_empty());
    assert_eq!(state.status, GameStatus::Playing);
}

#[test]
fn draw_cards_draws_available_cards_then_ends_on_shortage() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).deck = vec![CardRef {
        instance: CardInstanceId(20),
        def: fixtures::id("quarry-whelp"),
    }];
    let leaf = EffectLeaf::DrawCards { amount: 5 };

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert_eq!(
        events,
        vec![
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: CardInstanceId(20),
            },
            GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: crate::domain::state::LossReason::EmptyDeckDraw,
            },
        ]
    );
    assert!(state.players.get(PlayerId::One).deck.is_empty());
    assert_eq!(
        state.status,
        GameStatus::Ended(crate::domain::state::GameOutcome {
            winner: PlayerId::Two,
            reason: crate::domain::state::LossReason::EmptyDeckDraw,
        })
    );
    assert!(state.work.is_empty());
}

#[test]
fn drawing_exactly_the_final_card_is_successful() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).deck.truncate(1);
    let leaf = EffectLeaf::DrawCards { amount: 1 };

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert_eq!(events.len(), 1);
    assert!(state.players.get(PlayerId::One).deck.is_empty());
    assert_eq!(state.status, GameStatus::Playing);
}

#[test]
fn block_responses_is_a_documented_no_op_here() {
    // `BlockResponses` gates casting an Attack Spell as a response rather
    // than changing board state; `engine::stack::cast_spell` reads its
    // condition and block directly, so this interpreter never touches it.
    let state = base_state();
    let leaf = EffectLeaf::BlockResponses {
        condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
        block: crate::domain::cards::ResponseBlock::AttackSpells,
    };

    let (next_state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn look_at_prizes_emits_an_honest_observation_event_even_with_no_prizes_left() {
    let state = base_state();
    let leaf = EffectLeaf::LookAtPrizes;

    let (next_state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::PrizesViewed {
            player: PlayerId::One,
            prizes: vec![],
        }]
    );
    assert_eq!(next_state, state, "the look changes nothing observable");
}

#[test]
fn look_at_prizes_names_every_prize_still_face_down() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).prizes = vec![
        CardRef {
            instance: CardInstanceId(20),
            def: fixtures::id("quarry-whelp"),
        },
        CardRef {
            instance: CardInstanceId(21),
            def: fixtures::id("quarry-whelp"),
        },
    ];
    let leaf = EffectLeaf::LookAtPrizes;

    let (_state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::PrizesViewed {
            player: PlayerId::One,
            prizes: vec![CardInstanceId(20), CardInstanceId(21)],
        }]
    );
}

#[test]
fn cannot_be_moved_by_opponent_attaches_the_duration_marker() {
    let state = base_state();
    let leaf = EffectLeaf::CannotBeMovedByOpponent {
        target: EffectTarget::Selected,
    };

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert!(events.is_empty());
    assert!(
        state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .duration_markers
            .contains(&crate::domain::state::DurationMarker::CannotBeMovedByOpponent)
    );
}

#[test]
fn cannot_be_moved_by_opponent_on_an_empty_position_is_a_silent_miss() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = None;
    let leaf = EffectLeaf::CannotBeMovedByOpponent {
        target: EffectTarget::Selected,
    };

    let (_state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert!(events.is_empty());
}

#[test]
fn return_spell_from_discard_moves_the_first_spell_to_hand() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).discard = vec![
        CardRef {
            instance: CardInstanceId(30),
            def: fixtures::id("quarry-whelp"),
        },
        CardRef {
            instance: CardInstanceId(31),
            def: fixtures::id("ember-lance"),
        },
    ];
    let leaf = EffectLeaf::ReturnSpellFromDiscard;

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert!(events.is_empty());
    assert_eq!(state.players.get(PlayerId::One).discard.len(), 1);
    assert_eq!(
        state.players.get(PlayerId::One).hand,
        vec![CardRef {
            instance: CardInstanceId(31),
            def: fixtures::id("ember-lance"),
        }]
    );
}

#[test]
fn return_spell_from_discard_with_no_spell_present_is_a_silent_miss() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).discard = vec![CardRef {
        instance: CardInstanceId(30),
        def: fixtures::id("quarry-whelp"),
    }];
    let leaf = EffectLeaf::ReturnSpellFromDiscard;

    let (next_state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn return_spell_to_deck_top_moves_the_first_spell_in_hand_to_the_deck_front() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = vec![CardRef {
        instance: CardInstanceId(40),
        def: fixtures::id("ember-lance"),
    }];
    let leaf = EffectLeaf::ReturnSpellToDeckTop;

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert!(events.is_empty());
    assert!(state.players.get(PlayerId::One).hand.is_empty());
    assert_eq!(
        state.players.get(PlayerId::One).deck.first(),
        Some(&CardRef {
            instance: CardInstanceId(40),
            def: fixtures::id("ember-lance"),
        })
    );
}

#[test]
fn swap_opposing_positions_exchanges_the_opponents_main_and_named_bench_slot() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).bench[0] = Some(whelp(PlayerId::Two));
    let leaf = EffectLeaf::SwapOpposingPositions;

    let (state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert_eq!(
        events,
        vec![GameEvent::SummonsSwapped {
            player: PlayerId::Two,
            main: BenchSlot::First,
        }]
    );
    // `apply_leaf` never drains `state.work`, so the queued `EnteringMain`
    // step below has not run yet — `entered_main_this_turn` is set only when
    // `engine::triggers::movement_trigger` processes that step, not here.
    assert!(
        !state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .entered_main_this_turn
    );
    assert!(state.players.get(PlayerId::Two).bench[0].is_some());
}

#[test]
fn swap_opposing_positions_refuses_a_main_that_cannot_be_moved_by_opponent() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        duration_markers: vec![crate::domain::state::DurationMarker::CannotBeMovedByOpponent],
        ..whelp(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).bench[0] = Some(whelp(PlayerId::Two));
    let leaf = EffectLeaf::SwapOpposingPositions;

    let (state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert!(events.is_empty());
    assert!(
        state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .duration_markers
            .contains(&crate::domain::state::DurationMarker::CannotBeMovedByOpponent)
    );
    assert!(state.players.get(PlayerId::Two).bench[0].is_some());
}

#[test]
fn move_summon_relocates_a_bench_summon_to_an_empty_bench_slot_and_enqueues_its_triggers() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).bench[0] = Some(whelp(PlayerId::One));
    let leaf = EffectLeaf::MoveSummon;

    let (state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[
            Position::Bench(BenchSlot::First),
            Position::Bench(BenchSlot::Second),
        ],
        &leaf,
    );

    assert!(events.is_empty());
    assert!(state.players.get(PlayerId::One).bench[0].is_none());
    assert!(state.players.get(PlayerId::One).bench[1].is_some());
    assert_eq!(
        state.work,
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
fn move_summon_onto_an_occupied_destination_is_a_silent_miss() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).bench[0] = Some(whelp(PlayerId::One));
    state.players.get_mut(PlayerId::One).bench[1] = Some(whelp(PlayerId::One));
    let leaf = EffectLeaf::MoveSummon;

    let (next_state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[
            Position::Bench(BenchSlot::First),
            Position::Bench(BenchSlot::Second),
        ],
        &leaf,
    );

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn move_summon_from_an_empty_source_is_a_silent_miss() {
    let state = base_state();
    let leaf = EffectLeaf::MoveSummon;

    let (next_state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[
            Position::Bench(BenchSlot::First),
            Position::Bench(BenchSlot::Second),
        ],
        &leaf,
    );

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn swap_positions_exchanges_main_and_the_named_bench_slot_and_enqueues_the_four_triggers() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).bench[0] = Some(whelp(PlayerId::One));
    let leaf = EffectLeaf::SwapPositions;

    let (state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert_eq!(
        events,
        vec![GameEvent::SummonsSwapped {
            player: PlayerId::One,
            main: BenchSlot::First,
        }]
    );
    // Same as `swap_opposing_positions`: the leaf only queues the
    // `EnteringMain` step below, it does not drain it, so the flag is still
    // false right after the leaf runs.
    assert!(
        !state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .entered_main_this_turn
    );
    assert!(state.players.get(PlayerId::One).bench[0].is_some());
    assert_eq!(
        state.work,
        VecDeque::from(vec![
            WorkItem::MovementTrigger(
                MovementStep::LeavingMain,
                PlayerId::One,
                Position::Bench(BenchSlot::First)
            ),
            WorkItem::MovementTrigger(
                MovementStep::EnteringBench,
                PlayerId::One,
                Position::Bench(BenchSlot::First)
            ),
            WorkItem::MovementTrigger(MovementStep::LeavingBench, PlayerId::One, Position::Main),
            WorkItem::MovementTrigger(MovementStep::EnteringMain, PlayerId::One, Position::Main),
        ])
    );
}

#[test]
fn swap_positions_against_an_empty_bench_slot_is_a_silent_miss() {
    let state = base_state();
    let leaf = EffectLeaf::SwapPositions;

    let (next_state, events) = apply_leaf(
        &state,
        source(PlayerId::One),
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn produce_mana_leaf_banks_the_named_summons_own_printed_type() {
    let state = base_state();
    let leaf = EffectLeaf::ProduceMana {
        target: EffectTarget::Selected,
    };

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::One,
            source: ManaSource::Summon(Position::Main),
            mana_type: crate::domain::ids::ManaType::Matter,
        }]
    );
    assert_eq!(state.players.get(PlayerId::One).mana.matter, 1);
}

#[test]
fn produce_mana_leaf_banks_for_non_active_controller() {
    let state = base_state();
    let leaf = EffectLeaf::ProduceMana {
        target: EffectTarget::Selected,
    };

    let (state, events) = apply_leaf(&state, source(PlayerId::Two), &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Main),
            mana_type: crate::domain::ids::ManaType::Matter,
        }]
    );
    assert_eq!(state.players.get(PlayerId::One).mana.matter, 0);
    assert_eq!(state.players.get(PlayerId::Two).mana.matter, 1);
}

#[test]
fn produce_mana_leaf_preserves_non_active_controller_in_pending_choice() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(31),
                def: fixtures::id("set-path-adept"),
            },
            vec![],
        ),
        ..whelp(PlayerId::Two)
    });

    let (state, events) = apply_leaf(
        &state,
        source(PlayerId::Two),
        &[Position::Main],
        &EffectLeaf::ProduceMana {
            target: EffectTarget::Selected,
        },
    );

    assert!(events.is_empty());
    assert_eq!(
        state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Main),
        })
    );
}

#[test]
fn source_targeted_mana_ignores_a_sibling_effects_selected_target() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).bench[0] = Some(whelp(PlayerId::Two));
    let trigger_source = EffectSource::Trigger {
        controller: PlayerId::Two,
        position: Position::Bench(BenchSlot::First),
        ability: fixtures::trigger_id("spite-thorn"),
    };

    let (state, events) = apply_leaf(
        &state,
        trigger_source,
        &[Position::Main],
        &EffectLeaf::ProduceMana {
            target: EffectTarget::Source,
        },
    );

    assert_eq!(
        events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Bench(BenchSlot::First)),
            mana_type: crate::domain::ids::ManaType::Matter,
        }]
    );
    assert_eq!(state.players.get(PlayerId::Two).mana.matter, 1);
}

#[test]
fn produce_mana_leaf_with_no_target_is_a_no_op() {
    let state = base_state();
    let leaf = EffectLeaf::ProduceMana {
        target: EffectTarget::Selected,
    };

    let (next_state, events) = apply_leaf(&state, source(PlayerId::One), &[], &leaf);

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn ready_summon_turns_the_targeted_summon_ready() {
    let state = base_state();
    let leaf = EffectLeaf::ReadySummon;

    let (state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::SummonsReadied {
            player: PlayerId::One,
            positions: vec![Position::Main],
        }]
    );
    assert!(
        state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .ready
    );
}

#[test]
fn ready_summon_on_an_empty_position_is_a_silent_miss() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = None;
    let leaf = EffectLeaf::ReadySummon;

    let (_state, events) = apply_leaf(&state, source(PlayerId::One), &[Position::Main], &leaf);

    assert!(events.is_empty());
}
