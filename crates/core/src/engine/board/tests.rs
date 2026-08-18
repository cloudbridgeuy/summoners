//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.
#![allow(clippy::expect_used)]

use super::*;
use crate::domain::cards::fixtures;
use crate::domain::ids::CardInstanceId;
use crate::domain::state::{
    CardRef, GameStatus, ManaBank, PendingInput, PerPlayer, StackWindow, TurnState, UpgradeActivity,
};
use std::collections::VecDeque;

fn card_ref(instance: u32, def: &'static str) -> CardRef {
    CardRef {
        instance: CardInstanceId(instance),
        def: fixtures::id(def),
    }
}

fn base_summon(owner: PlayerId, def: &'static str) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(card_ref(1, def), vec![]),
        damage: 0,
        readiness: Readiness::Ready,
        owner,
        controller: owner,
        duration_markers: vec![],
        turn: crate::domain::state::SummonTurnRecord::fresh(),
    }
}

fn empty_player(owner: PlayerId) -> PlayerState {
    PlayerState {
        main: Some(base_summon(owner, "quarry-whelp")),
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
        players: PerPlayer::new(empty_player(PlayerId::One), empty_player(PlayerId::Two)),
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

// --- play_summon ---------------------------------------------------

#[test]
fn play_summon_puts_a_base_into_an_empty_bench_slot_exhausted_and_played_this_turn() {
    let mut state = base_state();
    let card = card_ref(2, "quarry-whelp");
    state.players.one.hand.push(card);

    let outcome = play_summon(&state, PlayerId::One, card.instance, BenchSlot::First)
        .expect("a Base Summon may be played into an empty Bench slot");

    let bench = outcome.state.players.get(PlayerId::One).bench[0]
        .as_ref()
        .expect("the Bench slot now holds the played Summon");
    assert_eq!(bench.readiness, Readiness::Exhausted);
    assert_eq!(bench.turn.upgrade, UpgradeActivity::PlayedThisTurn);
    assert_eq!(bench.chain.base(), card);
    assert!(outcome.state.players.get(PlayerId::One).hand.is_empty());
}

#[test]
fn play_summon_emits_exactly_one_summon_played_event() {
    let mut state = base_state();
    let card = card_ref(2, "quarry-whelp");
    state.players.one.hand.push(card);

    let outcome = play_summon(&state, PlayerId::One, card.instance, BenchSlot::Second)
        .expect("a Base Summon may be played into an empty Bench slot");

    assert_eq!(
        outcome.events,
        vec![GameEvent::SummonPlayed {
            player: PlayerId::One,
            card: card.instance,
            slot: BenchSlot::Second,
        }]
    );
}

#[test]
fn play_summon_rejects_a_pending_decision() {
    let mut state = base_state();
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });

    let result = play_summon(&state, PlayerId::One, CardInstanceId(2), BenchSlot::First);

    assert_eq!(result, Err(ActionError::PendingInputMismatch));
}

#[test]
fn play_summon_rejects_outside_main_phase() {
    let mut state = base_state();
    state.turn.phase = Phase::Combat;

    let result = play_summon(&state, PlayerId::One, CardInstanceId(2), BenchSlot::First);

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn play_summon_rejects_an_open_priority_window() {
    let mut state = base_state();
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });

    let result = play_summon(&state, PlayerId::One, CardInstanceId(2), BenchSlot::First);

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn play_summon_rejects_an_occupied_bench_slot() {
    let mut state = base_state();
    let card = card_ref(2, "quarry-whelp");
    state.players.one.hand.push(card);
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "quarry-whelp"));

    let result = play_summon(&state, PlayerId::One, card.instance, BenchSlot::First);

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

#[test]
fn play_summon_rejects_a_card_not_in_hand() {
    let state = base_state();

    let result = play_summon(&state, PlayerId::One, CardInstanceId(99), BenchSlot::First);

    assert_eq!(result, Err(ActionError::UnknownCard));
}

