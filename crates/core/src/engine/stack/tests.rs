//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.

use super::*;
use crate::domain::cards::CardDefId;
use crate::domain::ids::{BenchSlot, CardInstanceId};
use crate::domain::state::{
    CardRef, ManaBank, PendingInput, PerPlayer, PlayerState, SummonInstance, TurnState,
    UpgradeChain, WorkItem,
};
use std::collections::VecDeque;

fn summon(owner: PlayerId, def: &'static str) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def: CardDefId(def),
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

fn player_state(owner: PlayerId) -> PlayerState {
    PlayerState {
        main: Some(summon(owner, "quarry-whelp")),
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
    }
}

// --- declare_attack -----------------------------------------------

#[test]
fn declare_attack_pays_the_free_cost_opens_a_window_and_pushes_the_stack() {
    let state = base_state();

    let outcome = declare_attack(&state, PlayerId::One, Position::Main, None)
        .expect("Quarry Whelp's Attack is free");

    assert!(outcome.state.turn.normal_attack_used);
    assert_eq!(outcome.state.turn.phase, Phase::Combat);
    assert_eq!(
        outcome.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
    assert_eq!(
        outcome.state.stack,
        vec![StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        }]
    );
    assert_eq!(
        outcome.events,
        vec![GameEvent::AttackDeclared {
            player: PlayerId::One,
            target: Position::Main,
        }]
    );
}

#[test]
fn declare_attack_pays_a_nonzero_cost_and_emits_mana_deducted_first() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 3,
        mind: 0,
        spirit: 0,
    };

    let outcome = declare_attack(&state, PlayerId::One, Position::Main, None)
        .expect("Quarry Brute's Attack costs one Generic and the bank covers it");

    assert_eq!(
        outcome.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::AttackDeclared {
                player: PlayerId::One,
                target: Position::Main,
            },
        ]
    );
    assert_eq!(outcome.state.players.get(PlayerId::One).mana.matter, 2);
}

#[test]
fn declare_attack_reports_a_mana_shortfall() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));

    let result = declare_attack(&state, PlayerId::One, Position::Main, None);

    assert_eq!(
        result,
        Err(ActionError::InsufficientMana {
            short: ManaBank {
                matter: 1,
                mind: 0,
                spirit: 0,
            }
        })
    );
}

#[test]
fn declare_attack_rejects_a_hint_naming_an_empty_pool() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 3,
        mind: 0,
        spirit: 0,
    };

    let result = declare_attack(
        &state,
        PlayerId::One,
        Position::Main,
        Some(ManaType::Spirit),
    );

    assert_eq!(result, Err(ActionError::InvalidManaHint));
}

#[test]
fn declare_attack_rejects_a_second_declaration_even_after_a_different_summon_moved_into_main() {
    let mut state = base_state();
    state.turn.normal_attack_used = true;
    state.turn.phase = Phase::Combat;
    // Simulate a different Summon now sitting in Main; the rejection
    // must still be the specific NormalAttackAlreadyUsed, not something
    // that depends on which Summon is there.
    state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));

    let result = declare_attack(&state, PlayerId::One, Position::Main, None);

    assert_eq!(result, Err(ActionError::NormalAttackAlreadyUsed));
}

#[test]
fn declare_attack_rejects_a_bench_target() {
    let state = base_state();

    let result = declare_attack(
        &state,
        PlayerId::One,
        Position::Bench(BenchSlot::First),
        None,
    );

    assert_eq!(result, Err(ActionError::InvalidTarget));
}

#[test]
fn declare_attack_rejects_an_open_window() {
    let mut state = base_state();
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });

    let result = declare_attack(&state, PlayerId::One, Position::Main, None);

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn a_rejected_declaration_leaves_the_callers_state_untouched() {
    let mut state = base_state();
    state.turn.normal_attack_used = true;
    let before = state.clone();

    let _ = declare_attack(&state, PlayerId::One, Position::Main, None);

    assert_eq!(state, before);
}

// --- cast_spell: timing gates, payment, Stack push -------------------

fn hand_with(def: &'static str, instance: u32) -> Vec<CardRef> {
    vec![CardRef {
        instance: CardInstanceId(instance),
        def: CardDefId(def),
    }]
}

