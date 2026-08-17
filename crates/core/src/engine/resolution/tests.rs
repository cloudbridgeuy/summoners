//! Split out of mod.rs to stay under the file length cap; shares that
//! module's items through `super::*`.

use super::*;
use crate::domain::cards::fixtures;
use crate::domain::ids::{CardInstanceId, PlayerId, Position};
use crate::domain::state::{
    CardRef, GameStatus, ManaBank, ManaSource, MovementStep, PendingInput, PerPlayer, Phase,
    PlayerState, SummonInstance, TurnState, UpgradeChain,
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
        deck: vec![CardRef {
            instance: CardInstanceId(10),
            def: fixtures::id("quarry-whelp"),
        }],
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
            active_player: PlayerId::Two,
            phase: Phase::Upkeep,
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

#[test]
fn apply_leaves_skips_a_paired_conditional_bonus_when_deal_damage_is_immutable() {
    // Rules §30, the Old Sow's `Root and Renew`-adjacent Attack text:
    // "her attack damage can be neither increased nor prevented." No
    // registry fixture pairs an immutable `DealDamage` with a
    // `ConditionalBonus` today (the Old Sow's own Attack is a lone
    // immutable `DealDamage`), so this test builds the pairing directly
    // to prove the gate holds even if a future fixture combines them.
    let mut state = base_state();
    state.turn.spell_played_this_turn = true;
    let leaves = vec![
        EffectLeaf::DealDamage {
            amount: 70,
            immutable: true,
        },
        EffectLeaf::ConditionalBonus {
            condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
            amount: 40,
        },
    ];

    let (state, events) = apply_leaves(&state, PlayerId::One, &[Position::Main], &leaves);

    assert_eq!(
        events,
        vec![GameEvent::DamageApplied {
            position: Position::Main,
            before: 0,
            after: 70,
        }],
        "only the immutable DealDamage's own event lands; the bonus \
         leaf runs no code, not even to find its own condition false"
    );
    assert_eq!(
        state
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
fn apply_leaves_lets_a_paired_conditional_bonus_add_to_mutable_damage() {
    // The control case: the same pairing, but `DealDamage` is not
    // marked immutable, so the bonus adds normally (the Warden's and
    // the Griefsinger's own Attack fixtures both pair the two this way
    // already; this test isolates the pairing on its own).
    let mut state = base_state();
    state.turn.spell_played_this_turn = true;
    let leaves = vec![
        EffectLeaf::DealDamage {
            amount: 50,
            immutable: false,
        },
        EffectLeaf::ConditionalBonus {
            condition: crate::domain::cards::EffectCondition::SpellPlayedThisTurn,
            amount: 40,
        },
    ];

    let (state, _events) = apply_leaves(&state, PlayerId::One, &[Position::Main], &leaves);

    assert_eq!(
        state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        90,
        "50 from DealDamage plus 40 from the bonus"
    );
}

#[test]
fn drain_runs_the_full_upkeep_sequence_in_order() {
    let mut state = base_state();
    state.work = VecDeque::from(vec![
        WorkItem::ReadyAll,
        WorkItem::DrawCard,
        WorkItem::ProduceMana(ManaSource::Player),
    ]);

    let (state, events) = drain(&state);

    assert!(state.work.is_empty());
    assert_eq!(
        events,
        vec![
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: CardInstanceId(10),
            },
            GameEvent::ManaProduced {
                player: PlayerId::Two,
                source: ManaSource::Player,
                mana_type: crate::domain::ids::ManaType::Matter,
            },
        ]
    );
}

#[test]
fn drain_stops_as_soon_as_produce_mana_pauses_on_a_choice() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: fixtures::id("set-path-adept"),
            },
            vec![],
        ),
        ..whelp(PlayerId::Two)
    });
    state.work = VecDeque::from(vec![
        WorkItem::ReadyAll,
        WorkItem::DrawCard,
        WorkItem::ProduceMana(ManaSource::Player),
    ]);

    let (state, events) = drain(&state);

    assert_eq!(
        state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Player,
        })
    );
    assert!(state.work.is_empty());
    assert_eq!(
        events.len(),
        2,
        "ReadyAll and DrawCard ran; production paused before its own event"
    );
}

