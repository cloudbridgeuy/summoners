//! Integration-style demo tests that drive `apply` end to end through a
//! full action sequence (declare/pass/pass, cast/pass/pass, a full turn
//! handoff), rather than exercising one handler in isolation. Split out of
//! `tests.rs` to stay under the file length cap; shares that module's
//! fixtures through `super::*`.

use super::*;
use crate::domain::state::Coin;
use crate::scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario};

// -- Demo: EndTurn hands off, runs the opponent's Upkeep, and Mana
// production either auto-produces or pauses for ChooseManaType;
// ConvertCoin banks one anchored Mana and is one-use; an empty-deck
// draw ends the game immediately. ---------------------------------------

#[test]
fn demo_end_turn_runs_a_mono_type_upkeep_without_pausing() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).deck = vec![card_ref(100, "quarry-whelp")];

    let outcome =
        full_end_turn(&state, PlayerId::One).expect("legal from a resting Main, both pass");

    assert_eq!(outcome.state.pending, None);
    assert_eq!(outcome.state.turn.active_player, PlayerId::Two);
    assert_eq!(outcome.state.players.get(PlayerId::Two).mana.matter, 1);
    assert_eq!(
        outcome.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: CardInstanceId(100),
            },
            GameEvent::ManaProduced {
                player: PlayerId::Two,
                source: ManaSource::Player,
                mana_type: ManaType::Matter,
            },
        ]
    );
}

#[test]
fn demo_end_turn_pauses_on_a_dual_type_board_and_choose_mana_type_answers_it() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(card_ref(200, "set-path-adept"), vec![]),
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).deck = vec![card_ref(201, "quarry-whelp")];

    let outcome =
        full_end_turn(&state, PlayerId::One).expect("legal from a resting Main, both pass");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: CardInstanceId(201),
            },
        ],
        "Set-Path Adept produces both Matter and Mind, so production pauses \
         before its own event"
    );
    assert_eq!(
        outcome.state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Player,
        })
    );
    assert_eq!(
        outcome.state.players.get(PlayerId::Two).mana,
        ManaBank::default()
    );

    let answered = apply(
        &outcome.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Mind,
        },
    )
    .expect("Mind is anchored to Set-Path Adept");

    assert_eq!(answered.state.pending, None);
    assert_eq!(answered.state.players.get(PlayerId::Two).mana.mind, 1);
    assert_eq!(
        answered.events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Player,
            mana_type: ManaType::Mind,
        }]
    );
}

#[test]
fn demo_convert_coin_banks_one_anchored_mana_and_removes_the_coin() {
    fn player(instance: u32) -> ScenarioPlayer {
        ScenarioPlayer {
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            main: Some(ScenarioSummon {
                chain: vec![card_ref(instance, "quarry-whelp")],
                damage: 0,
                ready: true,
            }),
            bench: [None, None, None],
        }
    }

    let mut scenario = Scenario {
        players: PerPlayer::new(player(301), player(302)),
        active_player: PlayerId::One,
        coin: Some(Coin),
    };
    let state = from_scenario(fixtures::card_set(), &scenario).expect("the Coin scenario parses");

    assert_eq!(state.coin, Some(Coin));
    assert_eq!(
        apply(
            &state,
            &GameAction::ConvertCoin {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
            },
        ),
        Err(ActionError::InvalidTarget)
    );

    scenario.active_player = PlayerId::Two;
    let state = from_scenario(fixtures::card_set(), &scenario).expect("the Coin scenario parses");

    let outcome = apply(
        &state,
        &GameAction::ConvertCoin {
            player: PlayerId::Two,
            mana_type: ManaType::Matter,
        },
    )
    .expect("anchored, owner's own Main Phase");

    assert_eq!(
        outcome.events,
        vec![GameEvent::CoinConverted {
            player: PlayerId::Two,
            mana_type: ManaType::Matter,
        }]
    );
    let player_state = outcome.state.players.get(PlayerId::Two);
    assert_eq!(player_state.mana.matter, 1);
    assert_eq!(outcome.state.coin, None);

    // The Coin is one-use: converting again with no Coin left to spend
    // is rejected.
    assert_eq!(
        apply(
            &outcome.state,
            &GameAction::ConvertCoin {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
            },
        ),
        Err(ActionError::InvalidTarget)
    );
}

