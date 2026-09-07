#![allow(clippy::expect_used, clippy::unwrap_used)]
use super::*;
use crate::protocol::{
    BoardView, CardDescription, HandCardView, ManaView, PhaseView, PlayerPublicView, ReadinessView,
    SeatsView, SummonView,
};
use summoners_match_log::wire::EntityIdV1;

const SEATS: [Seat; 2] = [Seat::One, Seat::Two];

fn base_player() -> PlayerPublicView {
    PlayerPublicView {
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
        hand_count: 2,
    }
}

fn card_with_abilities(name: &str, abilities: Vec<AbilityDescription>) -> CardDescription {
    CardDescription {
        name: name.to_string(),
        life: None,
        retreat_cost: None,
        mana_types: Vec::new(),
        cost: None,
        abilities,
        effects: Vec::new(),
        modifiers: Vec::new(),
        timing: None,
        persistent: false,
    }
}

fn hand_card(instance: u32, name: &str) -> HandCardView {
    HandCardView {
        instance,
        card: card_with_abilities(name, Vec::new()),
    }
}

fn ability(kind: AbilityKind, id: &str, name: &str) -> AbilityDescription {
    AbilityDescription {
        kind,
        id: EntityIdV1(id.to_string()),
        name: name.to_string(),
        cost: None,
        effects: Vec::new(),
        trigger_event: None,
        timing: None,
        respondable: false,
        persistent: false,
        modifiers: Vec::new(),
    }
}

fn chain_summon(chain: Vec<CardDescription>) -> SummonView {
    SummonView {
        chain,
        damage: 0,
        readiness: ReadinessView::Ready,
        owner: Seat::One,
        controller: Seat::One,
    }
}

fn main_board(summon: SummonView) -> BoardView {
    BoardView {
        main: Some(summon),
        bench: [None, None, None],
    }
}

fn with_board_for(mut view: PlayerView, seat: Seat, board: BoardView) -> PlayerView {
    match seat {
        Seat::One => view.players.one.board = board,
        Seat::Two => view.players.two.board = board,
    }
    view
}

fn view(seat: Seat) -> PlayerView {
    PlayerView {
        you: seat,
        hand: vec![hand_card(9, "Base"), hand_card(10, "Second")],
        players: SeatsView {
            one: base_player(),
            two: base_player(),
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

fn invalid_render() -> Vec<PromptEffect> {
    vec![PromptEffect::Render(vec!["Invalid selection".to_string()])]
}

#[test]
fn cast_spell_menu_opens_hand_then_selecting_card_opens_targets() {
    for seat in SEATS {
        let view = view(seat);
        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "9");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::CastCard
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec![
                "1. Base".to_string(),
                "2. Second".to_string()
            ])]
        );

        let (state, effects) = reduce(state, &view, "2");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::Targets {
                    action: TargetAction::Cast(10),
                    targets: TargetList::empty()
                }
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(target_lines())]);
    }
}

#[test]
fn cast_spell_hand_selection_invalid_resets_to_menu() {
    for seat in SEATS {
        let view = view(seat);
        for input in ["0", "3", "", "abc"] {
            let (state, effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::CastCard,
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
fn cast_spell_zero_targets_each_mana_hint_submits_for_both_seats() {
    for seat in SEATS {
        let view = view(seat);
        let state = PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action: TargetAction::Cast(9),
                targets: TargetList::empty(),
            },
        };

        let (hint_state, hint_effects) = reduce(state, &view, "5");
        assert_eq!(
            hint_state,
            PromptState::Form {
                revision: 4,
                form: Form::ManaHint {
                    action: TargetAction::Cast(9),
                    targets: TargetList::empty()
                }
            }
        );
        assert_eq!(hint_effects, vec![PromptEffect::Render(mana_hint_lines())]);

        for (input, hint) in [
            ("1", Some(ManaTypeV1::Matter)),
            ("2", Some(ManaTypeV1::Mind)),
            ("3", Some(ManaTypeV1::Spirit)),
            ("4", None),
        ] {
            let (end_state, end_effects) = reduce(hint_state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::CastSpell {
                        player: player(seat),
                        card: 9,
                        targets: Vec::new(),
                        mana_hint: hint,
                    },
                }]
            );
        }

        for input in ["0", "5", "abc", ""] {
            let (invalid_state, invalid_effects) = reduce(hint_state, &view, input);
            assert_eq!(invalid_state, PromptState::Menu { revision: 4 });
            assert_eq!(invalid_effects, invalid_render());
        }
    }
}