#[test]
fn play_summon_rejects_a_non_base_form() {
    let mut state = base_state();
    let card = card_ref(2, "quarry-brute");
    state.players.one.hand.push(card);

    let result = play_summon(&state, PlayerId::One, card.instance, BenchSlot::First);

    assert_eq!(result, Err(ActionError::UnknownCard));
}

// --- upgrade_summon --------------------------------------------------

#[test]
fn upgrade_summon_stacks_the_new_top_ready_false_and_upgraded_flag_set() {
    let mut state = base_state();
    let upgrade = card_ref(2, "quarry-brute");
    state.players.one.hand.push(upgrade);

    let outcome = upgrade_summon(&state, PlayerId::One, upgrade.instance, Position::Main)
        .expect("Base to Enhanced is a legal climb with matching Mana Types");

    let main = outcome
        .state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("Main still holds the Summon");
    assert_eq!(main.chain.top(), upgrade);
    assert_eq!(main.chain.base(), card_ref(1, "quarry-whelp"));
    assert_eq!(main.readiness, Readiness::Exhausted);
    assert_eq!(main.turn.upgrade, UpgradeActivity::UpgradedThisTurn);
    assert!(outcome.state.players.get(PlayerId::One).hand.is_empty());
    assert_eq!(
        outcome.events,
        vec![GameEvent::SummonUpgraded {
            player: PlayerId::One,
            card: upgrade.instance,
            position: Position::Main,
        }]
    )
}

#[test]
fn upgrade_summon_rejects_a_pending_decision() {
    let mut state = base_state();
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });

    let result = upgrade_summon(&state, PlayerId::One, CardInstanceId(2), Position::Main);

    assert_eq!(result, Err(ActionError::PendingInputMismatch));
}