#[test]
fn drain_stops_immediately_once_a_draw_failure_sets_status() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).deck = vec![];
    state.work = VecDeque::from(vec![
        WorkItem::ReadyAll,
        WorkItem::DrawCard,
        WorkItem::ProduceMana(ManaSource::Player),
    ]);

    let (state, events) = drain(&state);

    assert!(!state.status.is_playing());
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::ProduceMana(ManaSource::Player)]),
        "the loop checks status before popping the next item, so the \
         unrun item is left queued rather than executed — harmless, since \
         a finished game rejects every later action outright"
    );
    assert_eq!(
        events,
        vec![
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Main],
            },
            GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: crate::domain::state::LossReason::EmptyDeckDraw,
            },
        ],
        "CardDrawn never fires and ManaProduced never runs after the loss"
    );
}

#[test]
fn drain_leaves_a_finished_game_untouched_even_with_queued_work() {
    let mut state = base_state();
    state.status = GameStatus::Ended(crate::domain::state::GameOutcome {
        winner: PlayerId::One,
        reason: crate::domain::state::LossReason::EmptyDeckDraw,
    });
    state.work = VecDeque::from(vec![WorkItem::ReadyAll]);

    let (state, events) = drain(&state);

    assert!(events.is_empty());
    assert_eq!(state.work, VecDeque::from(vec![WorkItem::ReadyAll]));
}

#[test]
fn drain_leaves_a_broken_game_untouched_even_with_queued_work() {
    let breakage = crate::domain::cards::Breakage {
        rule: "destruction",
        entity: fixtures::id("quarry-whelp"),
        expected: crate::domain::cards::ComponentKind::Life,
    };
    let mut state = base_state();
    state.status = GameStatus::Broken(breakage);
    state.work = VecDeque::from(vec![WorkItem::ReadyAll]);

    let (state, events) = drain(&state);

    assert!(events.is_empty());
    assert_eq!(state.status, GameStatus::Broken(breakage));
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::ReadyAll]),
        "a broken status stops the loop before it ever pops the next item"
    );
}

#[test]
fn drain_is_a_silent_no_op_for_a_movement_or_ability_trigger_with_no_matching_card() {
    // Quarry Whelp carries no `CardNode::Trigger`, so both items find
    // nothing to fire; `LeavingMain` also does not touch
    // `entered_main_this_turn` (only `EnteringMain` does).
    let mut state = base_state();
    state.work = VecDeque::from(vec![
        WorkItem::MovementTrigger(MovementStep::LeavingMain, PlayerId::Two, Position::Main),
        WorkItem::FireTrigger(
            PlayerId::Two,
            Position::Main,
            crate::domain::cards::TriggerEvent::YourUpkeep,
            fixtures::trigger_id("dawn-tender"),
        ),
    ]);

    let (state, events) = drain(&state);

    assert!(events.is_empty());
    assert!(state.work.is_empty());
    assert_eq!(state.status, GameStatus::Playing);
}

#[test]
fn drain_fires_an_entering_main_trigger_and_sets_the_entered_flag() {
    // Rules §28, §36, §39: Hearth Warden's immediate trigger heals
    // itself the moment it enters Main.
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: fixtures::id("hearth-warden"),
            },
            vec![],
        ),
        damage: 20,
        ..whelp(PlayerId::Two)
    });
    state.work = VecDeque::from(vec![WorkItem::MovementTrigger(
        MovementStep::EnteringMain,
        PlayerId::Two,
        Position::Main,
    )]);

    let (state, events) = drain(&state);

    assert_eq!(
        events,
        vec![
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Main,
                event: crate::domain::cards::TriggerEvent::EntersMain,
                ability: fixtures::trigger_id("hearth-warden"),
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ]
    );
    let healed = state
        .players
        .get(PlayerId::Two)
        .main
        .as_ref()
        .expect("main");
    assert_eq!(healed.damage, 5);
    assert!(healed.entered_main_this_turn);
}

