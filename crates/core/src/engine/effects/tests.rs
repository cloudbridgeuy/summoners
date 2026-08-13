//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.

use super::*;
use crate::domain::cards::fixtures;
use crate::domain::ids::{BenchSlot, CardInstanceId};
use crate::domain::state::{
    CardRef, ManaBank, PerPlayer, Phase, PlayerState, TurnState, UpgradeChain,
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
            spell_played_this_turn: false,
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: VecDeque::new(),
        pending: None,
        outcome: None,
        cards: fixtures::card_set(),
    }
}

#[test]
fn deal_damage_hits_the_defenders_main_and_enqueues_a_destruction_check() {
    let state = base_state();
    let leaf = EffectLeaf::DealDamage {
        amount: 10,
        immutable: false,
    };

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::DamageApplied {
            position: Position::Main,
            before: 0,
            after: 10,
        }]
    );
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::DestructionCheck(Position::Main)])
    );
}

#[test]
fn deal_damage_applies_its_full_amount_regardless_of_immutability() {
    // Rules §30, the Old Sow's `Root and Renew`-adjacent Attack text: "her
    // attack damage can be neither increased nor prevented." The "prevented"
    // half needs no active guard anywhere in this interpreter, because no
    // leaf in this crate ever reduces an opponent's already-accumulated
    // Damage before it lands (`heal` only ever reads `controller`'s own
    // board — see the `heal` tests above). This test pins that down
    // directly: `deal_damage` computes the identical `after` amount whether
    // `immutable` is true or false, since nothing about the flag ever
    // reaches its own arithmetic.
    let mutable = EffectLeaf::DealDamage {
        amount: 70,
        immutable: false,
    };
    let immutable = EffectLeaf::DealDamage {
        amount: 70,
        immutable: true,
    };

    let (_, mutable_events) = apply_leaf(&base_state(), PlayerId::One, &[Position::Main], &mutable);
    let (_, immutable_events) =
        apply_leaf(&base_state(), PlayerId::One, &[Position::Main], &immutable);

    assert_eq!(mutable_events, immutable_events);
}

#[test]
fn deal_damage_on_an_empty_position_is_a_silent_miss() {
    let state = base_state();
    let leaf = EffectLeaf::DealDamage {
        amount: 10,
        immutable: false,
    };

    let (state, events) = apply_leaf(
        &state,
        PlayerId::One,
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert!(events.is_empty());
    assert!(state.work.is_empty());
}

#[test]
fn deal_damage_with_no_target_is_a_silent_miss() {
    let state = base_state();
    let leaf = EffectLeaf::DealDamage {
        amount: 10,
        immutable: false,
    };

    let (state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

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
    let leaf = EffectLeaf::Heal { amount: 20 };

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

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
    let leaf = EffectLeaf::Heal { amount: 20 };

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

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
    let leaf = EffectLeaf::Heal { amount: 20 };

    let (_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert!(events.is_empty());
}

#[test]
fn draw_cards_draws_for_the_controller_and_enqueues_a_loss_check() {
    let state = base_state();
    let leaf = EffectLeaf::DrawCards { amount: 1 };

    let (state, events) = apply_leaf(&state, PlayerId::Two, &[], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::CardDrawn {
            player: PlayerId::Two,
            card: CardInstanceId(10),
        }]
    );
    assert_eq!(state.players.get(PlayerId::Two).hand.len(), 1);
    assert_eq!(state.players.get(PlayerId::Two).deck.len(), 1);
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)])
    );
}

