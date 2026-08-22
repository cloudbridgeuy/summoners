use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use summoners_core::domain::errors::ActionError;

use super::ManaBankV1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ErrorV1 {
    NotYourDecision,
    GameAlreadyOver,
    GameBroken,
    WrongPhase,
    EmptyPosition,
    UnknownCard,
    InsufficientMana { short: ManaBankV1 },
    SummonExhausted,
    NormalAttackAlreadyUsed,
    NormalRetreatAlreadyUsed,
    AlreadyUpgradedThisTurn,
    PlayedThisTurn,
    IllegalUpgradeTarget,
    InvalidTarget,
    PendingInputMismatch,
    InvalidManaHint,
}

impl From<ActionError> for ErrorV1 {
    fn from(value: ActionError) -> Self {
        match value {
            ActionError::NotYourDecision => Self::NotYourDecision,
            ActionError::GameAlreadyOver => Self::GameAlreadyOver,
            ActionError::GameBroken => Self::GameBroken,
            ActionError::WrongPhase => Self::WrongPhase,
            ActionError::EmptyPosition => Self::EmptyPosition,
            ActionError::UnknownCard => Self::UnknownCard,
            ActionError::InsufficientMana { short } => Self::InsufficientMana {
                short: short.into(),
            },
            ActionError::SummonExhausted => Self::SummonExhausted,
            ActionError::NormalAttackAlreadyUsed => Self::NormalAttackAlreadyUsed,
            ActionError::NormalRetreatAlreadyUsed => Self::NormalRetreatAlreadyUsed,
            ActionError::AlreadyUpgradedThisTurn => Self::AlreadyUpgradedThisTurn,
            ActionError::PlayedThisTurn => Self::PlayedThisTurn,
            ActionError::IllegalUpgradeTarget => Self::IllegalUpgradeTarget,
            ActionError::InvalidTarget => Self::InvalidTarget,
            ActionError::PendingInputMismatch => Self::PendingInputMismatch,
            ActionError::InvalidManaHint => Self::InvalidManaHint,
        }
    }
}

impl From<ErrorV1> for ActionError {
    fn from(value: ErrorV1) -> Self {
        match value {
            ErrorV1::NotYourDecision => Self::NotYourDecision,
            ErrorV1::GameAlreadyOver => Self::GameAlreadyOver,
            ErrorV1::GameBroken => Self::GameBroken,
            ErrorV1::WrongPhase => Self::WrongPhase,
            ErrorV1::EmptyPosition => Self::EmptyPosition,
            ErrorV1::UnknownCard => Self::UnknownCard,
            ErrorV1::InsufficientMana { short } => Self::InsufficientMana {
                short: short.into(),
            },
            ErrorV1::SummonExhausted => Self::SummonExhausted,
            ErrorV1::NormalAttackAlreadyUsed => Self::NormalAttackAlreadyUsed,
            ErrorV1::NormalRetreatAlreadyUsed => Self::NormalRetreatAlreadyUsed,
            ErrorV1::AlreadyUpgradedThisTurn => Self::AlreadyUpgradedThisTurn,
            ErrorV1::PlayedThisTurn => Self::PlayedThisTurn,
            ErrorV1::IllegalUpgradeTarget => Self::IllegalUpgradeTarget,
            ErrorV1::InvalidTarget => Self::InvalidTarget,
            ErrorV1::PendingInputMismatch => Self::PendingInputMismatch,
            ErrorV1::InvalidManaHint => Self::InvalidManaHint,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use summoners_core::domain::state::ManaBank;

    #[test]
    fn every_action_error_variant_round_trips() {
        let errors = vec![
            ActionError::NotYourDecision,
            ActionError::GameAlreadyOver,
            ActionError::GameBroken,
            ActionError::WrongPhase,
            ActionError::EmptyPosition,
            ActionError::UnknownCard,
            ActionError::InsufficientMana {
                short: ManaBank {
                    matter: 1,
                    mind: 2,
                    spirit: 3,
                },
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
        ];

        for error in errors {
            assert_eq!(ActionError::from(ErrorV1::from(error)), error);
        }
    }
}