#[test]
fn demo_declare_pass_pass_resolves_the_attack_through_apply() {
    // The full §29–35 chain driven only through `apply`: declaring opens
    // the window with the defender first (rules §31, §46), each side
    // passes once, and the second pass both closes the window and lets
    // `engine::resolution::drain` resolve the Attack now sitting on top
    // of the Stack — all inside that same `apply` call, since nothing
    // is left to respond to it.
    let state = base_state();

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("Quarry Whelp's Attack is free and Main is a legal target");

    assert_eq!(
        declared.events,
        vec![GameEvent::AttackDeclared {
            player: PlayerId::One,
            target: Position::Main,
        }],
        "the free cost pays without emitting ManaDeducted"
    );
    assert!(declared.state.turn.normal_attack_used);
    assert_eq!(declared.state.turn.phase, Phase::Combat);
    assert_eq!(
        declared.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        }),
        "the defender holds Priority first"
    );
    assert_eq!(
        declared.state.stack,
        vec![StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        }]
    );

    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority");

    assert_eq!(
        defender_passed.events,
        vec![GameEvent::PriorityPassed {
            player: PlayerId::Two
        }],
        "one pass never resolves anything by itself"
    );
    assert!(!defender_passed.state.stack.is_empty());

    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the attacker holds Priority second");

    assert_eq!(
        resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                }
            },
            GameEvent::DamageApplied {
                position: Position::Main,
                before: 0,
                after: 10,
            },
        ],
        "the double pass both closes the window and drains the Attack \
         it left on top of the Stack, in that order"
    );
    assert!(resolved.state.stack.is_empty());
    assert_eq!(resolved.state.turn.window, None);
    assert_eq!(
        resolved
            .state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("Two's Main summon is still there")
            .damage,
        10
    );
}

#[test]
fn demo_a_lethal_attack_destroys_recovers_a_prize_and_promotes_through_apply() {
    // The full §24/§25/§41 chain, driven only through `apply`: a lethal
    // hit destroys Two's Main, Two's opponent (One) chooses which Prize
    // Two recovers, then Two chooses which Bench Summon is promoted.
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        damage: 30, // ten more finishes Quarry Whelp's printed Life of 40.
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).prizes = vec![card_ref(2, "quarry-whelp")];
    state.players.get_mut(PlayerId::Two).bench = [
        Some(summon(PlayerId::Two)),
        Some(summon(PlayerId::Two)),
        None,
    ];

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("Quarry Whelp's Attack is free and Main is a legal target");
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the attacker holds Priority second");

    assert_eq!(
        resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                }
            },
            GameEvent::DamageApplied {
                position: Position::Main,
                before: 30,
                after: 40,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            },
        ],
        "the destruction chain discards and records the loss, then pauses \
         on Prize recovery before Promotion or the loss check ever run"
    );
    assert_eq!(resolved.state.players.get(PlayerId::Two).main, None);
    assert_eq!(resolved.state.players.get(PlayerId::Two).main_losses, 1);
    assert_eq!(
        resolved.state.players.get(PlayerId::Two).discard,
        vec![card_ref(1, "quarry-whelp")]
    );
    assert_eq!(
        resolved.state.pending,
        Some(PendingInput::PrizePick {
            chooser: PlayerId::One
        }),
        "rules §25: the opponent of the player recovering the Prize chooses"
    );

    let prize_picked = apply(
        &resolved.state,
        &GameAction::ChoosePrize {
            player: PlayerId::One,
            prize_index: 0,
        },
    )
    .expect("a Prize is pending");

    assert_eq!(
        prize_picked.events,
        vec![GameEvent::PrizeRecovered {
            player: PlayerId::Two,
            card: CardInstanceId(2),
        }],
        "recovering the Prize resumes the chain into Promotion, which \
         pauses again with two Bench Summons to choose from"
    );
    assert_eq!(prize_picked.state.players.get(PlayerId::Two).prizes, vec![]);
    assert_eq!(
        prize_picked.state.players.get(PlayerId::Two).hand,
        vec![card_ref(2, "quarry-whelp")]
    );
    assert_eq!(
        prize_picked.state.pending,
        Some(PendingInput::Promotion {
            player: PlayerId::Two
        })
    );

    let promoted = apply(
        &prize_picked.state,
        &GameAction::ChoosePromotion {
            player: PlayerId::Two,
            slot: BenchSlot::First,
        },
    )
    .expect("a Promotion is pending");

    assert_eq!(
        promoted.events,
        vec![GameEvent::SummonPromoted {
            player: PlayerId::Two,
            from: BenchSlot::First,
        }],
        "the two queued movement-trigger steps and the final loss check \
         all run silently: no loss condition applies with a fresh Main"
    );
    assert_eq!(promoted.state.pending, None);
    assert_eq!(promoted.state.status, GameStatus::Playing);
    assert!(promoted.state.work.is_empty());
    let two = promoted.state.players.get(PlayerId::Two);
    assert!(two.main.is_some(), "Promotion filled the empty Main");
    assert!(
        two.main
            .as_ref()
            .expect("checked above")
            .entered_main_this_turn
    );
    assert_eq!(two.bench, [None, Some(summon(PlayerId::Two)), None]);
}

