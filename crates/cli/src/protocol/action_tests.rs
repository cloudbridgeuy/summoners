#![allow(clippy::expect_used, clippy::unwrap_used)]
use super::*;
use summoners_match_log::wire::{BenchSlotV1, EntityIdV1, ManaTypeV1, PlayerIdV1, PositionV1};

fn assert_matching_seat_ok_and_other_seat_rejected(
    player: PlayerId,
    action: ActionV1,
    expected: GameAction,
) {
    assert_eq!(normalize_action(player, action.clone()), Ok(expected));
    assert_eq!(
        normalize_action(player.opponent(), action),
        Err(ProtocolError::Actor)
    );
}

fn mana_hint_cases() -> Vec<(Option<ManaTypeV1>, Option<ManaType>)> {
    vec![
        (None, None),
        (Some(ManaTypeV1::Matter), Some(ManaType::Matter)),
        (Some(ManaTypeV1::Mind), Some(ManaType::Mind)),
        (Some(ManaTypeV1::Spirit), Some(ManaType::Spirit)),
    ]
}

fn target_cases() -> Vec<(Vec<PositionV1>, Vec<Position>)> {
    vec![
        (vec![], vec![]),
        (vec![PositionV1::Main], vec![Position::Main]),
        (
            vec![
                PositionV1::Bench {
                    slot: BenchSlotV1::First,
                },
                PositionV1::Main,
                PositionV1::Bench {
                    slot: BenchSlotV1::Third,
                },
            ],
            vec![
                Position::Bench(BenchSlot::First),
                Position::Main,
                Position::Bench(BenchSlot::Third),
            ],
        ),
    ]
}

fn position_cases() -> Vec<(PositionV1, Position)> {
    vec![
        (PositionV1::Main, Position::Main),
        (
            PositionV1::Bench {
                slot: BenchSlotV1::First,
            },
            Position::Bench(BenchSlot::First),
        ),
        (
            PositionV1::Bench {
                slot: BenchSlotV1::Second,
            },
            Position::Bench(BenchSlot::Second),
        ),
        (
            PositionV1::Bench {
                slot: BenchSlotV1::Third,
            },
            Position::Bench(BenchSlot::Third),
        ),
    ]
}

fn bench_slot_cases() -> Vec<(BenchSlotV1, BenchSlot)> {
    vec![
        (BenchSlotV1::First, BenchSlot::First),
        (BenchSlotV1::Second, BenchSlot::Second),
        (BenchSlotV1::Third, BenchSlot::Third),
    ]
}

fn mana_type_cases() -> Vec<(ManaTypeV1, ManaType)> {
    vec![
        (ManaTypeV1::Matter, ManaType::Matter),
        (ManaTypeV1::Mind, ManaType::Mind),
        (ManaTypeV1::Spirit, ManaType::Spirit),
    ]
}

#[test]
fn play_summon_checks_every_bench_slot_and_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_slot, domain_slot) in bench_slot_cases() {
            let action = ActionV1::PlaySummon {
                player: player.into(),
                card: 7,
                slot: wire_slot,
            };
            let expected = GameAction::PlaySummon {
                player,
                card: CardInstanceId(7),
                slot: domain_slot,
            };
            assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
        }
    }
}

#[test]
fn upgrade_summon_checks_every_position_and_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_position, domain_position) in position_cases() {
            let action = ActionV1::UpgradeSummon {
                player: player.into(),
                card: 12,
                position: wire_position,
            };
            let expected = GameAction::UpgradeSummon {
                player,
                card: CardInstanceId(12),
                position: domain_position,
            };
            assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
        }
    }
}

#[test]
fn cast_spell_checks_target_lists_and_mana_hints_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_targets, domain_targets) in target_cases() {
            for (wire_mana, domain_mana) in mana_hint_cases() {
                let action = ActionV1::CastSpell {
                    player: player.into(),
                    card: 21,
                    targets: wire_targets.clone(),
                    mana_hint: wire_mana,
                };
                let expected = GameAction::CastSpell {
                    player,
                    card: CardInstanceId(21),
                    targets: domain_targets.clone(),
                    mana_hint: domain_mana,
                };
                assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
            }
        }
    }
}

