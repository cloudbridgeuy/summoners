#![allow(clippy::expect_used)]
use super::*;
use crate::protocol::{
    BoardView, CardDescription, HandCardView, ManaView, PendingKindView, PendingView, PhaseView,
    PlayerPublicView, SeatsView,
};

const SEATS: [Seat; 2] = [Seat::One, Seat::Two];

fn view(seat: Seat) -> PlayerView {
    let player = PlayerPublicView {
        board: BoardView {
            main: None,
            bench: [None, None, None],
        },
        mana: ManaView {
            matter: 1,
            mind: 1,
            spirit: 1,
        },
        main_losses: 0,
        discard: Vec::new(),
        persistent: Vec::new(),
        deck_count: 3,
        prize_count: 2,
        hand_count: 1,
    };
    PlayerView {
        you: seat,
        hand: vec![HandCardView {
            instance: 9,
            card: CardDescription {
                name: "Base".to_string(),
                life: None,
                retreat_cost: None,
                mana_types: Vec::new(),
                cost: None,
                abilities: Vec::new(),
                effects: Vec::new(),
            },
        }],
        players: SeatsView {
            one: player.clone(),
            two: player,
        },
        coin: false,
        stack: Vec::new(),
        phase: PhaseView::Main,
        active_player: Seat::One,
        priority_holder: None,
        pending: None,
        outcome: None,
    }
}

fn actor(seat: Seat) -> PlayerIdV1 {
    player(seat)
}

fn invalid_render() -> Vec<PromptEffect> {
    vec![PromptEffect::Render(vec!["Invalid selection".to_string()])]
}

#[test]
fn play_form_maps_numbered_hand_and_bench_to_wire_action() {
    let view = view(Seat::One);
    let (state, _) = reduce(PromptState::Menu { revision: 4 }, &view, "1");
    let (state, _) = reduce(state, &view, "1");
    let (_, effects) = reduce(state, &view, "3");
    assert_eq!(
        effects,
        vec![PromptEffect::Submit {
            revision: 4,
            action: ActionV1::PlaySummon {
                player: PlayerIdV1::One,
                card: 9,
                slot: BenchSlotV1::Third
            }
        }]
    );
}
#[test]
fn prize_number_is_one_based_and_wire_index_is_zero_based() {
    let view = view(Seat::One);
    let (_, effects) = form_step(2, &view, PlayerIdV1::One, Form::Prize, Some(2));
    assert_eq!(
        effects,
        vec![PromptEffect::Submit {
            revision: 2,
            action: ActionV1::ChoosePrize {
                player: PlayerIdV1::One,
                prize_index: 1
            }
        }]
    );
}
#[test]
fn revision_cancels_active_form() {
    let (state, effects) = revised(
        &PromptState::Form {
            revision: 1,
            form: Form::Prize,
        },
        2,
    );
    assert_eq!(state, PromptState::Menu { revision: 2 });
    assert_eq!(effects, vec![PromptEffect::Cancelled]);
}
#[test]
fn position_order_is_main_then_bench() {
    assert_eq!(position(Some(1)), Some(PositionV1::Main));
    assert_eq!(
        position(Some(4)),
        Some(PositionV1::Bench {
            slot: BenchSlotV1::Third
        })
    );
}