#[test]
fn drain_opens_a_window_for_a_respondable_trigger_and_resumes_the_interrupted_drain_below_it() {
    // Rules §38, §40: a respondable destruction trigger creates a
    // segment base and opens a window for the opponent of its
    // controller; the drain stops there instead of running the rest
    // of `work` or touching the Stack.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(3),
                def: fixtures::id("spite-thorn"),
            },
            vec![],
        ),
        ..whelp(PlayerId::One)
    });
    state.work = VecDeque::from(vec![
        WorkItem::FireTrigger(
            PlayerId::One,
            Position::Main,
            crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            fixtures::trigger_id("spite-thorn"),
        ),
        WorkItem::LossCheck(PlayerId::Two),
    ]);

    let (state, events) = drain(&state);

    assert_eq!(
        events,
        vec![GameEvent::TriggerFired {
            controller: PlayerId::One,
            position: Position::Main,
            event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            ability: fixtures::trigger_id("spite-thorn"),
        }]
    );
    assert_eq!(state.stack.len(), 1);
    assert_eq!(state.stack_segment_bases, vec![0]);
    assert_eq!(
        state.turn.window,
        Some(crate::domain::state::StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)]),
        "the interrupted item stays queued below the open segment"
    );
}

#[test]
fn drain_resumes_the_interrupted_work_once_two_passes_close_the_triggers_window() {
    // Rules §38, §40: once the opponent of the trigger's controller and
    // then the controller both pass, the segment the trigger opened
    // resolves — top-first, like any other Stack item — and only then
    // does the destruction chain it interrupted continue underneath it.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(3),
                def: fixtures::id("spite-thorn"),
            },
            vec![],
        ),
        ..whelp(PlayerId::One)
    });
    state.work = VecDeque::from(vec![
        WorkItem::FireTrigger(
            PlayerId::One,
            Position::Main,
            crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            fixtures::trigger_id("spite-thorn"),
        ),
        WorkItem::LossCheck(PlayerId::Two),
    ]);

    let (state, _opening_events) = drain(&state);
    // The window opened for the opponent of the trigger's controller
    // (rules §38); both must pass in a row to close it (rules §32–33).
    let after_first_pass = crate::engine::stack::pass(&state, PlayerId::Two)
        .expect("Two holds Priority first, opposite the trigger's controller");
    let after_second_pass = crate::engine::stack::pass(&after_first_pass.state, PlayerId::One)
        .expect("One passes second, closing the window");
    assert_eq!(
        after_second_pass.state.turn.window, None,
        "the window is closed, but the Stack still holds the Trigger \
         item, so no handover runs yet"
    );

    let (state, events) = drain(&after_second_pass.state);

    assert_eq!(
        events,
        vec![
            GameEvent::StackItemResolved {
                item: StackItem::Trigger {
                    controller: PlayerId::One,
                    source: Position::Main,
                    event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
                    targets: vec![Position::Main],
                    effects: vec![crate::domain::cards::EffectLeaf::DealDamage {
                        amount: 15,
                        immutable: false,
                    }],
                },
            },
            GameEvent::DamageApplied {
                position: Position::Main,
                before: 0,
                after: 15,
            },
        ],
        "the segment's own Trigger item resolved, dealing its Damage to \
         Two's Main"
    );
    assert!(
        state.stack.is_empty(),
        "the segment's item is the only thing on the Stack"
    );
    assert!(
        state.stack_segment_bases.is_empty(),
        "the segment's base came off once its item resolved"
    );
    assert!(
        state.work.is_empty(),
        "the interrupted LossCheck, and the DestructionCheck the \
         Trigger's Damage enqueued, both ran once the segment settled"
    );
    assert_eq!(
        state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        15,
        "15 Damage lands, short of Quarry Whelp's 40 Life, so nothing \
         is destroyed and the chain stays quiet"
    );
}

#[test]
fn drain_runs_a_loss_check_that_finds_no_losing_condition_as_a_silent_no_op() {
    let mut state = base_state();
    state.work = VecDeque::from(vec![WorkItem::LossCheck(PlayerId::One)]);

    let (state, events) = drain(&state);

    assert!(events.is_empty());
    assert!(state.work.is_empty());
    assert_eq!(state.status, GameStatus::Playing);
}

// -- Stack resolution: step 2 of the loop ---------------------------------

