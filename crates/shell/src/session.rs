//! Pure classification of a recorded match's current game status.

use summoners_core::domain::state::{GameState, GameStatus};

/// Whether a recorded game can still accept actions, has reached a
/// terminal outcome, or has become unplayable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// The match can still accept and resolve an action.
    Playing,
    /// The match reached a terminal outcome — a win or a resignation.
    Ended,
    /// The match entered a broken state: a rule demanded a fact no entity
    /// printed.
    Broken,
}

/// Classify a recorded match's current game state.
#[must_use]
pub fn classify(state: &GameState) -> SessionStatus {
    match state.status {
        GameStatus::Playing => SessionStatus::Playing,
        GameStatus::Ended(_) => SessionStatus::Ended,
        GameStatus::Broken(_) => SessionStatus::Broken,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::{collections::VecDeque, sync::Arc};

    use summoners_core::domain::{
        cards::{Breakage, CardSet, ComponentKind, EntityId},
        ids::PlayerId,
        state::{GameOutcome, LossReason, ManaBank, PerPlayer, Phase, PlayerState, TurnState},
    };

    use super::*;

    fn state_with(status: GameStatus) -> GameState {
        let player = PlayerState {
            main: None,
            bench: [None, None, None],
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            enchantments: vec![],
        };
        GameState {
            players: PerPlayer::new(player.clone(), player),
            coin: None,
            turn: TurnState {
                active_player: PlayerId::One,
                phase: Phase::Main,
                window: None,
                normal_attack_used: false,
                normal_retreat_used: false,
                spell_played_this_turn: PerPlayer::new(false, false),
            },
            stack: vec![],
            stack_segment_bases: vec![],
            work: VecDeque::new(),
            pending: None,
            status,
            cards: Arc::new(CardSet::new(vec![])),
        }
    }

    #[test]
    fn playing_is_playing() {
        assert_eq!(
            classify(&state_with(GameStatus::Playing)),
            SessionStatus::Playing
        );
    }

    #[test]
    fn ended_is_ended() {
        let status = GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::EmptyDeckDraw,
        });

        assert_eq!(classify(&state_with(status)), SessionStatus::Ended);
    }

    #[test]
    fn broken_is_broken() {
        let status = GameStatus::Broken(Breakage {
            rule: "destruction",
            entity: EntityId::parse(&"bb".repeat(16)).expect("valid test entity ID"),
            expected: ComponentKind::Life,
        });

        assert_eq!(classify(&state_with(status)), SessionStatus::Broken);
    }
}