#[test]
fn play_summon_selects_hand_card_then_bench_slot() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "1");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::PlayCard
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec!["1. Base".to_string()])]
        );

        let (state, effects) = reduce(state, &view, "1");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::PlaySlot { card: 9 }
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(bench_lines())]);

        for (input, slot) in [
            ("1", BenchSlotV1::First),
            ("2", BenchSlotV1::Second),
            ("3", BenchSlotV1::Third),
        ] {
            let (end_state, end_effects) = reduce(state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::PlaySummon {
                        player: actor(seat),
                        card: 9,
                        slot,
                    },
                }]
            );
        }
    }
}
#[test]
fn play_summon_invalid_card_index_resets_to_menu() {
    for seat in SEATS {
        let view = view(seat);
        for input in ["0", "2", "abc"] {
            let (state, effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::PlayCard,
                },
                &view,
                input,
            );
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}
#[test]
fn play_summon_invalid_slot_resets_to_menu() {
    for seat in SEATS {
        let view = view(seat);
        for input in ["0", "4"] {
            let (state, effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::PlaySlot { card: 9 },
                },
                &view,
                input,
            );
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn upgrade_summon_selects_hand_card_then_position() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "2");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::UpgradeCard
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec!["1. Base".to_string()])]
        );

        let (state, effects) = reduce(state, &view, "1");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::UpgradePosition { card: 9 }
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec![
                "1. Main".to_string(),
                "2. Bench 1".to_string(),
                "3. Bench 2".to_string(),
                "4. Bench 3".to_string(),
            ])]
        );

        for (input, position) in [
            ("1", PositionV1::Main),
            (
                "2",
                PositionV1::Bench {
                    slot: BenchSlotV1::First,
                },
            ),
            (
                "3",
                PositionV1::Bench {
                    slot: BenchSlotV1::Second,
                },
            ),
            (
                "4",
                PositionV1::Bench {
                    slot: BenchSlotV1::Third,
                },
            ),
        ] {
            let (end_state, end_effects) = reduce(state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::UpgradeSummon {
                        player: actor(seat),
                        card: 9,
                        position,
                    },
                }]
            );
        }

        for input in ["0", "5"] {
            let (state, effects) = reduce(state, &view, input);
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn retreat_selects_bench_slot_then_mana_hint() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "3");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::RetreatSlot
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(bench_lines())]);

        let (state, effects) = reduce(state, &view, "2");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::Mana {
                    action: ManaAction::Retreat(BenchSlotV1::Second)
                }
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(mana_hint_lines())]);

        for (input, hint) in [
            ("1", Some(ManaTypeV1::Matter)),
            ("2", Some(ManaTypeV1::Mind)),
            ("3", Some(ManaTypeV1::Spirit)),
            ("4", None),
        ] {
            let (end_state, end_effects) = reduce(state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::Retreat {
                        player: actor(seat),
                        slot: BenchSlotV1::Second,
                        mana_hint: hint,
                    },
                }]
            );
        }

        for input in ["0", "5"] {
            let (state, effects) = reduce(state, &view, input);
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn declare_attack_selects_target_then_mana_hint() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "4");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::AttackTarget
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(position_lines())]);

        for (input, target) in [
            ("1", PositionV1::Main),
            (
                "2",
                PositionV1::Bench {
                    slot: BenchSlotV1::First,
                },
            ),
            (
                "3",
                PositionV1::Bench {
                    slot: BenchSlotV1::Second,
                },
            ),
            (
                "4",
                PositionV1::Bench {
                    slot: BenchSlotV1::Third,
                },
            ),
        ] {
            let (next_state, next_effects) = reduce(state, &view, input);
            assert_eq!(
                next_state,
                PromptState::Form {
                    revision: 4,
                    form: Form::Mana {
                        action: ManaAction::Attack(target)
                    }
                }
            );
            assert_eq!(next_effects, vec![PromptEffect::Render(mana_hint_lines())]);

            for (hint_input, hint) in [
                ("1", Some(ManaTypeV1::Matter)),
                ("2", Some(ManaTypeV1::Mind)),
                ("3", Some(ManaTypeV1::Spirit)),
                ("4", None),
            ] {
                let (end_state, end_effects) = reduce(next_state, &view, hint_input);
                assert_eq!(end_state, PromptState::Menu { revision: 4 });
                assert_eq!(
                    end_effects,
                    vec![PromptEffect::Submit {
                        revision: 4,
                        action: ActionV1::DeclareAttack {
                            player: actor(seat),
                            target,
                            mana_hint: hint,
                        },
                    }]
                );
            }

            let (inv_state, inv_effects) = reduce(next_state, &view, "5");
            assert_eq!(inv_state, PromptState::Menu { revision: 4 });
            assert_eq!(inv_effects, invalid_render());
        }
    }
}

