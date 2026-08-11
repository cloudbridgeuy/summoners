//! `GameEvent`: past-tense facts describing what changed. Events observe
//! the resulting `GameState`; they never drive it (decision 3). `apply`
//! returns the full ordered batch produced by one accepted action, including
//! every step the resolution loop drained automatically.

use crate::domain::actions::SkillIndex;
use crate::domain::cards::TriggerEvent;
use crate::domain::ids::{BenchSlot, CardInstanceId, ManaType, PlayerId, Position};
use crate::domain::state::{LossReason, ManaSource, StackItem};

/// One past-tense fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    /// Rules §9: a turn began for `player`.
    TurnBegan { player: PlayerId },
    /// Rules §10 step 1: every Summon `player` controls became Ready.
    SummonsReadied {
        player: PlayerId,
        positions: Vec<Position>,
    },
    /// Rules §10 step 2.
    CardDrawn {
        player: PlayerId,
        card: CardInstanceId,
    },
    /// Rules §10 step 3, §11.
    ManaProduced {
        player: PlayerId,
        source: ManaSource,
        mana_type: ManaType,
    },
    /// Rules §17.
    SummonPlayed {
        player: PlayerId,
        card: CardInstanceId,
        slot: BenchSlot,
    },
    /// Rules §18–20.
    SummonUpgraded {
        player: PlayerId,
        card: CardInstanceId,
        position: Position,
    },
    /// Rules §34.
    SpellCast {
        player: PlayerId,
        card: CardInstanceId,
        targets: Vec<Position>,
    },
    /// Rules §15.
    SkillActivated {
        player: PlayerId,
        position: Position,
        skill: SkillIndex,
    },
    /// Rules §29–31.
    AttackDeclared { player: PlayerId, target: Position },
    /// Rules §32–33.
    PriorityPassed { player: PlayerId },
    /// Rules §35.
    StackItemResolved { item: StackItem },
    /// Damage before and after one application, so a presentation layer can
    /// animate the change without recomputing it (rules §21).
    DamageApplied {
        position: Position,
        before: u32,
        after: u32,
    },
    /// Rules §22.
    Healed { position: Position, amount: u32 },
    /// Rules §23.
    SummonDestroyed { position: Position, owner: PlayerId },
    /// Rules §24 step 3, §25.
    PrizeRecovered {
        player: PlayerId,
        card: CardInstanceId,
    },
    /// Rules §24 step 4.
    SummonPromoted { player: PlayerId, from: BenchSlot },
    /// Rules §26: a normal Retreat exchanged Main and one Bench slot.
    SummonsSwapped { player: PlayerId, main: BenchSlot },
    /// Rules §28, §36–39.
    TriggerFired {
        controller: PlayerId,
        position: Position,
        event: TriggerEvent,
    },
    /// Rules §7.
    CoinConverted {
        player: PlayerId,
        mana_type: ManaType,
    },
    /// Rules §10 (payment policy, decision 14): Mana left the bank to pay a
    /// cost.
    ManaDeducted {
        player: PlayerId,
        mana_type: ManaType,
        amount: u32,
    },
    /// Rules §2: the match ended.
    GameEnded {
        winner: PlayerId,
        reason: LossReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_game_event_variant_constructs() {
        let events = vec![
            GameEvent::TurnBegan {
                player: PlayerId::One,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: CardInstanceId(1),
            },
            GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Player,
                mana_type: ManaType::Matter,
            },
            GameEvent::SummonPlayed {
                player: PlayerId::One,
                card: CardInstanceId(1),
                slot: BenchSlot::First,
            },
            GameEvent::SummonUpgraded {
                player: PlayerId::One,
                card: CardInstanceId(1),
                position: Position::Main,
            },
            GameEvent::SpellCast {
                player: PlayerId::One,
                card: CardInstanceId(1),
                targets: vec![Position::Main],
            },
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
            },
            GameEvent::AttackDeclared {
                player: PlayerId::One,
                target: Position::Main,
            },
            GameEvent::PriorityPassed {
                player: PlayerId::One,
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
            GameEvent::Healed {
                position: Position::Main,
                amount: 10,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::One,
            },
            GameEvent::PrizeRecovered {
                player: PlayerId::One,
                card: CardInstanceId(1),
            },
            GameEvent::SummonPromoted {
                player: PlayerId::One,
                from: BenchSlot::First,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::YourUpkeep,
            },
            GameEvent::CoinConverted {
                player: PlayerId::Two,
                mana_type: ManaType::Spirit,
            },
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: LossReason::ThirdMainLoss,
            },
        ];
        assert_eq!(events.len(), 21);
    }
}
