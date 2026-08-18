//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.

use super::*;
use crate::domain::cards::fixtures;
use crate::domain::cards::{Entity, EntityId};
use crate::domain::ids::CardInstanceId;
use crate::domain::state::{
    CardRef, GameOutcome, GameStatus, LossReason, ManaBank, PerPlayer, Phase, TurnState,
    UpgradeChain,
};
use std::collections::VecDeque;
use std::sync::Arc;

fn summon(owner: PlayerId) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def: fixtures::id("quarry-whelp"),
            },
            vec![],
        ),
        damage: 0,
        ready: true,
        owner,
        controller: owner,
        duration_markers: vec![],
        turn: crate::domain::state::SummonTurnRecord::fresh(),
    }
}

fn player_state(owner: PlayerId) -> PlayerState {
    PlayerState {
        main: Some(summon(owner)),
        bench: [None, None, None],
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        enchantments: vec![],
    }
}

fn base_state() -> GameState {
    GameState {
        players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
        coin: None,
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

fn lethal(owner: PlayerId) -> SummonInstance {
    SummonInstance {
        damage: 40, // Quarry Whelp's printed Life.
        ..summon(owner)
    }
}

// -- check ---------------------------------------------------------

#[test]
fn check_is_a_no_op_for_a_non_lethal_hit() {
    let state = base_state();

    let (next_state, events) = check(&state, Position::Main);

    assert_eq!(next_state, state);
    assert!(events.is_empty());
}

#[test]
fn check_enqueues_the_six_step_main_chain_for_an_over_damaged_main() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));

    let (state, events) = check(&state, Position::Main);

    assert!(events.is_empty());
    assert_eq!(
        state.work,
        VecDeque::from(vec![
            WorkItem::DiscardDestroyedChain(Position::Main),
            WorkItem::RecordMainLoss(PlayerId::Two),
            WorkItem::RecoverPrize(PlayerId::Two),
            WorkItem::PromoteBenchSummon(PlayerId::Two),
            WorkItem::ResolveMovementConsequences(PlayerId::Two),
            WorkItem::LossCheck(PlayerId::Two),
        ])
    );
}

#[test]
fn check_enqueues_only_discard_and_loss_check_for_an_over_damaged_bench_summon() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).bench[0] = Some(lethal(PlayerId::Two));

    let (state, _events) = check(&state, Position::Bench(BenchSlot::First));

    assert_eq!(
        state.work,
        VecDeque::from(vec![
            WorkItem::DiscardDestroyedChain(Position::Bench(BenchSlot::First)),
            WorkItem::LossCheck(PlayerId::Two),
        ])
    );
}

#[test]
fn check_orders_the_opponent_of_the_active_player_first_when_both_are_destroyed() {
    // Rules §41: one event destroying both players' Summons at once
    // resolves the active player's opponent's chain first.
    let mut state = base_state();
    state.turn.active_player = PlayerId::One;
    state.players.get_mut(PlayerId::One).main = Some(lethal(PlayerId::One));
    state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));

    let (state, _events) = check(&state, Position::Main);

    let players_in_order: Vec<PlayerId> = state
        .work
        .iter()
        .filter_map(|item| match item {
            WorkItem::RecordMainLoss(player) => Some(*player),
            _ => None,
        })
        .collect();
    assert_eq!(players_in_order, vec![PlayerId::Two, PlayerId::One]);
}

// -- life_of / is_destroyed / check / discard_destroyed_chain: a card
// that cannot answer for its own printed Life breaks the game rather
// than panicking or silently treating the card as indestructible. --

/// An id no entity in `base_state()`'s own `fixtures::card_set()` uses.
fn life_less_entity_id() -> EntityId {
    EntityId::parse(&"aa".repeat(16)).expect("valid fixture id")
}

/// A top-level entity that prints nothing at all — no `Life` — wrapped
/// in a fresh, locally built `CardSet`/`Arc`, exactly as this crate's
/// fixtures never carry a card like this on purpose.
fn state_with_life_less_main(player: PlayerId) -> GameState {
    let mut state = base_state();
    let entity = Entity {
        id: life_less_entity_id(),
        components: vec![],
    };
    state.cards = Arc::new(CardSet::new(vec![entity]));
    state.players.get_mut(player).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def: life_less_entity_id(),
            },
            vec![],
        ),
        ..summon(player)
    });
    state
}