#[test]
fn end_turn_confirms_with_one() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "5");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::EndTurn
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec![
                "Enter 1 to confirm, or cancel".to_string()
            ])]
        );

        let (end_state, end_effects) = reduce(state, &view, "1");
        assert_eq!(end_state, PromptState::Menu { revision: 4 });
        assert_eq!(
            end_effects,
            vec![PromptEffect::Submit {
                revision: 4,
                action: ActionV1::EndTurn {
                    player: actor(seat)
                },
            }]
        );

        for input in ["2", "abc"] {
            let (state, effects) = reduce(state, &view, input);
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn pass_priority_submits_immediately_from_menu() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "6");
        assert_eq!(state, PromptState::Menu { revision: 4 });
        assert_eq!(
            effects,
            vec![PromptEffect::Submit {
                revision: 4,
                action: ActionV1::PassPriority {
                    player: actor(seat)
                },
            }]
        );
    }
}

#[test]
fn convert_coin_selects_mana_type() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "7");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::Mana {
                    action: ManaAction::Convert
                }
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(mana_lines())]);

        for (input, mana_type) in [
            ("1", ManaTypeV1::Matter),
            ("2", ManaTypeV1::Mind),
            ("3", ManaTypeV1::Spirit),
        ] {
            let (end_state, end_effects) = reduce(state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::ConvertCoin {
                        player: actor(seat),
                        mana_type,
                    },
                }]
            );
        }

        let (state, effects) = reduce(state, &view, "4");
        assert_eq!(state, PromptState::Menu { revision: 4 });
        assert_eq!(effects, invalid_render());
    }
}

#[test]
fn choose_mana_type_forced_form_submits_each_type() {
    for seat in SEATS {
        let view = view(seat);
        let mut pending_view = view.clone();
        pending_view.pending = Some(PendingView {
            kind: PendingKindView::ManaProduction,
            owner: seat,
        });
        assert_eq!(
            forced_form(&pending_view),
            Some(Form::Mana {
                action: ManaAction::Choose
            })
        );

        let form = Form::Mana {
            action: ManaAction::Choose,
        };
        for (input, mana_type) in [
            ("1", ManaTypeV1::Matter),
            ("2", ManaTypeV1::Mind),
            ("3", ManaTypeV1::Spirit),
        ] {
            let (end_state, end_effects) =
                reduce(PromptState::Form { revision: 4, form }, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::ChooseManaType {
                        player: actor(seat),
                        mana_type,
                    },
                }]
            );
        }

        let (state, effects) = reduce(PromptState::Form { revision: 4, form }, &view, "4");
        assert_eq!(state, PromptState::Menu { revision: 4 });
        assert_eq!(effects, invalid_render());

        let (state, effects) = reduce(PromptState::Form { revision: 4, form }, &view, "cancel");
        assert_eq!(state, PromptState::Menu { revision: 4 });
        assert_eq!(
            effects,
            vec![
                PromptEffect::Cancelled,
                PromptEffect::Render(prompt(&view, 4))
            ]
        );
    }
}

#[test]
fn choose_promotion_forced_form_submits_each_slot() {
    for seat in SEATS {
        let view = view(seat);
        let mut pending_view = view.clone();
        pending_view.pending = Some(PendingView {
            kind: PendingKindView::Promotion,
            owner: seat,
        });
        assert_eq!(forced_form(&pending_view), Some(Form::Promotion));

        for (input, slot) in [
            ("1", BenchSlotV1::First),
            ("2", BenchSlotV1::Second),
            ("3", BenchSlotV1::Third),
        ] {
            let (end_state, end_effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::Promotion,
                },
                &view,
                input,
            );
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::ChoosePromotion {
                        player: actor(seat),
                        slot,
                    },
                }]
            );
        }

        for input in ["0", "4"] {
            let (state, effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::Promotion,
                },
                &view,
                input,
            );
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn choose_prize_forced_form_submits_zero_based_index() {
    for seat in SEATS {
        let view = view(seat);
        let mut pending_view = view.clone();
        pending_view.pending = Some(PendingView {
            kind: PendingKindView::PrizePick,
            owner: seat,
        });
        assert_eq!(forced_form(&pending_view), Some(Form::Prize));

        for (input, index) in [("1", 0u64), ("2", 1), ("7", 6)] {
            let (end_state, end_effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::Prize,
                },
                &view,
                input,
            );
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::ChoosePrize {
                        player: actor(seat),
                        prize_index: index,
                    },
                }]
            );
        }

        for input in ["0", "abc"] {
            let (state, effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::Prize,
                },
                &view,
                input,
            );
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn forced_form_is_none_without_pending() {
    let view = view(Seat::One);
    assert_eq!(forced_form(&view), None);
}