#[test]
fn cast_spell_targets_duplicate_position_preserved_in_order() {
    let view = view(Seat::One);
    let mut state = PromptState::Form {
        revision: 4,
        form: Form::Targets {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
    };
    for input in ["2", "2", "1"] {
        let (next_state, effects) = reduce(state, &view, input);
        assert_eq!(effects, vec![PromptEffect::Render(target_lines())]);
        state = next_state;
    }

    let (state, effects) = reduce(state, &view, "5");
    assert_eq!(effects, vec![PromptEffect::Render(mana_hint_lines())]);

    let (end_state, end_effects) = reduce(state, &view, "4");
    assert_eq!(end_state, PromptState::Menu { revision: 4 });
    assert_eq!(
        end_effects,
        vec![PromptEffect::Submit {
            revision: 4,
            action: ActionV1::CastSpell {
                player: PlayerIdV1::One,
                card: 9,
                targets: vec![
                    PositionV1::Bench {
                        slot: BenchSlotV1::First
                    },
                    PositionV1::Bench {
                        slot: BenchSlotV1::First
                    },
                    PositionV1::Main,
                ],
                mana_hint: None,
            },
        }]
    );
}

#[test]
fn cast_spell_targets_all_four_positions_then_fifth_append_rejected() {
    let view = view(Seat::One);
    let mut state = PromptState::Form {
        revision: 4,
        form: Form::Targets {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
    };
    for input in ["1", "2", "3", "4"] {
        let (next_state, _) = reduce(state, &view, input);
        state = next_state;
    }
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action: TargetAction::Cast(9),
                targets: TargetList([
                    Some(PositionV1::Main),
                    Some(PositionV1::Bench {
                        slot: BenchSlotV1::First
                    }),
                    Some(PositionV1::Bench {
                        slot: BenchSlotV1::Second
                    }),
                    Some(PositionV1::Bench {
                        slot: BenchSlotV1::Third
                    }),
                ])
            }
        }
    );

    let (end_state, end_effects) = reduce(state, &view, "1");
    assert_eq!(end_state, PromptState::Menu { revision: 4 });
    assert_eq!(end_effects, invalid_render());
}

#[test]
fn cast_spell_targets_undo_and_clear_sequences() {
    let view = view(Seat::One);
    let action = TargetAction::Cast(9);
    let mut state = PromptState::Form {
        revision: 4,
        form: Form::Targets {
            action,
            targets: TargetList::empty(),
        },
    };

    for input in ["1", "2"] {
        let (next_state, _) = reduce(state, &view, input);
        state = next_state;
    }
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList([
                    Some(PositionV1::Main),
                    Some(PositionV1::Bench {
                        slot: BenchSlotV1::First
                    }),
                    None,
                    None
                ])
            }
        }
    );

    let (state, effects) = reduce(state, &view, "6");
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList([Some(PositionV1::Main), None, None, None])
            }
        }
    );
    assert_eq!(effects, vec![PromptEffect::Render(target_lines())]);

    let (state, _) = reduce(state, &view, "6");
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList::empty()
            }
        }
    );

    let (state, effects) = reduce(state, &view, "6");
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList::empty()
            }
        }
    );
    assert_eq!(effects, vec![PromptEffect::Render(target_lines())]);

    let (state, _) = reduce(state, &view, "3");
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList([
                    Some(PositionV1::Bench {
                        slot: BenchSlotV1::Second
                    }),
                    None,
                    None,
                    None
                ])
            }
        }
    );

    let (state, effects) = reduce(state, &view, "7");
    assert_eq!(
        state,
        PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList::empty()
            }
        }
    );
    assert_eq!(effects, vec![PromptEffect::Render(target_lines())]);

    let (final_state, final_effects) = reduce(state, &view, "7");
    assert_eq!(final_state, state);
    assert_eq!(final_effects, vec![PromptEffect::Render(target_lines())]);
}

