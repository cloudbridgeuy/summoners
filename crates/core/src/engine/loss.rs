//! Loss checks (rules §2): a player loses immediately once any of the three
//! losing conditions is met — a third Main Summon loss, a Main Summon loss
//! with no Benched Summon left to promote, or a required draw from an empty
//! Deck. "Immediately" means the resolution loop stops as soon as it sees
//! `status` leave `Playing` (rules §2: "as soon as a player reaches [a
//! losing condition], the game ends and unresolved effects do not
//! continue"), so nothing still queued after any of these runs.

use crate::domain::events::GameEvent;
use crate::domain::ids::PlayerId;
use crate::domain::state::{GameOutcome, GameState, GameStatus, LossReason};

/// Set `status` to a win for `loser`'s opponent and return the shared
/// `GameEnded` fact every losing path emits.
fn lose(state: &GameState, loser: PlayerId, reason: LossReason) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let outcome = GameOutcome {
        winner: loser.opponent(),
        reason,
    };
    state.status = GameStatus::Ended(outcome);

    (
        state,
        vec![GameEvent::GameEnded {
            winner: outcome.winner,
            reason: outcome.reason,
        }],
    )
}

/// `player` was required to draw but their Deck was empty (rules §2, §10
/// step 2, §58).
pub(crate) fn draw_failure(state: &GameState, player: PlayerId) -> (GameState, Vec<GameEvent>) {
    lose(state, player, LossReason::EmptyDeckDraw)
}

/// `WorkItem::LossCheck` (rules §24 step 6, §2). A third Main Summon loss
/// ends the game outright, ahead of whatever that same destruction's
/// Promotion step found — the third loss is decisive regardless of whether
/// a Bench Summon was available (rules §2). Short of a third loss, an empty
/// Main with no Benched Summon left is the other Main-destruction losing
/// condition. Neither condition can hold outside a Main destruction that
/// just ran, so calling this after a Bench destruction is a harmless no-op.
pub(crate) fn check(state: &GameState, player: PlayerId) -> (GameState, Vec<GameEvent>) {
    let player_state = state.players.get(player);
    if player_state.main_losses >= 3 {
        return lose(state, player, LossReason::ThirdMainLoss);
    }
    if player_state.main.is_none() && player_state.bench.iter().all(Option::is_none) {
        return lose(state, player, LossReason::NoPromotionAvailable);
    }
    (state.clone(), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cards::fixtures;
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
                    def: fixtures::id("quarry-whelp"),
                },
                vec![],
            ),
            damage: 0,
            readiness: crate::domain::state::Readiness::Ready,
            owner,
            controller: owner,
            duration_markers: vec![],
            turn: crate::domain::state::SummonTurnRecord::fresh(),
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
            enchantments: vec![],
        }
    }

    fn base_state() -> GameState {
        GameState {
            players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
            coin: None,
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
            status: GameStatus::Playing,
            cards: fixtures::card_set(),
        }
    }

    #[test]
    fn draw_failure_ends_the_game_for_the_drawing_players_opponent() {
        let state = base_state();

        let (state, events) = draw_failure(&state, PlayerId::Two);

        assert_eq!(
            state.status,
            GameStatus::Ended(GameOutcome {
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

    #[test]
    fn check_ends_the_game_on_a_third_main_loss() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main_losses = 3;

        let (state, events) = check(&state, PlayerId::One);

        assert_eq!(
            state.status,
            GameStatus::Ended(GameOutcome {
                winner: PlayerId::Two,
                reason: LossReason::ThirdMainLoss,
            })
        );
        assert_eq!(
            events,
            vec![GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: LossReason::ThirdMainLoss,
            }]
        );
    }

    #[test]
    fn check_ends_the_game_when_no_promotion_is_available() {
        let mut state = base_state();
        let one = state.players.get_mut(PlayerId::One);
        one.main = None;
        one.bench = [None, None, None];

        let (state, events) = check(&state, PlayerId::One);

        assert_eq!(
            state.status,
            GameStatus::Ended(GameOutcome {
                winner: PlayerId::Two,
                reason: LossReason::NoPromotionAvailable,
            })
        );
        assert_eq!(
            events,
            vec![GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: LossReason::NoPromotionAvailable,
            }]
        );
    }

    #[test]
    fn check_is_a_no_op_when_the_board_still_has_options() {
        let state = base_state();

        let (next_state, events) = check(&state, PlayerId::One);

        assert_eq!(next_state, state);
        assert!(events.is_empty());
    }

    #[test]
    fn check_is_a_no_op_when_main_is_empty_but_a_bench_summon_remains() {
        let mut state = base_state();
        let one = state.players.get_mut(PlayerId::One);
        one.main = None;
        one.bench = [Some(summon(PlayerId::One)), None, None];

        let (state, events) = check(&state, PlayerId::One);

        assert_eq!(state.status, GameStatus::Playing);
        assert!(events.is_empty());
    }

    #[test]
    fn check_prioritizes_the_third_main_loss_over_no_promotion_available() {
        let mut state = base_state();
        let one = state.players.get_mut(PlayerId::One);
        one.main = None;
        one.bench = [None, None, None];
        one.main_losses = 3;

        let (state, _events) = check(&state, PlayerId::One);

        assert_eq!(
            state.status,
            GameStatus::Ended(GameOutcome {
                winner: PlayerId::Two,
                reason: LossReason::ThirdMainLoss,
            })
        );
    }
}