#[test]
fn life_of_breaks_when_the_entitys_card_prints_no_life() {
    let state = state_with_life_less_main(PlayerId::Two);
    let summon = state
        .players
        .get(PlayerId::Two)
        .main
        .as_ref()
        .expect("just set");

    assert_eq!(
        life_of(&state.cards, summon),
        Err(Breakage {
            rule: "destruction",
            entity: life_less_entity_id(),
            expected: ComponentKind::Life,
        })
    );
}

#[test]
fn life_of_breaks_when_the_entity_id_is_not_in_the_card_set_at_all() {
    let state = base_state();
    let missing_id = life_less_entity_id();
    let summon = SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def: missing_id,
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    };

    assert_eq!(
        life_of(&state.cards, &summon),
        Err(Breakage {
            rule: "destruction",
            entity: missing_id,
            expected: ComponentKind::Life,
        })
    );
}

#[test]
fn check_breaks_the_game_when_the_occupying_cards_life_cannot_be_read() {
    let state = state_with_life_less_main(PlayerId::Two);

    let (state, events) = check(&state, Position::Main);

    assert!(events.is_empty());
    assert_eq!(
        state.status,
        GameStatus::Broken(Breakage {
            rule: "destruction",
            entity: life_less_entity_id(),
            expected: ComponentKind::Life,
        })
    );
    assert!(
        state.work.is_empty(),
        "nothing was enqueued before the read that broke the game"
    );
}

#[test]
fn discard_destroyed_chain_breaks_the_game_when_the_occupying_cards_life_cannot_be_read() {
    let state = state_with_life_less_main(PlayerId::Two);

    let (state, events) = discard_destroyed_chain(&state, Position::Main);

    assert!(events.is_empty());
    assert_eq!(
        state.status,
        GameStatus::Broken(Breakage {
            rule: "destruction",
            entity: life_less_entity_id(),
            expected: ComponentKind::Life,
        })
    );
    assert!(
        state.players.get(PlayerId::Two).main.is_some(),
        "the summon was never taken off the board once the read broke the game"
    );
}

// -- discard_destroyed_chain -----------------------------------------

#[test]
fn discard_destroyed_chain_moves_the_chain_to_the_owners_discard() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));

    let (state, events) = discard_destroyed_chain(&state, Position::Main);

    assert_eq!(
        events,
        vec![GameEvent::SummonDestroyed {
            position: Position::Main,
            owner: PlayerId::Two,
        }]
    );
    assert_eq!(state.players.get(PlayerId::Two).main, None);
    assert_eq!(
        state.players.get(PlayerId::Two).discard,
        vec![CardRef {
            instance: CardInstanceId(1),
            def: fixtures::id("quarry-whelp"),
        }]
    );
}

#[test]
fn discard_destroyed_chain_is_a_no_op_when_nothing_here_is_over_damaged() {
    let state = base_state();

    let (next_state, events) = discard_destroyed_chain(&state, Position::Main);

    assert_eq!(next_state, state);
    assert!(events.is_empty());
}

#[test]
fn discard_destroyed_chain_queues_a_matching_any_summon_destroyed_trigger() {
    // Rules §37, §41: Spite Thorn's respondable trigger fires whenever
    // any Summon is destroyed, including its own controller's own
    // Bench Summon being destroyed elsewhere.
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));
    state.players.get_mut(PlayerId::One).bench[0] = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(9),
                def: fixtures::id("spite-thorn"),
            },
            vec![],
        ),
        ..summon(PlayerId::One)
    });

    let (state, events) = discard_destroyed_chain(&state, Position::Main);

    assert_eq!(
        events,
        vec![GameEvent::SummonDestroyed {
            position: Position::Main,
            owner: PlayerId::Two,
        }]
    );
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::FireTrigger(
            PlayerId::One,
            Position::Bench(BenchSlot::First),
            TriggerEvent::AnySummonDestroyed,
            fixtures::trigger_id("spite-thorn"),
        )])
    );
}

// -- record_main_loss --------------------------------------------------

#[test]
fn record_main_loss_increments_the_losers_count() {
    let state = base_state();

    let (state, events) = record_main_loss(&state, PlayerId::Two);

    assert_eq!(state.players.get(PlayerId::Two).main_losses, 1);
    assert!(events.is_empty());
}

// -- recover_prize / answer_prize ---------------------------------------

fn prize(instance: u32) -> CardRef {
    CardRef {
        instance: CardInstanceId(instance),
        def: fixtures::id("quarry-whelp"),
    }
}