#[test]
fn cast_spell_targets_field_invalid_input_resets_to_menu() {
    let view = view(Seat::One);
    let state = PromptState::Form {
        revision: 4,
        form: Form::Targets {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
    };
    for input in ["0", "8", "abc", ""] {
        let (end_state, end_effects) = reduce(state, &view, input);
        assert_eq!(end_state, PromptState::Menu { revision: 4 });
        assert_eq!(end_effects, invalid_render());
    }
}

#[test]
fn cast_spell_mana_hint_field_invalid_input_resets_to_menu() {
    let view = view(Seat::One);
    let state = PromptState::Form {
        revision: 4,
        form: Form::ManaHint {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
    };
    for input in ["0", "5", "abc", ""] {
        let (end_state, end_effects) = reduce(state, &view, input);
        assert_eq!(end_state, PromptState::Menu { revision: 4 });
        assert_eq!(end_effects, invalid_render());
    }
}

#[test]
fn cast_spell_cancel_at_every_field() {
    let view = view(Seat::One);
    let forms = [
        Form::CastCard,
        Form::Targets {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
        Form::ManaHint {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
    ];
    for form in forms {
        for input in ["cancel", "CANCEL", "  cancel "] {
            let (state, effects) = reduce(PromptState::Form { revision: 4, form }, &view, input);
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
}

#[test]
fn activate_skill_menu_opens_positions_then_lists_only_skill_abilities() {
    for seat in SEATS {
        let card = card_with_abilities(
            "Guardian",
            vec![
                ability(AbilityKind::Attack, "atk-1", "Strike"),
                ability(AbilityKind::Trigger, "trig-1", "Ward"),
                ability(AbilityKind::Skill, "skill-1", "Rally"),
            ],
        );
        let view = with_board_for(view(seat), seat, main_board(chain_summon(vec![card])));

        let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, "10");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::SkillPosition
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(position_lines())]);

        let (state, effects) = reduce(state, &view, "1");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::SkillAbility {
                    position: PositionV1::Main
                }
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec!["1. Rally".to_string()])]
        );
    }
}