#[test]
fn forced_form_ignores_owner_seat() {
    for kind in [
        PendingKindView::ManaProduction,
        PendingKindView::Promotion,
        PendingKindView::PrizePick,
    ] {
        let mut owned_by_one = view(Seat::One);
        owned_by_one.pending = Some(PendingView {
            kind,
            owner: Seat::One,
        });
        let mut owned_by_two = view(Seat::One);
        owned_by_two.pending = Some(PendingView {
            kind,
            owner: Seat::Two,
        });
        assert_eq!(forced_form(&owned_by_one), forced_form(&owned_by_two));
        assert!(forced_form(&owned_by_one).is_some());
    }
}

#[test]
fn resign_confirms_with_yes_variants_and_one() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "8");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::Resign
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec![
                "Confirm Give up with yes".to_string()
            ])]
        );

        for input in ["yes", "YES", " yes ", "1"] {
            let (end_state, end_effects) = reduce(state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::Resign {
                        player: actor(seat)
                    },
                }]
            );
        }

        for input in ["no", "2"] {
            let (state, effects) = reduce(state, &view, input);
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }

        let (state, effects) = reduce(state, &view, "cancel");
        assert_eq!(state, PromptState::Menu { revision: 4 });
        assert_eq!(
            effects,
            vec![
                PromptEffect::Cancelled,
                PromptEffect::Render(prompt(&view, 4))
            ]
        );
    }
}