#[test]
fn upgrade_summon_rejects_outside_main_phase() {
    let mut state = base_state();
    state.turn.phase = Phase::Upkeep;

    let result = upgrade_summon(&state, PlayerId::One, CardInstanceId(2), Position::Main);

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn upgrade_summon_rejects_an_empty_position() {
    let state = base_state();

    let result = upgrade_summon(
        &state,
        PlayerId::One,
        CardInstanceId(2),
        Position::Bench(BenchSlot::First),
    );

    assert_eq!(result, Err(ActionError::EmptyPosition));
}

#[test]
fn upgrade_summon_rejects_a_summon_played_this_turn() {
    let mut state = base_state();
    let upgrade = card_ref(2, "quarry-brute");
    state.players.one.hand.push(upgrade);
    let Some(main) = &mut state.players.one.main else {
        unreachable!("fixture always sets up a Main Summon")
    };
    main.turn.upgrade = UpgradeActivity::PlayedThisTurn;

    let result = upgrade_summon(&state, PlayerId::One, upgrade.instance, Position::Main);

    assert_eq!(result, Err(ActionError::PlayedThisTurn));
}

#[test]
fn upgrade_summon_rejects_a_summon_already_upgraded_this_turn() {
    let mut state = base_state();
    let upgrade = card_ref(2, "quarry-brute");
    state.players.one.hand.push(upgrade);
    let Some(main) = &mut state.players.one.main else {
        unreachable!("fixture always sets up a Main Summon")
    };
    main.turn.upgrade = UpgradeActivity::UpgradedThisTurn;

    let result = upgrade_summon(&state, PlayerId::One, upgrade.instance, Position::Main);

    assert_eq!(result, Err(ActionError::AlreadyUpgradedThisTurn));
}

#[test]
fn upgrade_summon_rejects_a_form_that_does_not_climb() {
    let mut state = base_state();
    let same_form = card_ref(2, "set-path-adept");
    state.players.one.hand.push(same_form);

    let result = upgrade_summon(&state, PlayerId::One, same_form.instance, Position::Main);

    assert_eq!(result, Err(ActionError::IllegalUpgradeTarget));
}

#[test]
fn upgrade_summon_rejects_a_higher_form_from_a_different_lineage() {
    // Main holds a Set-Path Adept (Produces Matter, Mind). Quarry Brute
    // climbs Base -> Enhanced but only produces Matter, dropping Mind —
    // rules §18's Mana-Type-superset requirement, not just Form order.
    let mut state = base_state();
    state.players.one.main = Some(base_summon(PlayerId::One, "set-path-adept"));
    let cross_lineage = card_ref(2, "quarry-brute");
    state.players.one.hand.push(cross_lineage);

    let result = upgrade_summon(
        &state,
        PlayerId::One,
        cross_lineage.instance,
        Position::Main,
    );

    assert_eq!(result, Err(ActionError::IllegalUpgradeTarget));
}

#[test]
fn upgrade_summon_rejects_a_card_not_in_hand() {
    let state = base_state();

    let result = upgrade_summon(&state, PlayerId::One, CardInstanceId(99), Position::Main);

    assert_eq!(result, Err(ActionError::UnknownCard));
}

// --- retreat ----------------------------------------------------------

#[test]
fn retreat_pays_the_printed_cost_and_swaps_main_with_the_chosen_bench_slot() {
    let mut state = base_state();
    state.players.one.mana = ManaBank {
        matter: 5,
        mind: 0,
        spirit: 0,
    };
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

    let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
        .expect("the bank can cover Quarry Whelp's printed Retreat Cost of 1");

    let player = outcome.state.players.get(PlayerId::One);
    assert_eq!(
        player.main.as_ref().map(|s| s.chain.base().def),
        Some(fixtures::id("set-path-adept"))
    );
    assert_eq!(
        player.bench[0].as_ref().map(|s| s.chain.base().def),
        Some(fixtures::id("quarry-whelp"))
    );
    assert_eq!(
        player.mana,
        ManaBank {
            matter: 4,
            mind: 0,
            spirit: 0,
        }
    );
    assert!(outcome.state.turn.normal_retreat_used);

    // `retreat` itself only queues the four movement triggers (rules
    // §28); it never sets the Main-entry record directly. Draining
    // the queue runs the `EnteringMain` step through
    // `engine::triggers::movement_trigger`, the one place that does.
    let (drained, _) = crate::engine::resolution::drain(&outcome.state);
    let drained_player = drained.players.get(PlayerId::One);
    assert!(
        drained_player
            .main
            .as_ref()
            .is_some_and(|s| s.turn.main_entry.is_some())
    );
}

#[test]
fn retreat_costs_more_when_the_opponent_controls_the_warden_of_set_paths() {
    // The opponent's Warden of Set Paths prints a Passive raising this
    // Retreat's cost by 1 (rules §16). Quarry Whelp's printed Retreat
    // Cost is 1, all-Generic, so the adjusted cost is 2.
    let mut state = base_state();
    state.players.one.mana = ManaBank {
        matter: 5,
        mind: 0,
        spirit: 0,
    };
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
    state.players.two.main = Some(base_summon(PlayerId::Two, "warden-of-set-paths"));

    let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
        .expect("the bank covers Quarry Whelp's printed Retreat Cost of 1 plus the Warden's +1");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 2,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
        ]
    );
    assert_eq!(
        outcome.state.players.get(PlayerId::One).mana,
        ManaBank {
            matter: 3,
            mind: 0,
            spirit: 0,
        }
    );
}

#[test]
fn retreat_emits_mana_deducted_then_summons_swapped() {
    let mut state = base_state();
    state.players.one.mana = ManaBank {
        matter: 5,
        mind: 0,
        spirit: 0,
    };
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

    let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
        .expect("the bank can cover the printed Retreat Cost");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
        ]
    );
}

