use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use summoners_core::domain::{
    actions::GameAction,
    ids::{CardInstanceId, ManaType, Position},
};

use crate::error::WireConversionError;

use super::{BenchSlotV1, EntityIdV1, PlayerIdV1, PositionV1, parse_entity_id};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManaTypeV1 {
    Matter,
    Mind,
    Spirit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionV1 {
    PlaySummon {
        player: PlayerIdV1,
        card: u32,
        slot: BenchSlotV1,
    },
    UpgradeSummon {
        player: PlayerIdV1,
        card: u32,
        position: PositionV1,
    },
    CastSpell {
        player: PlayerIdV1,
        card: u32,
        targets: Vec<PositionV1>,
        mana_hint: Option<ManaTypeV1>,
    },
    ActivateSkill {
        player: PlayerIdV1,
        position: PositionV1,
        ability: EntityIdV1,
        targets: Vec<PositionV1>,
        mana_hint: Option<ManaTypeV1>,
    },
    Retreat {
        player: PlayerIdV1,
        slot: BenchSlotV1,
        mana_hint: Option<ManaTypeV1>,
    },
    DeclareAttack {
        player: PlayerIdV1,
        target: PositionV1,
        mana_hint: Option<ManaTypeV1>,
    },
    EndTurn {
        player: PlayerIdV1,
    },
    PassPriority {
        player: PlayerIdV1,
    },
    ConvertCoin {
        player: PlayerIdV1,
        mana_type: ManaTypeV1,
    },
    ChooseManaType {
        player: PlayerIdV1,
        mana_type: ManaTypeV1,
    },
    ChoosePromotion {
        player: PlayerIdV1,
        slot: BenchSlotV1,
    },
    ChoosePrize {
        player: PlayerIdV1,
        prize_index: u64,
    },
    Resign {
        player: PlayerIdV1,
    },
}

impl From<ManaType> for ManaTypeV1 {
    fn from(value: ManaType) -> Self {
        match value {
            ManaType::Matter => Self::Matter,
            ManaType::Mind => Self::Mind,
            ManaType::Spirit => Self::Spirit,
        }
    }
}

impl From<ManaTypeV1> for ManaType {
    fn from(value: ManaTypeV1) -> Self {
        match value {
            ManaTypeV1::Matter => Self::Matter,
            ManaTypeV1::Mind => Self::Mind,
            ManaTypeV1::Spirit => Self::Spirit,
        }
    }
}

impl From<&GameAction> for ActionV1 {
    fn from(value: &GameAction) -> Self {
        match value {
            GameAction::PlaySummon { player, card, slot } => Self::PlaySummon {
                player: (*player).into(),
                card: card.0,
                slot: (*slot).into(),
            },
            GameAction::UpgradeSummon {
                player,
                card,
                position,
            } => Self::UpgradeSummon {
                player: (*player).into(),
                card: card.0,
                position: (*position).into(),
            },
            GameAction::CastSpell {
                player,
                card,
                targets,
                mana_hint,
            } => Self::CastSpell {
                player: (*player).into(),
                card: card.0,
                targets: targets.iter().copied().map(PositionV1::from).collect(),
                mana_hint: mana_hint.map(ManaTypeV1::from),
            },
            GameAction::ActivateSkill {
                player,
                position,
                ability,
                targets,
                mana_hint,
            } => Self::ActivateSkill {
                player: (*player).into(),
                position: (*position).into(),
                ability: (*ability).into(),
                targets: targets.iter().copied().map(PositionV1::from).collect(),
                mana_hint: mana_hint.map(ManaTypeV1::from),
            },
            GameAction::Retreat {
                player,
                slot,
                mana_hint,
            } => Self::Retreat {
                player: (*player).into(),
                slot: (*slot).into(),
                mana_hint: mana_hint.map(ManaTypeV1::from),
            },
            GameAction::DeclareAttack {
                player,
                target,
                mana_hint,
            } => Self::DeclareAttack {
                player: (*player).into(),
                target: (*target).into(),
                mana_hint: mana_hint.map(ManaTypeV1::from),
            },
            GameAction::EndTurn { player } => Self::EndTurn {
                player: (*player).into(),
            },
            GameAction::PassPriority { player } => Self::PassPriority {
                player: (*player).into(),
            },
            GameAction::ConvertCoin { player, mana_type } => Self::ConvertCoin {
                player: (*player).into(),
                mana_type: (*mana_type).into(),
            },
            GameAction::ChooseManaType { player, mana_type } => Self::ChooseManaType {
                player: (*player).into(),
                mana_type: (*mana_type).into(),
            },
            GameAction::ChoosePromotion { player, slot } => Self::ChoosePromotion {
                player: (*player).into(),
                slot: (*slot).into(),
            },
            GameAction::ChoosePrize {
                player,
                prize_index,
            } => Self::ChoosePrize {
                player: (*player).into(),
                prize_index: *prize_index as u64,
            },
            GameAction::Resign { player } => Self::Resign {
                player: (*player).into(),
            },
        }
    }
}

impl TryFrom<ActionV1> for GameAction {
    type Error = WireConversionError;

    fn try_from(value: ActionV1) -> Result<Self, Self::Error> {
        match value {
            ActionV1::PlaySummon { player, card, slot } => Ok(Self::PlaySummon {
                player: player.into(),
                card: CardInstanceId(card),
                slot: slot.into(),
            }),
            ActionV1::UpgradeSummon {
                player,
                card,
                position,
            } => Ok(Self::UpgradeSummon {
                player: player.into(),
                card: CardInstanceId(card),
                position: position.into(),
            }),
            ActionV1::CastSpell {
                player,
                card,
                targets,
                mana_hint,
            } => Ok(Self::CastSpell {
                player: player.into(),
                card: CardInstanceId(card),
                targets: targets.into_iter().map(Position::from).collect(),
                mana_hint: mana_hint.map(ManaType::from),
            }),
            ActionV1::ActivateSkill {
                player,
                position,
                ability,
                targets,
                mana_hint,
            } => Ok(Self::ActivateSkill {
                player: player.into(),
                position: position.into(),
                ability: parse_entity_id(ability)?,
                targets: targets.into_iter().map(Position::from).collect(),
                mana_hint: mana_hint.map(ManaType::from),
            }),
            ActionV1::Retreat {
                player,
                slot,
                mana_hint,
            } => Ok(Self::Retreat {
                player: player.into(),
                slot: slot.into(),
                mana_hint: mana_hint.map(ManaType::from),
            }),
            ActionV1::DeclareAttack {
                player,
                target,
                mana_hint,
            } => Ok(Self::DeclareAttack {
                player: player.into(),
                target: target.into(),
                mana_hint: mana_hint.map(ManaType::from),
            }),
            ActionV1::EndTurn { player } => Ok(Self::EndTurn {
                player: player.into(),
            }),
            ActionV1::PassPriority { player } => Ok(Self::PassPriority {
                player: player.into(),
            }),
            ActionV1::ConvertCoin { player, mana_type } => Ok(Self::ConvertCoin {
                player: player.into(),
                mana_type: mana_type.into(),
            }),
            ActionV1::ChooseManaType { player, mana_type } => Ok(Self::ChooseManaType {
                player: player.into(),
                mana_type: mana_type.into(),
            }),
            ActionV1::ChoosePromotion { player, slot } => Ok(Self::ChoosePromotion {
                player: player.into(),
                slot: slot.into(),
            }),
            ActionV1::ChoosePrize {
                player,
                prize_index,
            } => Ok(Self::ChoosePrize {
                player: player.into(),
                prize_index: usize::try_from(prize_index).map_err(|_| {
                    WireConversionError::IntegerOutOfRange {
                        field: "prize_index",
                        value: prize_index,
                    }
                })?,
            }),
            ActionV1::Resign { player } => Ok(Self::Resign {
                player: player.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use summoners_core::domain::cards::EntityId;
    use summoners_core::domain::ids::{BenchSlot, PlayerId};

    #[test]
    fn every_action_variant_round_trips() {
        let ability = EntityId::parse(&"01".repeat(16)).expect("valid test entity ID");
        let actions = vec![
            GameAction::PlaySummon {
                player: PlayerId::One,
                card: CardInstanceId(1),
                slot: BenchSlot::First,
            },
            GameAction::UpgradeSummon {
                player: PlayerId::Two,
                card: CardInstanceId(2),
                position: Position::Bench(BenchSlot::Second),
            },
            GameAction::CastSpell {
                player: PlayerId::One,
                card: CardInstanceId(3),
                targets: vec![Position::Main],
                mana_hint: Some(ManaType::Mind),
            },
            GameAction::ActivateSkill {
                player: PlayerId::Two,
                position: Position::Main,
                ability,
                targets: vec![Position::Bench(BenchSlot::Third)],
                mana_hint: Some(ManaType::Spirit),
            },
            GameAction::Retreat {
                player: PlayerId::One,
                slot: BenchSlot::First,
                mana_hint: None,
            },
            GameAction::DeclareAttack {
                player: PlayerId::One,
                target: Position::Main,
                mana_hint: Some(ManaType::Matter),
            },
            GameAction::EndTurn {
                player: PlayerId::One,
            },
            GameAction::PassPriority {
                player: PlayerId::Two,
            },
            GameAction::ConvertCoin {
                player: PlayerId::Two,
                mana_type: ManaType::Spirit,
            },
            GameAction::ChooseManaType {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
            },
            GameAction::ChoosePromotion {
                player: PlayerId::One,
                slot: BenchSlot::Third,
            },
            GameAction::ChoosePrize {
                player: PlayerId::Two,
                prize_index: 7,
            },
            GameAction::Resign {
                player: PlayerId::One,
            },
        ];

        for action in actions {
            let rebuilt =
                GameAction::try_from(ActionV1::from(&action)).expect("the wire action is valid");
            assert_eq!(rebuilt, action);
        }
    }

    #[test]
    fn invalid_ability_id_is_a_typed_conversion_error() {
        let wire = ActionV1::ActivateSkill {
            player: PlayerIdV1::One,
            position: PositionV1::Main,
            ability: EntityIdV1("not-an-id".to_string()),
            targets: vec![],
            mana_hint: None,
        };

        assert_eq!(
            GameAction::try_from(wire),
            Err(WireConversionError::InvalidEntityId {
                value: "not-an-id".to_string()
            })
        );
    }
}