#[test]
fn drain_leaves_the_stack_untouched_while_a_window_is_still_open() {
    let mut state = base_state();
    state.turn.window = Some(crate::domain::state::StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });
    state.stack = vec![StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Main,
    }];

    let (state, events) = drain(&state);

    assert!(events.is_empty());
    assert_eq!(
        state.stack.len(),
        1,
        "step 2 only runs once the window has closed"
    );
}

#[test]
fn drain_resolves_the_stack_strictly_top_first_once_work_is_empty_and_the_window_is_closed() {
    let mut state = base_state();
    state.turn.window = None;
    state.stack = vec![
        StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        },
        StackItem::Attack {
            attacker: PlayerId::Two,
            target: Position::Main,
        },
    ];

    let (state, events) = drain(&state);

    assert!(state.stack.is_empty());
    assert!(
        state.work.is_empty(),
        "every DestructionCheck a resolution enqueued was drained too"
    );
    // Quarry Whelp deals 10; the item pushed last (Two attacking One) is
    // the top of the Stack and resolves first (rules §35).
    assert_eq!(
        events,
        vec![
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::Two,
                    target: Position::Main,
                },
            },
            GameEvent::DamageApplied {
                position: Position::Main,
                before: 0,
                after: 10,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            GameEvent::DamageApplied {
                position: Position::Main,
                before: 0,
                after: 10,
            },
        ]
    );
}

#[test]
fn drain_resolves_an_attack_on_an_empty_bench_position_as_a_miss() {
    let mut state = base_state();
    state.turn.window = None;
    state.stack = vec![StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Bench(crate::domain::ids::BenchSlot::First),
    }];

    let (state, events) = drain(&state);

    assert!(state.stack.is_empty());
    assert_eq!(
        events,
        vec![GameEvent::StackItemResolved {
            item: StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Bench(crate::domain::ids::BenchSlot::First),
            },
        }],
        "a miss still resolves the Stack item, but applies no Damage"
    );
    assert!(
        state.work.is_empty(),
        "nothing to check for destruction on an empty position"
    );
}

#[test]
fn drain_resolves_a_support_spell_and_discards_it_to_its_casters_pile() {
    let mut state = base_state();
    state.turn.window = None;
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 15,
        ..whelp(PlayerId::One)
    });
    let card = CardRef {
        instance: CardInstanceId(99),
        def: fixtures::id("renewing-balm"),
    };
    state.stack = vec![StackItem::Spell {
        caster: PlayerId::One,
        card,
        targets: vec![Position::Main],
    }];

    let (state, events) = drain(&state);

    assert!(state.stack.is_empty());
    assert_eq!(
        events,
        vec![
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card,
                    targets: vec![Position::Main],
                },
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ]
    );
    assert_eq!(state.players.get(PlayerId::One).discard, vec![card]);
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
fn drain_resolves_an_attack_spell_against_the_opponents_main() {
    let mut state = base_state();
    state.turn.window = None;
    let card = CardRef {
        instance: CardInstanceId(99),
        def: fixtures::id("ember-lance"),
    };
    state.stack = vec![StackItem::Spell {
        caster: PlayerId::One,
        card,
        targets: vec![Position::Main],
    }];

    let (state, events) = drain(&state);

    assert_eq!(
        events,
        vec![
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card,
                    targets: vec![Position::Main],
                },
            },
            GameEvent::DamageApplied {
                position: Position::Main,
                before: 0,
                after: 10,
            },
        ]
    );
    assert_eq!(state.players.get(PlayerId::One).discard, vec![card]);
    assert_eq!(
        state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main")
            .damage,
        10
    );
}

#[test]
fn drain_resolves_a_draw_spell_and_settles_the_loss_check_it_enqueues() {
    let mut state = base_state();
    state.turn.window = None;
    let card = CardRef {
        instance: CardInstanceId(99),
        def: fixtures::id("scrying-glass"),
    };
    state.stack = vec![StackItem::Spell {
        caster: PlayerId::Two,
        card,
        targets: vec![],
    }];

    let (state, events) = drain(&state);

    assert_eq!(
        events,
        vec![
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::Two,
                    card,
                    targets: vec![],
                },
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: CardInstanceId(10),
            },
        ]
    );
    assert_eq!(state.players.get(PlayerId::Two).discard, vec![card]);
    assert!(
        state.work.is_empty(),
        "the LossCheck the draw enqueued was drained too"
    );
}