#[test]
fn retreat_enqueues_the_four_movement_triggers_in_fixed_order() {
    let mut state = base_state();
    state.players.one.mana = ManaBank {
        matter: 5,
        mind: 0,
        spirit: 0,
    };
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

    let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
        .expect("the bank can cover the printed Retreat Cost");

    let work: Vec<_> = outcome.state.work.into_iter().collect();
    assert_eq!(
        work,
        vec![
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
        ]
    );
}

#[test]
fn retreat_rejects_a_pending_decision() {
    let mut state = base_state();
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });

    let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

    assert_eq!(result, Err(ActionError::PendingInputMismatch));
}

#[test]
fn retreat_rejects_outside_main_phase() {
    let mut state = base_state();
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
    state.turn.phase = Phase::Combat;

    let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn retreat_rejects_a_second_normal_retreat_this_turn() {
    let mut state = base_state();
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
    state.turn.normal_retreat_used = true;

    let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

    assert_eq!(result, Err(ActionError::NormalRetreatAlreadyUsed));
}

#[test]
fn retreat_rejects_an_empty_bench_slot() {
    let state = base_state();

    let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

    assert_eq!(result, Err(ActionError::EmptyPosition));
}

#[test]
fn retreat_reports_a_mana_shortfall() {
    let mut state = base_state();
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
    // Quarry Whelp's Retreat Cost is 1, all-Generic; the bank is empty.

    let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

    assert_eq!(
        result,
        Err(ActionError::InsufficientMana {
            short: ManaBank {
                matter: 1,
                mind: 0,
                spirit: 0
            }
        })
    );
}

#[test]
fn retreat_rejects_a_hint_naming_an_empty_pool() {
    let mut state = base_state();
    state.players.one.mana = ManaBank {
        matter: 5,
        mind: 0,
        spirit: 0,
    };
    state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

    let result = retreat(
        &state,
        PlayerId::One,
        BenchSlot::First,
        Some(ManaType::Spirit),
    );

    assert_eq!(result, Err(ActionError::InvalidManaHint));
}

#[test]
fn a_rejected_retreat_leaves_the_callers_state_untouched() {
    let state = base_state();
    let before = state.clone();

    let _ = retreat(&state, PlayerId::One, BenchSlot::First, None);

    assert_eq!(state, before);
}

// --- end to end, through the public `apply` entry point ---------------

#[test]
fn play_upgrade_then_retreat_chain_through_scenario_and_apply() {
    use crate::domain::actions::GameAction;
    use crate::engine::apply::apply;
    use crate::scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario};

    let scenario = Scenario {
        players: crate::domain::state::PerPlayer::new(
            ScenarioPlayer {
                deck: vec![],
                hand: vec![
                    card_ref(2, "set-path-adept"),
                    card_ref(3, "quarry-brute"),
                    card_ref(4, "colossus-of-the-quarry"),
                ],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank {
                    matter: 5,
                    mind: 0,
                    spirit: 0,
                },
                main_losses: 0,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(1, "quarry-whelp")],
                    damage: 0,
                    readiness: Readiness::Ready,
                }),
                bench: [None, None, None],
            },
            ScenarioPlayer {
                deck: vec![card_ref(102, "set-path-adept")],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                main_losses: 0,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(101, "set-path-adept")],
                    damage: 0,
                    readiness: Readiness::Ready,
                }),
                bench: [None, None, None],
            },
        ),
        active_player: PlayerId::One,
        coin: None,
    };
    let state =
        from_scenario(fixtures::card_set(), &scenario).expect("this board is a legal scenario");
    assert_eq!(
        state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("the scenario has a Main Summon")
            .turn,
        crate::domain::state::SummonTurnRecord::fresh()
    );

    // 1. Play the Base Set-Path Adept from hand onto the empty Bench.
    let outcome = apply(
        &state,
        &GameAction::PlaySummon {
            player: PlayerId::One,
            card: CardInstanceId(2),
            slot: BenchSlot::First,
        },
    )
    .expect("the Bench slot is empty and the card is a Base Summon in hand");
    assert_eq!(
        outcome.events,
        vec![GameEvent::SummonPlayed {
            player: PlayerId::One,
            card: CardInstanceId(2),
            slot: BenchSlot::First,
        }]
    );
    assert_eq!(
        outcome.state.players.get(PlayerId::One).bench[0]
            .as_ref()
            .expect("the played Summon is on the Bench")
            .turn
            .upgrade,
        UpgradeActivity::PlayedThisTurn
    );

    let rejected_played_upgrade = apply(
        &outcome.state,
        &GameAction::UpgradeSummon {
            player: PlayerId::One,
            card: CardInstanceId(3),
            position: Position::Bench(BenchSlot::First),
        },
    );
    assert_eq!(rejected_played_upgrade, Err(ActionError::PlayedThisTurn));

    // 2. Upgrade the Main Quarry Whelp with the Quarry Brute in hand.
    let outcome = apply(
        &outcome.state,
        &GameAction::UpgradeSummon {
            player: PlayerId::One,
            card: CardInstanceId(3),
            position: Position::Main,
        },
    )
    .expect("Base to Enhanced climbs with matching Mana Types and nothing upgraded yet");
    assert_eq!(
        outcome.events,
        vec![GameEvent::SummonUpgraded {
            player: PlayerId::One,
            card: CardInstanceId(3),
            position: Position::Main,
        }]
    );
    assert_eq!(
        outcome
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .map(|s| s.chain.top().def),
        Some(fixtures::id("quarry-brute"))
    );
    assert_eq!(
        outcome
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("the upgraded Summon remains in Main")
            .turn
            .upgrade,
        UpgradeActivity::UpgradedThisTurn
    );

    let rejected_second_upgrade = apply(
        &outcome.state,
        &GameAction::UpgradeSummon {
            player: PlayerId::One,
            card: CardInstanceId(4),
            position: Position::Main,
        },
    );
    assert_eq!(
        rejected_second_upgrade,
        Err(ActionError::AlreadyUpgradedThisTurn)
    );

    // 3. Retreat: pay Quarry Brute's printed Retreat Cost of 2 and swap
    // Main with the newly played Set-Path Adept.
    let outcome = apply(
        &outcome.state,
        &GameAction::Retreat {
            player: PlayerId::One,
            slot: BenchSlot::First,
            mana_hint: None,
        },
    )
    .expect("the bank covers the printed Retreat Cost of 2");
    assert_eq!(
        outcome.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 2,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
        ]
    );

    let one = outcome.state.players.get(PlayerId::One);
    assert_eq!(
        one.main.as_ref().map(|s| s.chain.base().def),
        Some(fixtures::id("set-path-adept"))
    );
    assert!(
        one.main
            .as_ref()
            .is_some_and(|s| s.turn.main_entry.is_some())
    );
    assert_eq!(
        one.bench[0].as_ref().map(|s| s.chain.top().def),
        Some(fixtures::id("quarry-brute"))
    );
    assert_eq!(
        one.mana,
        ManaBank {
            matter: 3,
            mind: 0,
            spirit: 0,
        }
    );
    assert!(outcome.state.turn.normal_retreat_used);

    // 4. End the turn through the public response window. Both passes hand
    // play to Player Two and reset every Summon's full turn record.
    let outcome = apply(
        &outcome.state,
        &GameAction::EndTurn {
            player: PlayerId::One,
        },
    )
    .expect("the active player can end a resting Main Phase");
    let outcome = apply(
        &outcome.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds the final response window first");
    let outcome = apply(
        &outcome.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the second consecutive pass hands the turn over");

    for player in [PlayerId::One, PlayerId::Two] {
        let player_state = outcome.state.players.get(player);
        for summon in player_state
            .main
            .iter()
            .chain(player_state.bench.iter().flatten())
        {
            assert_eq!(
                summon.turn,
                crate::domain::state::SummonTurnRecord::fresh(),
                "{player:?}"
            );
        }
    }
}