#[test]
fn activate_skill_position_selection_invalid_resets_to_menu() {
    for seat in SEATS {
        let view = view(seat);
        for input in ["0", "5", "abc", ""] {
            let (state, effects) = reduce(
                PromptState::Form {
                    revision: 4,
                    form: Form::SkillPosition,
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
fn activate_skill_ability_list_and_selection_invalid_without_a_matching_skill() {
    for seat in SEATS {
        let empty_board_view = view(seat);
        let (state, effects) = reduce(
            PromptState::Form {
                revision: 4,
                form: Form::SkillPosition,
            },
            &empty_board_view,
            "1",
        );
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::SkillAbility {
                    position: PositionV1::Main
                }
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(Vec::new())]);

        for input in ["0", "1", "2", "abc", ""] {
            let (end_state, end_effects) = reduce(state, &empty_board_view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(end_effects, invalid_render());
        }

        let non_skill_card = card_with_abilities(
            "Brute",
            vec![
                ability(AbilityKind::Attack, "atk", "Smash"),
                ability(AbilityKind::Trigger, "trig", "Roar"),
            ],
        );
        let non_skill_view = with_board_for(
            view(seat),
            seat,
            main_board(chain_summon(vec![non_skill_card])),
        );
        let (state, effects) = reduce(
            PromptState::Form {
                revision: 4,
                form: Form::SkillPosition,
            },
            &non_skill_view,
            "1",
        );
        assert_eq!(effects, vec![PromptEffect::Render(Vec::new())]);
        let (end_state, end_effects) = reduce(state, &non_skill_view, "1");
        assert_eq!(end_state, PromptState::Menu { revision: 4 });
        assert_eq!(end_effects, invalid_render());

        let single_skill_card = card_with_abilities(
            "Guardian",
            vec![ability(AbilityKind::Skill, "skill-1", "Rally")],
        );
        let single_skill_view = with_board_for(
            view(seat),
            seat,
            main_board(chain_summon(vec![single_skill_card])),
        );
        let (state, _) = reduce(
            PromptState::Form {
                revision: 4,
                form: Form::SkillPosition,
            },
            &single_skill_view,
            "1",
        );
        for input in ["0", "2", "abc", ""] {
            let (end_state, end_effects) = reduce(state, &single_skill_view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(end_effects, invalid_render());
        }
    }
}

#[test]
fn activate_skill_selecting_ability_resolves_top_layer_skill_id_and_reaches_targets() {
    for seat in SEATS {
        let bottom = card_with_abilities(
            "Base Form",
            vec![ability(AbilityKind::Skill, "bottom-skill", "Bottom Skill")],
        );
        let top = card_with_abilities(
            "Evolved Form",
            vec![
                ability(AbilityKind::Attack, "atk", "Slash"),
                ability(AbilityKind::Skill, "top-skill", "Rally"),
                ability(AbilityKind::Trigger, "trig", "Guard"),
            ],
        );
        let view = with_board_for(
            view(seat),
            seat,
            main_board(chain_summon(vec![bottom, top])),
        );

        let (state, effects) = reduce(
            PromptState::Form {
                revision: 4,
                form: Form::SkillPosition,
            },
            &view,
            "1",
        );
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::SkillAbility {
                    position: PositionV1::Main
                }
            }
        );
        assert_eq!(
            effects,
            vec![PromptEffect::Render(vec!["1. Rally".to_string()])]
        );

        let (state, effects) = reduce(state, &view, "1");
        assert_eq!(
            state,
            PromptState::Form {
                revision: 4,
                form: Form::Targets {
                    action: TargetAction::Skill {
                        position: PositionV1::Main,
                        skill: 0
                    },
                    targets: TargetList::empty()
                }
            }
        );
        assert_eq!(effects, vec![PromptEffect::Render(target_lines())]);

        let (state, _) = reduce(state, &view, "5");
        let (end_state, end_effects) = reduce(state, &view, "2");
        assert_eq!(end_state, PromptState::Menu { revision: 4 });
        assert_eq!(
            end_effects,
            vec![PromptEffect::Submit {
                revision: 4,
                action: ActionV1::ActivateSkill {
                    player: player(seat),
                    position: PositionV1::Main,
                    ability: EntityIdV1("top-skill".to_string()),
                    targets: Vec::new(),
                    mana_hint: Some(ManaTypeV1::Mind),
                },
            }]
        );
    }
}

#[test]
fn activate_skill_opponent_board_is_not_consulted() {
    for seat in SEATS {
        let opponent = match seat {
            Seat::One => Seat::Two,
            Seat::Two => Seat::One,
        };
        let opponent_card = card_with_abilities(
            "Guardian",
            vec![ability(AbilityKind::Skill, "skill-1", "Rally")],
        );
        let cross_view = with_board_for(
            view(seat),
            opponent,
            main_board(chain_summon(vec![opponent_card])),
        );

        let (state, effects) = reduce(
            PromptState::Form {
                revision: 4,
                form: Form::SkillPosition,
            },
            &cross_view,
            "1",
        );
        assert_eq!(effects, vec![PromptEffect::Render(Vec::new())]);

        let (end_state, end_effects) = reduce(state, &cross_view, "1");
        assert_eq!(end_state, PromptState::Menu { revision: 4 });
        assert_eq!(end_effects, invalid_render());
    }
}

#[test]
fn activate_skill_zero_targets_each_mana_hint_submits_for_both_seats() {
    for seat in SEATS {
        let card = card_with_abilities(
            "Guardian",
            vec![ability(AbilityKind::Skill, "skill-1", "Rally")],
        );
        let view = with_board_for(view(seat), seat, main_board(chain_summon(vec![card])));
        let action = TargetAction::Skill {
            position: PositionV1::Main,
            skill: 0,
        };
        let state = PromptState::Form {
            revision: 4,
            form: Form::Targets {
                action,
                targets: TargetList::empty(),
            },
        };

        let (hint_state, hint_effects) = reduce(state, &view, "5");
        assert_eq!(
            hint_state,
            PromptState::Form {
                revision: 4,
                form: Form::ManaHint {
                    action,
                    targets: TargetList::empty()
                }
            }
        );
        assert_eq!(hint_effects, vec![PromptEffect::Render(mana_hint_lines())]);

        for (input, hint) in [
            ("1", Some(ManaTypeV1::Matter)),
            ("2", Some(ManaTypeV1::Mind)),
            ("3", Some(ManaTypeV1::Spirit)),
            ("4", None),
        ] {
            let (end_state, end_effects) = reduce(hint_state, &view, input);
            assert_eq!(end_state, PromptState::Menu { revision: 4 });
            assert_eq!(
                end_effects,
                vec![PromptEffect::Submit {
                    revision: 4,
                    action: ActionV1::ActivateSkill {
                        player: player(seat),
                        position: PositionV1::Main,
                        ability: EntityIdV1("skill-1".to_string()),
                        targets: Vec::new(),
                        mana_hint: hint,
                    },
                }]
            );
        }

        let (invalid_state, invalid_effects) = reduce(hint_state, &view, "0");
        assert_eq!(invalid_state, PromptState::Menu { revision: 4 });
        assert_eq!(invalid_effects, invalid_render());
    }
}

#[test]
fn activate_skill_targets_duplicate_position_preserved_in_order() {
    let card = card_with_abilities(
        "Guardian",
        vec![ability(AbilityKind::Skill, "skill-1", "Rally")],
    );
    let view = with_board_for(
        view(Seat::Two),
        Seat::Two,
        main_board(chain_summon(vec![card])),
    );
    let action = TargetAction::Skill {
        position: PositionV1::Main,
        skill: 0,
    };
    let mut state = PromptState::Form {
        revision: 4,
        form: Form::Targets {
            action,
            targets: TargetList::empty(),
        },
    };
    for input in ["4", "4", "2"] {
        let (next_state, _) = reduce(state, &view, input);
        state = next_state;
    }
    let (state, _) = reduce(state, &view, "5");
    let (end_state, end_effects) = reduce(state, &view, "1");
    assert_eq!(end_state, PromptState::Menu { revision: 4 });
    assert_eq!(
        end_effects,
        vec![PromptEffect::Submit {
            revision: 4,
            action: ActionV1::ActivateSkill {
                player: PlayerIdV1::Two,
                position: PositionV1::Main,
                ability: EntityIdV1("skill-1".to_string()),
                targets: vec![
                    PositionV1::Bench {
                        slot: BenchSlotV1::Third
                    },
                    PositionV1::Bench {
                        slot: BenchSlotV1::Third
                    },
                    PositionV1::Bench {
                        slot: BenchSlotV1::First
                    },
                ],
                mana_hint: Some(ManaTypeV1::Matter),
            },
        }]
    );
}

#[test]
fn activate_skill_targets_fifth_append_rejected() {
    let card = card_with_abilities(
        "Guardian",
        vec![ability(AbilityKind::Skill, "skill-1", "Rally")],
    );
    let view = with_board_for(
        view(Seat::One),
        Seat::One,
        main_board(chain_summon(vec![card])),
    );
    let action = TargetAction::Skill {
        position: PositionV1::Main,
        skill: 0,
    };
    let mut state = PromptState::Form {
        revision: 4,
        form: Form::Targets {
            action,
            targets: TargetList::empty(),
        },
    };
    for input in ["1", "2", "3", "4"] {
        let (next_state, _) = reduce(state, &view, input);
        state = next_state;
    }
    let (end_state, end_effects) = reduce(state, &view, "2");
    assert_eq!(end_state, PromptState::Menu { revision: 4 });
    assert_eq!(end_effects, invalid_render());
}

#[test]
fn activate_skill_cancel_at_every_field() {
    let view = view(Seat::One);
    let forms = [
        Form::SkillPosition,
        Form::SkillAbility {
            position: PositionV1::Main,
        },
        Form::Targets {
            action: TargetAction::Skill {
                position: PositionV1::Main,
                skill: 0,
            },
            targets: TargetList::empty(),
        },
        Form::ManaHint {
            action: TargetAction::Skill {
                position: PositionV1::Main,
                skill: 0,
            },
            targets: TargetList::empty(),
        },
    ];
    for form in forms {
        for input in ["cancel", "CANCEL", "  cancel "] {
            let (state, effects) = reduce(PromptState::Form { revision: 4, form }, &view, input);
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
}

#[test]
fn spell_and_skill_forms_are_cancelled_on_revision_change_and_kept_on_match() {
    let forms = [
        Form::CastCard,
        Form::SkillPosition,
        Form::SkillAbility {
            position: PositionV1::Main,
        },
        Form::Targets {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
        Form::Targets {
            action: TargetAction::Skill {
                position: PositionV1::Main,
                skill: 0,
            },
            targets: TargetList::empty(),
        },
        Form::ManaHint {
            action: TargetAction::Cast(9),
            targets: TargetList::empty(),
        },
        Form::ManaHint {
            action: TargetAction::Skill {
                position: PositionV1::Main,
                skill: 0,
            },
            targets: TargetList::empty(),
        },
    ];
    for form in forms {
        let state = PromptState::Form { revision: 4, form };

        let (changed_state, changed_effects) = revised(&state, 5);
        assert_eq!(changed_state, PromptState::Menu { revision: 5 });
        assert_eq!(changed_effects, vec![PromptEffect::Cancelled]);

        let (kept_state, kept_effects) = revised(&state, 4);
        assert_eq!(kept_state, state);
        assert_eq!(kept_effects, Vec::new());
    }
}

#[test]
fn menu_prompt_lists_cast_spell_and_activate_skill_and_rejects_out_of_range() {
    for seat in SEATS {
        let view = view(seat);
        assert!(prompt(&view, 4)[0].contains("9. Cast Spell 10. Activate Skill"));
        for input in ["0", "11"] {
            let (state, effects) = reduce(PromptState::Menu { revision: 4 }, &view, input);
            assert_eq!(state, PromptState::Menu { revision: 4 });
            assert_eq!(effects, invalid_render());
        }
    }
}
