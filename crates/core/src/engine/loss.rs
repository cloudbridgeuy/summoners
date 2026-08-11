//! Loss checks (rules §2).
//!
//! Only the draw-failure check is implemented so far: a player required to
//! draw from an empty Deck loses immediately (rules §2, §10 step 2, §58).
//! The other two losing conditions — a third Main Summon loss, and a Main
//! Summon loss with no Benched Summon to promote — depend on destruction
//! handling that has not landed; there is no stub for them here because
//! nothing yet reaches that check.

use crate::domain::events::GameEvent;
use crate::domain::ids::PlayerId;
use crate::domain::state::{GameOutcome, GameState, LossReason};

/// `player` was required to draw but their Deck was empty. Sets `outcome`
/// to a win for the opponent and returns the `GameEnded` fact. The
/// resolution loop stops as soon as it sees `outcome` set (rules §2: "as
/// soon as a player reaches [a losing condition], the game ends and
/// unresolved effects do not continue"), so nothing still queued after this
/// runs.
pub(crate) fn draw_failure(state: &GameState, player: PlayerId) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let outcome = GameOutcome {
        winner: player.opponent(),
        reason: LossReason::EmptyDeckDraw,
    };
    state.outcome = Some(outcome);

    (
        state,
        vec![GameEvent::GameEnded {
            winner: outcome.winner,
            reason: outcome.reason,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::CardInstanceId;
    use crate::domain::state::{
        CardRef, ManaBank, PerPlayer, Phase, PlayerState, SummonInstance, TurnState, UpgradeChain,
    };
    use std::collections::VecDeque;

    fn summon(owner: PlayerId) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(1),
                    def: CardDefId("quarry-whelp"),
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
            main: Some(summon(owner)),
            bench: [None, None, None],
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            has_coin: false,
        }
    }

    fn base_state() -> GameState {
        GameState {
            players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
            turn: TurnState {
                active_player: PlayerId::Two,
                phase: Phase::Upkeep,
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

    #[test]
    fn draw_failure_ends_the_game_for_the_drawing_players_opponent() {
        let state = base_state();

        let (state, events) = draw_failure(&state, PlayerId::Two);

        assert_eq!(
            state.outcome,
            Some(GameOutcome {
                winner: PlayerId::One,
                reason: LossReason::EmptyDeckDraw,
            })
        );
        assert_eq!(
            events,
            vec![GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: LossReason::EmptyDeckDraw,
            }]
        );
    }

    #[test]
    fn draw_failure_leaves_the_rest_of_state_untouched() {
        let state = base_state();

        let (next_state, _events) = draw_failure(&state, PlayerId::One);

        assert_eq!(next_state.players, state.players);
        assert_eq!(next_state.turn, state.turn);
        assert_eq!(next_state.work, state.work);
    }
}