#[test]
fn demo_cast_pass_pass_resolves_a_support_spell_and_discards_it_through_apply() {
    // Design decision 16: a Support Spell cast in the caster's own Main
    // Phase pays its cost, goes on the Stack, and opens a Priority
    // window for the opponent (rules §34, §45); a double pass then
    // resolves it, emitting `SpellCast` at cast time and
    // `StackItemResolved` plus `Healed` at resolution, and the Spell
    // goes to its caster's discard (rules §22, §56).
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 15,
        ..summon(PlayerId::One)
    });
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(5, "renewing-balm")];
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(5),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("Renewing Balm is affordable and legal in the caster's own resting Main");

    assert_eq!(
        cast.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SpellCast {
                player: PlayerId::One,
                card: CardInstanceId(5),
                targets: vec![Position::Main],
            },
        ]
    );
    assert_eq!(
        cast.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
    assert!(cast.state.players.get(PlayerId::One).hand.is_empty());

    let defender_passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority");
    assert!(!defender_passed.state.stack.is_empty());

    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster holds Priority second");

    assert_eq!(
        resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card: card_ref(5, "renewing-balm"),
                    targets: vec![Position::Main],
                },
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ]
    );
    assert!(resolved.state.stack.is_empty());
    assert_eq!(resolved.state.turn.window, None);
    assert_eq!(
        resolved
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("One's Main summon is still there")
            .damage,
        0
    );
    assert_eq!(
        resolved.state.players.get(PlayerId::One).discard,
        vec![card_ref(5, "renewing-balm")]
    );
}

#[test]
fn demo_an_attack_spell_cast_in_response_resolves_before_the_attack_through_apply() {
    // An Attack Spell cast as a response is last onto the Stack, so it
    // resolves first — last in, first out (rules §35) — before the
    // attack it responded to.
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).hand = vec![card_ref(7, "ember-lance")];
    state.players.get_mut(PlayerId::Two).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("Quarry Whelp's Attack is free and Main is a legal target");
    assert_eq!(
        declared.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        }),
        "the defender holds Priority first"
    );

    let responded = apply(
        &declared.state,
        &GameAction::CastSpell {
            player: PlayerId::Two,
            card: CardInstanceId(7),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("Two holds Priority, so the Attack Spell is a legal response");

    assert_eq!(
        responded.state.stack,
        vec![
            StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            },
            StackItem::Spell {
                caster: PlayerId::Two,
                card: card_ref(7, "ember-lance"),
                targets: vec![Position::Main],
            },
        ]
    );
    assert_eq!(
        responded.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        }),
        "Priority moves back to the attacker after the response (rules §32)"
    );

    let attacker_passed = apply(
        &responded.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the attacker holds Priority");

    let resolved = apply(
        &attacker_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority second");

    assert_eq!(
        resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two
            },
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::Two,
                    card: card_ref(7, "ember-lance"),
                    targets: vec![Position::Main],
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
        ],
        "the Attack Spell resolves before the attack it responded to"
    );
    assert!(resolved.state.stack.is_empty());
    assert_eq!(
        resolved
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .damage,
        10,
        "the Attack Spell hit the original attacker back"
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
        10,
        "the normal attack still landed afterward"
    );
    assert_eq!(
        resolved.state.players.get(PlayerId::Two).discard,
        vec![card_ref(7, "ember-lance")]
    );
}

#[test]
fn demo_a_draw_spell_emits_card_drawn_through_apply() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(9, "scrying-glass")];
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };
    state.players.get_mut(PlayerId::One).deck = vec![card_ref(50, "quarry-whelp")];

    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(9),
            targets: vec![],
            mana_hint: None,
        },
    )
    .expect("Scrying Glass is affordable and legal in the caster's own resting Main");

    let defender_passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster holds Priority second");

    assert_eq!(
        resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card: card_ref(9, "scrying-glass"),
                    targets: vec![],
                },
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: CardInstanceId(50),
            },
        ]
    );
    assert_eq!(
        resolved.state.players.get(PlayerId::One).hand,
        vec![card_ref(50, "quarry-whelp")]
    );
    assert_eq!(
        resolved.state.players.get(PlayerId::One).discard,
        vec![card_ref(9, "scrying-glass")]
    );
}

#[test]
fn demo_a_wrong_timing_cast_returns_wrong_phase_through_apply() {
    // Rules §34: an Attack Spell may not be cast proactively, even
    // during the caster's own resting Main Phase with no window open.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(5, "ember-lance")];
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let result = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(5),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    );

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn demo_a_draw_from_an_empty_deck_ends_the_game_immediately() {
    // `base_state` already gives both players an empty Deck.
    let state = base_state();

    let outcome =
        full_end_turn(&state, PlayerId::One).expect("legal from a resting Main, both pass");

    assert_eq!(
        outcome.state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::EmptyDeckDraw,
        })
    );
    assert_eq!(
        outcome.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Main],
            },
            GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: LossReason::EmptyDeckDraw,
            },
        ],
        "CardDrawn never fires and ManaProduced never runs after the loss"
    );

    // Loss is immediate (rules §2): every later action is rejected.
    assert_eq!(
        apply(&outcome.state, &end_turn(PlayerId::Two)),
        Err(ActionError::GameAlreadyOver)
    );
}