#[test]
fn activate_skill_checks_position_targets_mana_hints_and_ability_for_both_seats() {
    let ability = EntityId::parse(&"5c".repeat(16)).expect("valid probe id");
    let wire_ability = EntityIdV1(ability.to_string());
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_position, domain_position) in position_cases() {
            for (wire_targets, domain_targets) in target_cases() {
                for (wire_mana, domain_mana) in mana_hint_cases() {
                    let action = ActionV1::ActivateSkill {
                        player: player.into(),
                        position: wire_position,
                        ability: wire_ability.clone(),
                        targets: wire_targets.clone(),
                        mana_hint: wire_mana,
                    };
                    let expected = GameAction::ActivateSkill {
                        player,
                        position: domain_position,
                        ability,
                        targets: domain_targets.clone(),
                        mana_hint: domain_mana,
                    };
                    assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
                }
            }
        }
    }
}

#[test]
fn activate_skill_with_an_unparsable_ability_id_is_a_conversion_error() {
    let action = ActionV1::ActivateSkill {
        player: PlayerIdV1::One,
        position: PositionV1::Main,
        ability: EntityIdV1("not-an-id".to_string()),
        targets: vec![],
        mana_hint: None,
    };
    assert!(matches!(
        normalize_action(PlayerId::One, action),
        Err(ProtocolError::Conversion(_))
    ));
}

#[test]
fn activate_skill_conversion_error_is_not_masked_by_a_mismatched_seat() {
    let action = ActionV1::ActivateSkill {
        player: PlayerIdV1::One,
        position: PositionV1::Main,
        ability: EntityIdV1("not-an-id".to_string()),
        targets: vec![],
        mana_hint: None,
    };
    assert!(matches!(
        normalize_action(PlayerId::Two, action),
        Err(ProtocolError::Conversion(_))
    ));
}

#[test]
fn retreat_checks_every_bench_slot_and_mana_hint_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_slot, domain_slot) in bench_slot_cases() {
            for (wire_mana, domain_mana) in mana_hint_cases() {
                let action = ActionV1::Retreat {
                    player: player.into(),
                    slot: wire_slot,
                    mana_hint: wire_mana,
                };
                let expected = GameAction::Retreat {
                    player,
                    slot: domain_slot,
                    mana_hint: domain_mana,
                };
                assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
            }
        }
    }
}

#[test]
fn declare_attack_checks_every_target_position_and_mana_hint_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_position, domain_position) in position_cases() {
            for (wire_mana, domain_mana) in mana_hint_cases() {
                let action = ActionV1::DeclareAttack {
                    player: player.into(),
                    target: wire_position,
                    mana_hint: wire_mana,
                };
                let expected = GameAction::DeclareAttack {
                    player,
                    target: domain_position,
                    mana_hint: domain_mana,
                };
                assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
            }
        }
    }
}

#[test]
fn end_turn_checks_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        let action = ActionV1::EndTurn {
            player: player.into(),
        };
        let expected = GameAction::EndTurn { player };
        assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
    }
}

#[test]
fn pass_priority_checks_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        let action = ActionV1::PassPriority {
            player: player.into(),
        };
        let expected = GameAction::PassPriority { player };
        assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
    }
}

#[test]
fn convert_coin_checks_every_mana_type_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_mana, domain_mana) in mana_type_cases() {
            let action = ActionV1::ConvertCoin {
                player: player.into(),
                mana_type: wire_mana,
            };
            let expected = GameAction::ConvertCoin {
                player,
                mana_type: domain_mana,
            };
            assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
        }
    }
}

#[test]
fn choose_mana_type_checks_every_mana_type_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_mana, domain_mana) in mana_type_cases() {
            let action = ActionV1::ChooseManaType {
                player: player.into(),
                mana_type: wire_mana,
            };
            let expected = GameAction::ChooseManaType {
                player,
                mana_type: domain_mana,
            };
            assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
        }
    }
}

#[test]
fn choose_promotion_checks_every_bench_slot_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for (wire_slot, domain_slot) in bench_slot_cases() {
            let action = ActionV1::ChoosePromotion {
                player: player.into(),
                slot: wire_slot,
            };
            let expected = GameAction::ChoosePromotion {
                player,
                slot: domain_slot,
            };
            assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
        }
    }
}

#[test]
fn choose_prize_checks_several_prize_indices_for_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        for prize_index in [0u64, 1, 7, u64::from(u32::MAX)] {
            let action = ActionV1::ChoosePrize {
                player: player.into(),
                prize_index,
            };
            let expected = GameAction::ChoosePrize {
                player,
                prize_index: usize::try_from(prize_index).expect("fits usize on this target"),
            };
            assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
        }
    }
}

#[test]
fn resign_checks_both_seats() {
    for player in [PlayerId::One, PlayerId::Two] {
        let action = ActionV1::Resign {
            player: player.into(),
        };
        let expected = GameAction::Resign { player };
        assert_matching_seat_ok_and_other_seat_rejected(player, action, expected);
    }
}