#[test]
fn cast_spell_a_support_spell_pays_cost_pushes_the_stack_and_opens_a_window_for_the_opponent() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = hand_with("renewing-balm", 5);
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let outcome = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    )
    .expect("Renewing Balm is affordable and legal in the caster's own Main");

    assert!(outcome.state.players.get(PlayerId::One).hand.is_empty());
    assert_eq!(
        outcome.state.stack,
        vec![StackItem::Spell {
            caster: PlayerId::One,
            card: CardRef {
                instance: CardInstanceId(5),
                def: CardDefId("renewing-balm"),
            },
            targets: vec![Position::Main],
        }]
    );
    assert_eq!(
        outcome.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
    assert!(outcome.state.turn.spell_played_this_turn);
    assert_eq!(
        outcome.events,
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
}

#[test]
fn cast_spell_allows_a_support_spell_as_a_response_too() {
    let mut state = base_state();
    state.turn.active_player = PlayerId::Two;
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });
    state.players.get_mut(PlayerId::One).hand = hand_with("renewing-balm", 5);
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let outcome = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    )
    .expect("One holds Priority, so the Support Spell is a legal response too (rules §34)");

    assert_eq!(
        outcome.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
}

#[test]
fn cast_spell_rejects_a_support_spell_outside_main_and_without_priority() {
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.players.get_mut(PlayerId::One).hand = hand_with("renewing-balm", 5);

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    );

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn cast_spell_rejects_an_attack_spell_cast_proactively_in_main() {
    // Rules §34: an Attack Spell "may normally be played as part of an
    // attack or as responses to appropriate offensive effects" — never
    // proactively, even during the caster's own resting Main Phase.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = hand_with("ember-lance", 5);
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    );

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn cast_spell_allows_an_attack_spell_as_a_response_inside_a_combat_window() {
    // Rules §46: "the defending player may play a legal Support Spell,
    // play a legal Attack Spell if its timing allows, or pass." The
    // Attack Spell is cast after the attack is already on the Stack, so
    // it resolves first (last in, first out — rules §35).
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::Two,
        prior_pass: false,
    });
    state.stack.push(StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Main,
    });
    state.players.get_mut(PlayerId::Two).hand = hand_with("ember-lance", 7);
    state.players.get_mut(PlayerId::Two).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let outcome = cast_spell(
        &state,
        PlayerId::Two,
        CardInstanceId(7),
        vec![Position::Main],
        None,
    )
    .expect("Two holds Priority, so the Attack Spell is a legal response");

    assert_eq!(
        outcome.state.stack,
        vec![
            StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            },
            StackItem::Spell {
                caster: PlayerId::Two,
                card: CardRef {
                    instance: CardInstanceId(7),
                    def: CardDefId("ember-lance"),
                },
                targets: vec![Position::Main],
            },
        ],
        "the Attack Spell is last in, so it sits on top of the original attack"
    );
    assert_eq!(
        outcome.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        })
    );
}

#[test]
fn cast_spell_rejects_a_pending_decision() {
    let mut state = base_state();
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });
    state.players.get_mut(PlayerId::One).hand = hand_with("renewing-balm", 5);

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    );

    assert_eq!(result, Err(ActionError::PendingInputMismatch));
}

#[test]
fn cast_spell_rejects_a_card_not_in_hand() {
    let state = base_state();

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(999),
        vec![Position::Main],
        None,
    );

    assert_eq!(result, Err(ActionError::UnknownCard));
}

#[test]
fn cast_spell_rejects_a_non_spell_card() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = hand_with("quarry-whelp", 5);

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    );

    assert_eq!(result, Err(ActionError::UnknownCard));
}

#[test]
fn cast_spell_reports_a_mana_shortfall() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = hand_with("renewing-balm", 5);

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        None,
    );

    assert_eq!(
        result,
        Err(ActionError::InsufficientMana {
            short: ManaBank {
                matter: 1,
                mind: 0,
                spirit: 0,
            }
        })
    );
}

#[test]
fn cast_spell_rejects_a_hint_naming_an_empty_pool() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).hand = hand_with("renewing-balm", 5);
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let result = cast_spell(
        &state,
        PlayerId::One,
        CardInstanceId(5),
        vec![Position::Main],
        Some(ManaType::Spirit),
    );

    assert_eq!(result, Err(ActionError::InvalidManaHint));
}

// --- pass: single vs double, window holder gating -------------------