#[test]
fn recover_prize_pauses_for_the_opponent_to_choose() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).prizes = vec![prize(9)];

    let (state, events) = recover_prize(&state, PlayerId::Two);

    assert_eq!(
        state.pending,
        Some(PendingInput::PrizePick {
            chooser: PlayerId::One
        })
    );
    assert!(events.is_empty());
}

#[test]
fn recover_prize_is_a_no_op_with_no_prizes_left() {
    let state = base_state();

    let (state, events) = recover_prize(&state, PlayerId::Two);

    assert_eq!(state.pending, None);
    assert!(events.is_empty());
}

#[test]
fn answer_prize_moves_the_chosen_prize_to_the_recovering_players_hand() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).prizes = vec![prize(9), prize(10)];
    state.pending = Some(PendingInput::PrizePick {
        chooser: PlayerId::One,
    });

    let outcome = answer_prize(&state, PlayerId::One, 0).expect("a prize is pending");

    assert_eq!(outcome.state.pending, None);
    assert_eq!(
        outcome.state.players.get(PlayerId::Two).prizes,
        vec![prize(10)]
    );
    assert_eq!(
        outcome.state.players.get(PlayerId::Two).hand,
        vec![prize(9)]
    );
    assert_eq!(
        outcome.events,
        vec![GameEvent::PrizeRecovered {
            player: PlayerId::Two,
            card: CardInstanceId(9),
        }]
    );
}

#[test]
fn answer_prize_rejects_a_mismatched_pending() {
    let state = base_state();

    assert_eq!(
        answer_prize(&state, PlayerId::One, 0),
        Err(ActionError::PendingInputMismatch)
    );
}

#[test]
fn answer_prize_rejects_the_wrong_chooser() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).prizes = vec![prize(9)];
    state.pending = Some(PendingInput::PrizePick {
        chooser: PlayerId::One,
    });

    assert_eq!(
        answer_prize(&state, PlayerId::Two, 0),
        Err(ActionError::PendingInputMismatch)
    );
}

#[test]
fn answer_prize_rejects_an_out_of_range_index() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).prizes = vec![prize(9)];
    state.pending = Some(PendingInput::PrizePick {
        chooser: PlayerId::One,
    });

    assert_eq!(
        answer_prize(&state, PlayerId::One, 1),
        Err(ActionError::InvalidTarget)
    );
}

// -- promote_bench_summon / answer_promotion -----------------------------

#[test]
fn promote_bench_summon_is_automatic_with_exactly_one_bench_summon() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = None;
    state.players.get_mut(PlayerId::Two).bench[1] = Some(summon(PlayerId::Two));

    let (state, events) = promote_bench_summon(&state, PlayerId::Two);

    assert_eq!(state.pending, None);
    assert!(state.players.get(PlayerId::Two).main.is_some());
    assert_eq!(state.players.get(PlayerId::Two).bench[1], None);
    assert_eq!(
        events,
        vec![GameEvent::SummonPromoted {
            player: PlayerId::Two,
            from: BenchSlot::Second,
        }]
    );
}

#[test]
fn promote_bench_summon_pauses_with_several_bench_summons() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = None;
    state.players.get_mut(PlayerId::Two).bench = [
        Some(summon(PlayerId::Two)),
        Some(summon(PlayerId::Two)),
        None,
    ];

    let (state, events) = promote_bench_summon(&state, PlayerId::Two);

    assert_eq!(
        state.pending,
        Some(PendingInput::Promotion {
            player: PlayerId::Two
        })
    );
    assert!(events.is_empty());
}

#[test]
fn promote_bench_summon_is_a_no_op_with_an_empty_bench() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = None;

    let (state, events) = promote_bench_summon(&state, PlayerId::Two);

    assert_eq!(state.pending, None);
    assert_eq!(state.players.get(PlayerId::Two).main, None);
    assert!(events.is_empty());
}

