//! `GameAction`: the closed set of moves an operator may submit to `apply`.
//!
//! Every action names its acting player explicitly; the actor gate in
//! `engine::apply` (decision 15) checks that name against `pending`, an open
//! Priority window, and the active player before any handler runs. Costs
//! never travel here — the engine reads them from the card tree (decision
//! 10) — and targets are battlefield positions, never Summon identities
//! (rules §30).

use crate::domain::cards::EntityId;
use crate::domain::ids::{BenchSlot, CardInstanceId, ManaType, PlayerId, Position};

/// One move an operator may submit to `apply`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameAction {
    /// Play a Base Summon from hand into an empty Bench slot (rules §17).
    PlaySummon {
        player: PlayerId,
        card: CardInstanceId,
        slot: BenchSlot,
    },
    /// Stack an Enhanced or Elite card onto the chain at `position` (rules
    /// §18–20).
    UpgradeSummon {
        player: PlayerId,
        card: CardInstanceId,
        position: Position,
    },
    /// Cast a Spell, proactively during Main or as a legal response (rules
    /// §34).
    CastSpell {
        player: PlayerId,
        card: CardInstanceId,
        targets: Vec<Position>,
        mana_hint: Option<ManaType>,
    },
    /// Activate one Skill of the Ready Summon at `position`, naming the
    /// ability by the id printed on its topmost card (rules §15).
    ActivateSkill {
        player: PlayerId,
        position: Position,
        ability: EntityId,
        targets: Vec<Position>,
        mana_hint: Option<ManaType>,
    },
    /// Perform the normal Retreat, swapping Main with the named Bench slot
    /// (rules §26).
    Retreat {
        player: PlayerId,
        slot: BenchSlot,
        mana_hint: Option<ManaType>,
    },
    /// Declare the normal attack against `target` (rules §29–30).
    DeclareAttack {
        player: PlayerId,
        target: Position,
        mana_hint: Option<ManaType>,
    },
    /// Decline to attack, or finish Combat once the Stack is settled
    /// (rules §47–48).
    EndTurn { player: PlayerId },
    /// Decline to add anything to an open Priority window (rules §32–33).
    PassPriority { player: PlayerId },
    /// Exchange the second player's Coin for one Mana (rules §7).
    ConvertCoin {
        player: PlayerId,
        mana_type: ManaType,
    },
    /// Answer a natural-production type choice (rules §11–12).
    ChooseManaType {
        player: PlayerId,
        mana_type: ManaType,
    },
    /// Answer a forced promotion by naming the Bench slot to promote
    /// (rules §24).
    ChoosePromotion { player: PlayerId, slot: BenchSlot },
    /// Answer a Prize recovery by naming which face-down Prize to reveal
    /// (rules §25).
    ChoosePrize {
        player: PlayerId,
        prize_index: usize,
    },
}

impl GameAction {
    /// The player who must be the legal actor for this action to proceed.
    pub fn actor(&self) -> PlayerId {
        match self {
            GameAction::PlaySummon { player, .. }
            | GameAction::UpgradeSummon { player, .. }
            | GameAction::CastSpell { player, .. }
            | GameAction::ActivateSkill { player, .. }
            | GameAction::Retreat { player, .. }
            | GameAction::DeclareAttack { player, .. }
            | GameAction::EndTurn { player }
            | GameAction::PassPriority { player }
            | GameAction::ConvertCoin { player, .. }
            | GameAction::ChooseManaType { player, .. }
            | GameAction::ChoosePromotion { player, .. }
            | GameAction::ChoosePrize { player, .. } => *player,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn sample_actions() -> Vec<GameAction> {
        vec![
            GameAction::PlaySummon {
                player: PlayerId::One,
                card: CardInstanceId(1),
                slot: BenchSlot::First,
            },
            GameAction::UpgradeSummon {
                player: PlayerId::One,
                card: CardInstanceId(2),
                position: Position::Main,
            },
            GameAction::CastSpell {
                player: PlayerId::One,
                card: CardInstanceId(3),
                targets: vec![Position::Main],
                mana_hint: Some(ManaType::Mind),
            },
            GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
                ability: EntityId::parse(&"0".repeat(32)).expect("valid probe id"),
                targets: vec![],
                mana_hint: None,
            },
            GameAction::Retreat {
                player: PlayerId::One,
                slot: BenchSlot::Second,
                mana_hint: None,
            },
            GameAction::DeclareAttack {
                player: PlayerId::One,
                target: Position::Main,
                mana_hint: None,
            },
            GameAction::EndTurn {
                player: PlayerId::One,
            },
            GameAction::PassPriority {
                player: PlayerId::One,
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
                player: PlayerId::One,
                prize_index: 0,
            },
        ]
    }

    #[test]
    fn every_game_action_variant_constructs() {
        assert_eq!(sample_actions().len(), 12);
    }

    #[test]
    fn actor_reads_the_named_player_for_every_variant() {
        for action in sample_actions() {
            let expected = match &action {
                GameAction::ConvertCoin { .. } => PlayerId::Two,
                _ => PlayerId::One,
            };
            assert_eq!(action.actor(), expected);
        }
    }
}