#[test]
fn a_single_pass_hands_priority_to_the_other_player_and_records_it() {
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::Two,
        prior_pass: false,
    });
    state.stack.push(StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Main,
    });

    let outcome = pass(&state, PlayerId::Two).expect("Two holds Priority");

    assert_eq!(
        outcome.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: true,
        })
    );
    assert_eq!(
        outcome.events,
        vec![GameEvent::PriorityPassed {
            player: PlayerId::Two
        }]
    );
}

#[test]
fn only_the_window_holder_may_pass() {
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::Two,
        prior_pass: false,
    });

    let result = pass(&state, PlayerId::One);

    assert_eq!(result, Err(ActionError::NotYourDecision));
}

#[test]
fn a_double_pass_with_something_still_on_the_stack_just_closes_the_window() {
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: true,
    });
    state.stack.push(StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Main,
    });

    let outcome = pass(&state, PlayerId::One).expect("One holds Priority");

    assert_eq!(outcome.state.turn.window, None);
    assert_eq!(outcome.state.stack.len(), 1, "left for resolution to drain");
    assert_eq!(
        outcome.events,
        vec![GameEvent::PriorityPassed {
            player: PlayerId::One
        }]
    );
}

// --- window_after_play: the other half of rules §32 ------------------

#[test]
fn window_after_play_names_the_other_player_as_holder_with_a_clean_streak() {
    assert_eq!(
        window_after_play(PlayerId::One),
        StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        }
    );
}

#[test]
fn window_after_play_never_names_the_acting_player_as_holder() {
    assert_ne!(window_after_play(PlayerId::One).holder, PlayerId::One);
    assert_ne!(window_after_play(PlayerId::Two).holder, PlayerId::Two);
}

#[test]
fn an_intervening_played_effect_clears_prior_pass_so_only_the_final_two_passes_are_consecutive() {
    // Rules §33's worked example: the defender passes, the attacker
    // plays an effect through the real §32 bookkeeping instead of
    // passing, the defender passes again, then the attacker passes —
    // only that last pair is consecutive.
    let mut state = base_state();
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::Two,
        prior_pass: false,
    });
    state.stack.push(StackItem::Attack {
        attacker: PlayerId::One,
        target: Position::Main,
    });

    // Defender (Two) passes first.
    let after_first_pass = pass(&state, PlayerId::Two).expect("Two holds Priority");
    assert_eq!(
        after_first_pass.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: true,
        })
    );

    // The attacker (One) plays an effect instead of passing: Priority
    // moves to Two and the pass streak clears (rules §32), through the
    // same `window_after_play` a future Spell cast would call.
    let mut after_played_effect = after_first_pass.state;
    after_played_effect.turn.window = Some(window_after_play(PlayerId::One));

    // Defender passes again — the first of a fresh streak.
    let after_second_pass = pass(&after_played_effect, PlayerId::Two).expect("Two holds Priority");
    assert_eq!(
        after_second_pass.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: true,
        }),
        "this is only the first pass since the played effect"
    );

    // The attacker passes: now two in a row, so the window closes.
    let after_third_pass =
        pass(&after_second_pass.state, PlayerId::One).expect("One holds Priority");
    assert_eq!(after_third_pass.state.turn.window, None);
}

#[test]
fn a_double_pass_with_an_empty_stack_hands_the_turn_over_immediately() {
    let mut state = base_state();
    state.turn.window = Some(StackWindow {
        holder: PlayerId::Two,
        prior_pass: true,
    });

    let outcome = pass(&state, PlayerId::Two).expect("Two holds Priority");

    assert_eq!(outcome.state.turn.window, None);
    assert_eq!(outcome.state.turn.active_player, PlayerId::Two);
    assert_eq!(outcome.state.turn.phase, Phase::Upkeep);
    assert_eq!(
        outcome.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two
            },
        ]
    );
    assert_eq!(
        outcome.state.work,
        VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(crate::domain::state::ManaSource::Player),
            WorkItem::BeginMainPhase,
        ])
    );
}

#[test]
fn pass_rejects_when_no_window_is_open() {
    let state = base_state();

    let result = pass(&state, PlayerId::One);

    assert_eq!(result, Err(ActionError::WrongPhase));
}

#[test]
fn pass_rejects_a_pending_decision() {
    let mut state = base_state();
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });

    let result = pass(&state, PlayerId::One);

    assert_eq!(result, Err(ActionError::PendingInputMismatch));
}