#[test]
fn menu_invalid_selection_resets_with_message() {
    for seat in SEATS {
        let view = view(seat);
        for input in ["0", "9", "", "abc"] {
            let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, input);
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}

#[test]
fn cancel_is_case_insensitive_and_trimmed_from_menu_and_forms() {
    for seat in SEATS {
        let view = view(seat);
        let forms = [
            PromptState::Menu { revision: 4 },
            PromptState::Form {
                revision: 4,
                form: Form::PlayCard,
            },
            PromptState::Form {
                revision: 4,
                form: Form::PlaySlot { card: 9 },
            },
            PromptState::Form {
                revision: 4,
                form: Form::UpgradeCard,
            },
            PromptState::Form {
                revision: 4,
                form: Form::UpgradePosition { card: 9 },
            },
            PromptState::Form {
                revision: 4,
                form: Form::RetreatSlot,
            },
            PromptState::Form {
                revision: 4,
                form: Form::AttackTarget,
            },
            PromptState::Form {
                revision: 4,
                form: Form::Mana {
                    action: ManaAction::Convert,
                },
            },
            PromptState::Form {
                revision: 4,
                form: Form::Mana {
                    action: ManaAction::Choose,
                },
            },
            PromptState::Form {
                revision: 4,
                form: Form::Mana {
                    action: ManaAction::Retreat(BenchSlotV1::First),
                },
            },
            PromptState::Form {
                revision: 4,
                form: Form::Mana {
                    action: ManaAction::Attack(PositionV1::Main),
                },
            },
            PromptState::Form {
                revision: 4,
                form: Form::EndTurn,
            },
            PromptState::Form {
                revision: 4,
                form: Form::Resign,
            },
            PromptState::Form {
                revision: 4,
                form: Form::Promotion,
            },
            PromptState::Form {
                revision: 4,
                form: Form::Prize,
            },
        ];
        for state in forms {
            for input in ["cancel", "CANCEL", "  cancel "] {
                let (end_state, end_effects) = reduce(state, &view, input);
                assert_eq!(end_state, PromptState::Menu { revision: 4 });
                assert_eq!(
                    end_effects,
                    vec![
                        PromptEffect::Cancelled,
                        PromptEffect::Render(prompt(&view, 4))
                    ]
                );
            }
        }
    }
}

#[test]
fn revised_cancels_when_revision_differs() {
    let (state, effects) = revised(
        &PromptState::Form {
            revision: 3,
            form: Form::Prize,
        },
        4,
    );
    assert_eq!(state, PromptState::Menu { revision: 4 });
    assert_eq!(effects, vec![PromptEffect::Cancelled]);

    let (state, effects) = revised(&PromptState::Menu { revision: 3 }, 4);
    assert_eq!(state, PromptState::Menu { revision: 4 });
    assert_eq!(effects, vec![PromptEffect::Cancelled]);
}

#[test]
fn revised_keeps_state_without_effects_when_revision_matches() {
    let (state, effects) = revised(&PromptState::Menu { revision: 4 }, 4);
    assert_eq!(state, PromptState::Menu { revision: 4 });
    assert_eq!(effects, Vec::new());

    let form = PromptState::Form {
        revision: 4,
        form: Form::EndTurn,
    };
    let (state, effects) = revised(&form, 4);
    assert_eq!(state, form);
    assert_eq!(effects, Vec::new());
}

#[test]
fn queued_input_after_stale_revision_is_interpreted_by_menu_not_the_old_form() {
    let view = view(Seat::One);
    let (state, effects) = revised(
        &PromptState::Form {
            revision: 3,
            form: Form::PlayCard,
        },
        4,
    );
    assert_eq!(state, PromptState::Menu { revision: 4 });
    assert_eq!(effects, vec![PromptEffect::Cancelled]);

    let (state, effects) = reduce(state, &view, "1");
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::PlayCard
        }
    );
    assert_eq!(
        effects,
        vec![PromptEffect::Render(vec!["1. Base".to_string()])]
    );
}

#[test]
fn submit_effect_carries_the_revision_the_state_was_built_with() {
    let view = view(Seat::One);
    let (_, old_effects) = reduce(
        PromptState::Form {
            revision: 3,
            form: Form::EndTurn,
        },
        &view,
        "1",
    );
    assert_eq!(
        old_effects,
        vec![PromptEffect::Submit {
            revision: 3,
            action: ActionV1::EndTurn {
                player: PlayerIdV1::One
            },
        }]
    );

    let (_, current_effects) = reduce(
        PromptState::Form {
            revision: 4,
            form: Form::EndTurn,
        },
        &view,
        "1",
    );
    assert_eq!(
        current_effects,
        vec![PromptEffect::Submit {
            revision: 4,
            action: ActionV1::EndTurn {
                player: PlayerIdV1::One
            },
        }]
    );

    assert_ne!(old_effects, current_effects);
}

#[test]
fn prompt_lines_base_menu_only() {
    let view = view(Seat::One);
    assert_eq!(
        prompt(&view, 4),
        vec![
            "Actions: 1. Play Summon 2. Upgrade Summon 3. Retreat 4. Declare Attack 5. End Turn 6. Pass Priority 7. Convert Coin 8. Resign"
                .to_string()
        ]
    );
}

#[test]
fn prompt_lines_add_pending_line() {
    let mut view = view(Seat::One);
    view.pending = Some(PendingView {
        kind: PendingKindView::ManaProduction,
        owner: Seat::Two,
    });
    let lines = prompt(&view, 4);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[1],
        format!(
            "Awaiting {:?} from {:?}",
            PendingKindView::ManaProduction,
            Seat::Two
        )
    );
}

#[test]
fn prompt_lines_add_priority_line() {
    let mut view = view(Seat::One);
    view.priority_holder = Some(Seat::One);
    let lines = prompt(&view, 4);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1], format!("Priority: {:?}", Some(Seat::One)));
}