#[test]
fn answer_promotion_moves_the_chosen_slot_to_main_and_preserves_ready() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = None;
    let mut resting = summon(PlayerId::Two);
    resting.ready = false;
    state.players.get_mut(PlayerId::Two).bench = [None, Some(resting), None];
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::Two,
    });
    // Mirrors what `enqueue_destruction` already left queued, right
    // behind the paused `PromoteBenchSummon` step, before the multi-slot
    // Bench forced this pause (rules §24 step 5 follows step 4).
    state
        .work
        .push_back(WorkItem::ResolveMovementConsequences(PlayerId::Two));

    let outcome =
        answer_promotion(&state, PlayerId::Two, BenchSlot::Second).expect("a slot is filled");

    assert_eq!(outcome.state.pending, None);
    let promoted = outcome
        .state
        .players
        .get(PlayerId::Two)
        .main
        .as_ref()
        .expect("promotion filled Main");
    assert!(
        !promoted.ready,
        "Promotion is a movement, not a new arrival"
    );
    assert_eq!(outcome.state.players.get(PlayerId::Two).bench[1], None);
    assert_eq!(
        outcome.events,
        vec![GameEvent::SummonPromoted {
            player: PlayerId::Two,
            from: BenchSlot::Second,
        }]
    );

    // `answer_promotion` itself only moves the Summon (rules §24 step
    // 4); the Main-entry record is set later, when the queued
    // `ResolveMovementConsequences` step (already sitting in `work`
    // above, exactly as it would be mid-destruction-chain) enqueues an
    // `EnteringMain` trigger and draining runs it through
    // `engine::triggers::movement_trigger`.
    let (drained, _) = crate::engine::resolution::drain(&outcome.state);
    let drained_promoted = drained
        .players
        .get(PlayerId::Two)
        .main
        .as_ref()
        .expect("promotion still filled Main after drain");
    assert!(drained_promoted.turn.main_entry.is_some());
}

#[test]
fn answer_promotion_rejects_a_mismatched_pending() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).bench[0] = Some(summon(PlayerId::Two));

    assert_eq!(
        answer_promotion(&state, PlayerId::Two, BenchSlot::First),
        Err(ActionError::PendingInputMismatch)
    );
}

#[test]
fn answer_promotion_rejects_an_empty_slot() {
    let mut state = base_state();
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::Two,
    });

    assert_eq!(
        answer_promotion(&state, PlayerId::Two, BenchSlot::First),
        Err(ActionError::EmptyPosition)
    );
}

// -- resolve_movement_consequences ---------------------------------------

#[test]
fn resolve_movement_consequences_queues_the_two_steps_ahead_of_a_pending_loss_check() {
    let mut state = base_state();
    state.work = VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)]);

    let (state, events) = resolve_movement_consequences(&state, PlayerId::Two);

    assert!(events.is_empty());
    assert_eq!(
        state.work,
        VecDeque::from(vec![
            WorkItem::MovementTrigger(MovementStep::LeavingBench, PlayerId::Two, Position::Main),
            WorkItem::MovementTrigger(MovementStep::EnteringMain, PlayerId::Two, Position::Main),
            WorkItem::LossCheck(PlayerId::Two),
        ])
    );
}

#[test]
fn resolve_movement_consequences_is_a_no_op_when_no_promotion_landed() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = None;

    let (state, events) = resolve_movement_consequences(&state, PlayerId::Two);

    assert!(state.work.is_empty());
    assert!(events.is_empty());
}

// -- freezing on a third Main loss mid-resolution -------------------------

#[test]
fn a_third_main_loss_freezes_the_rest_of_the_chain() {
    // Rules §2: the third Main loss ends the game outright, even with a
    // Promotion still queued behind `LossCheck`.
    let mut state = base_state();
    let two = state.players.get_mut(PlayerId::Two);
    two.main_losses = 2;
    two.main = Some(lethal(PlayerId::Two));
    two.bench[0] = Some(summon(PlayerId::Two));

    let (state, _events) = check(&state, Position::Main);
    let (state, _events) = discard_destroyed_chain(&state, Position::Main);
    let (state, _events) = record_main_loss(&state, PlayerId::Two);
    let (state, _events) = recover_prize(&state, PlayerId::Two);
    let (state, _events) = promote_bench_summon(&state, PlayerId::Two);
    let (state, _events) = crate::engine::loss::check(&state, PlayerId::Two);

    assert_eq!(
        state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::ThirdMainLoss,
        })
    );
    // The `ResolveMovementConsequences` step queued by `check` is still
    // sitting untouched: the resolution loop's own pending/status
    // check (not tested here — see `resolution::drain`) is what stops
    // it from ever running once `status` leaves `Playing`.
    assert!(
        state
            .work
            .contains(&WorkItem::ResolveMovementConsequences(PlayerId::Two))
    );
}