#[test]
fn draw_cards_stops_early_once_the_deck_empties() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).deck = vec![CardRef {
        instance: CardInstanceId(20),
        def: fixtures::id("quarry-whelp"),
    }];
    let leaf = EffectLeaf::DrawCards { amount: 5 };

    let (state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

    assert_eq!(events.len(), 1, "only one card was available to draw");
    assert!(state.players.get(PlayerId::One).deck.is_empty());
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::LossCheck(PlayerId::One)]),
        "a LossCheck is still enqueued even though the request over-asked"
    );
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

    let (next_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn look_at_prizes_emits_an_honest_observation_event_even_with_no_prizes_left() {
    let state = base_state();
    let leaf = EffectLeaf::LookAtPrizes;

    let (next_state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

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

    let (_state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::PrizesViewed {
            player: PlayerId::One,
            prizes: vec![CardInstanceId(20), CardInstanceId(21)],
        }]
    );
}

#[test]
fn conditional_bonus_applies_extra_damage_when_the_condition_holds() {
    let mut state = base_state();
    state.turn.spell_played_this_turn = true;
    let leaf = EffectLeaf::ConditionalBonus {
        condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
        amount: 40,
    };

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::DamageApplied {
            position: Position::Main,
            before: 0,
            after: 40,
        }]
    );
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::DestructionCheck(Position::Main)])
    );
}

#[test]
fn conditional_bonus_is_a_silent_miss_when_the_condition_does_not_hold() {
    let state = base_state();
    let leaf = EffectLeaf::ConditionalBonus {
        condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
        amount: 40,
    };

    let (next_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn conditional_bonus_reads_defender_entered_main_this_turn_off_the_opponents_board() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        entered_main_this_turn: true,
        ..whelp(PlayerId::Two)
    });
    let leaf = EffectLeaf::ConditionalBonus {
        condition: crate::domain::cards::EffectCondition::DefenderEnteredMainThisTurn,
        amount: 30,
    };

    let (_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert_eq!(
        events,
        vec![GameEvent::DamageApplied {
            position: Position::Main,
            before: 0,
            after: 30,
        }]
    );
}

#[test]
fn immutable_damage_in_finds_an_immutable_deal_damage_leaf() {
    let leaves = [
        EffectLeaf::DealDamage {
            amount: 70,
            immutable: true,
        },
        EffectLeaf::ConditionalBonus {
            condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
            amount: 40,
        },
    ];

    assert!(immutable_damage_in(&leaves));
}

#[test]
fn immutable_damage_in_is_false_without_an_immutable_deal_damage_leaf() {
    let leaves = [
        EffectLeaf::DealDamage {
            amount: 50,
            immutable: false,
        },
        EffectLeaf::ConditionalBonus {
            condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
            amount: 40,
        },
    ];

    assert!(!immutable_damage_in(&leaves));
    assert!(!immutable_damage_in(&[]));
}

#[test]
fn cannot_be_moved_by_opponent_attaches_the_duration_marker() {
    let state = base_state();
    let leaf = EffectLeaf::CannotBeMovedByOpponent;

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

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
    let leaf = EffectLeaf::CannotBeMovedByOpponent;

    let (_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

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

    let (state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

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

    let (next_state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

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

    let (state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

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
        PlayerId::One,
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
        PlayerId::One,
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
        PlayerId::One,
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
        PlayerId::One,
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
        PlayerId::One,
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
        PlayerId::One,
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
        PlayerId::One,
        &[Position::Bench(BenchSlot::First)],
        &leaf,
    );

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn produce_mana_leaf_banks_the_named_summons_own_printed_type() {
    let state = base_state();
    let leaf = EffectLeaf::ProduceMana;

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

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
fn produce_mana_leaf_with_no_target_is_a_no_op() {
    let state = base_state();
    let leaf = EffectLeaf::ProduceMana;

    let (next_state, events) = apply_leaf(&state, PlayerId::One, &[], &leaf);

    assert!(events.is_empty());
    assert_eq!(next_state, state);
}

#[test]
fn ready_summon_turns_the_targeted_summon_ready() {
    let state = base_state();
    let leaf = EffectLeaf::ReadySummon;

    let (state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

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

    let (_state, events) = apply_leaf(&state, PlayerId::One, &[Position::Main], &leaf);

    assert!(events.is_empty());
}
