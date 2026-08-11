//! `ActionError` names the rule an action violated (decision 4: the shell
//! reads `state.pending`, or it tries an action and handles this error;
//! there is no third channel). `InvalidScenario` is `scenario::from_scenario`'s
//! companion: it names the shape violations parsing itself can detect
//! (decision 17).

use crate::domain::cards::CardDefId;
use crate::domain::ids::{CardInstanceId, PlayerId, Position};
use crate::domain::state::ManaBank;

/// Why `apply` rejected an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionError {
    /// The actor gate (decision 15): someone other than the current legal
    /// actor tried to act.
    NotYourDecision,
    /// `state.outcome` is already set; nothing more can happen.
    GameAlreadyOver,
    /// This action is not legal during the current phase or window.
    WrongPhase,
    /// The named position has no Summon on it.
    EmptyPosition,
    /// The named card is not in a zone this action can read from, or is not
    /// a card this action can use.
    UnknownCard,
    /// The cost could not be paid; `short` names how much of which typed
    /// pools were missing.
    InsufficientMana { short: ManaBank },
    /// The Summon is Exhausted and cannot voluntarily activate a Skill
    /// (rules §13–14).
    SummonExhausted,
    /// The turn's one normal attack is already spent (rules §29).
    NormalAttackAlreadyUsed,
    /// The turn's one normal Retreat is already spent (rules §26).
    NormalRetreatAlreadyUsed,
    /// This Summon already upgraded once this turn (rules §18).
    AlreadyUpgradedThisTurn,
    /// This Summon was played this turn and cannot upgrade yet (rules §17).
    PlayedThisTurn,
    /// The named upgrade does not legally follow the current chain top
    /// (rules §18, §20).
    IllegalUpgradeTarget,
    /// The named target position is not legal for this action.
    InvalidTarget,
    /// `pending` is set, the right player answered, but the action is not
    /// the kind of answer `pending` is waiting for.
    PendingInputMismatch,
    /// The supplied `mana_hint` cannot pay any Generic component of this
    /// cost (decision 14).
    InvalidManaHint,
    /// Placeholder for behavior that has not been built yet. Every action
    /// whose handler has not landed returns this; it carries no rule name
    /// because no rule was actually checked.
    NotYetImplemented,
}

/// Why `scenario::from_scenario` rejected a `Scenario`. Parsing accepts any
/// board it can interpret (decision 17): these five variants are the only
/// rejections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidScenario {
    /// The same `CardInstanceId` appears more than once across the board.
    DuplicateCardInstance(CardInstanceId),
    /// A card names a `CardDefId` the registry has no fixture for.
    UnknownCardDef(CardDefId),
    /// A Summon description names a chain with zero cards in it.
    EmptyUpgradeChain {
        player: PlayerId,
        position: Position,
    },
    /// A Summon description's chain does not climb Base, Enhanced, Elite in
    /// order (rules §20).
    IllegalChainOrder {
        player: PlayerId,
        position: Position,
    },
    /// This player has no Summon in Main and none on the Bench.
    NoSummonInPlay { player: PlayerId },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_error_variant_constructs() {
        let errors = vec![
            ActionError::NotYourDecision,
            ActionError::GameAlreadyOver,
            ActionError::WrongPhase,
            ActionError::EmptyPosition,
            ActionError::UnknownCard,
            ActionError::InsufficientMana {
                short: ManaBank::default(),
            },
            ActionError::SummonExhausted,
            ActionError::NormalAttackAlreadyUsed,
            ActionError::NormalRetreatAlreadyUsed,
            ActionError::AlreadyUpgradedThisTurn,
            ActionError::PlayedThisTurn,
            ActionError::IllegalUpgradeTarget,
            ActionError::InvalidTarget,
            ActionError::PendingInputMismatch,
            ActionError::InvalidManaHint,
            ActionError::NotYetImplemented,
        ];
        assert_eq!(errors.len(), 16);
    }

    #[test]
    fn every_invalid_scenario_variant_constructs() {
        let errors = [
            InvalidScenario::DuplicateCardInstance(CardInstanceId(1)),
            InvalidScenario::UnknownCardDef(CardDefId("missing")),
            InvalidScenario::EmptyUpgradeChain {
                player: PlayerId::One,
                position: Position::Main,
            },
            InvalidScenario::IllegalChainOrder {
                player: PlayerId::One,
                position: Position::Main,
            },
            InvalidScenario::NoSummonInPlay {
                player: PlayerId::One,
            },
        ];
        assert_eq!(errors.len(), 5);
    }
}
