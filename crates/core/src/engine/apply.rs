//! The public transition and its actor gate.
//!
//! `apply` is a pure function: the same `GameState` and the same
//! `GameAction` always produce the same `ActionOutcome`, and a rejected
//! action leaves the caller's state untouched (the design's transition
//! contract). Before any handler runs, the actor gate enforces decision 15:
//! a finished game rejects everything, then `pending` (if set) names the
//! only legal actor, then an open Priority window, then the active player.

use crate::domain::actions::GameAction;
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::PlayerId;
use crate::domain::state::{GameState, PendingInput};

/// The result of one accepted action: the next state and the ordered facts
/// that describe how it got there (decision 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub state: GameState,
    pub events: Vec<GameEvent>,
}

/// Apply one action to `state`, returning the next state and its events, or
/// the typed reason the action could not proceed.
pub fn apply(state: &GameState, action: &GameAction) -> Result<ActionOutcome, ActionError> {
    if state.outcome.is_some() {
        return Err(ActionError::GameAlreadyOver);
    }

    check_actor(state, action.actor())?;

    dispatch(state, action)
}

/// The one player currently allowed to act (decision 15). A window can be
/// open regardless of which phase is resting (a Spell cast proactively in
/// Main, a respondable trigger during Upkeep or Main resolution, or a
/// declared attack in Combat), so this checks `state.turn.window` directly
/// rather than matching on the phase.
fn required_actor(state: &GameState) -> PlayerId {
    if let Some(pending) = &state.pending {
        return pending_actor(pending);
    }
    if let Some(window) = &state.turn.window {
        return window.holder;
    }
    state.turn.active_player
}

/// The player a paused decision is waiting on.
fn pending_actor(pending: &PendingInput) -> PlayerId {
    match pending {
        PendingInput::ManaProduction { player, .. } => *player,
        PendingInput::Promotion { player } => *player,
        PendingInput::PrizePick { chooser } => *chooser,
    }
}

/// Reject an action from anyone but the required actor.
fn check_actor(state: &GameState, actor: PlayerId) -> Result<(), ActionError> {
    if actor == required_actor(state) {
        Ok(())
    } else {
        Err(ActionError::NotYourDecision)
    }
}

/// Route an action that passed the actor gate to its handler.
///
/// No handler has landed yet: every arm is a placeholder that rejects with
/// `ActionError::NotYetImplemented`. Later work replaces one arm at a time;
/// this shape exists so those changes touch a single line each.
fn dispatch(_state: &GameState, action: &GameAction) -> Result<ActionOutcome, ActionError> {
    match action {
        GameAction::PlaySummon { .. } => Err(ActionError::NotYetImplemented),
        GameAction::UpgradeSummon { .. } => Err(ActionError::NotYetImplemented),
        GameAction::CastSpell { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ActivateSkill { .. } => Err(ActionError::NotYetImplemented),
        GameAction::Retreat { .. } => Err(ActionError::NotYetImplemented),
        GameAction::DeclareAttack { .. } => Err(ActionError::NotYetImplemented),
        GameAction::EndTurn { .. } => Err(ActionError::NotYetImplemented),
        GameAction::PassPriority { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ConvertCoin { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ChooseManaType { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ChoosePromotion { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ChoosePrize { .. } => Err(ActionError::NotYetImplemented),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{BenchSlot, CardInstanceId};
    use crate::domain::state::{
        CardRef, GameOutcome, LossReason, ManaBank, PerPlayer, Phase, PlayerState, StackWindow,
        SummonInstance, TurnState, UpgradeChain,
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
                active_player: PlayerId::One,
                phase: Phase::Main,
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

    fn end_turn(player: PlayerId) -> GameAction {
        GameAction::EndTurn { player }
    }

    #[test]
    fn a_finished_game_rejects_every_action_first() {
        let mut state = base_state();
        state.outcome = Some(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::ThirdMainLoss,
        });

        // The active player would otherwise be the legal actor; the wrong
        // player is used here to prove GameAlreadyOver wins even over a
        // mismatched actor (it must not read as NotYourDecision).
        let result = apply(&state, &end_turn(PlayerId::Two));

        assert_eq!(result, Err(ActionError::GameAlreadyOver));
    }

    #[test]
    fn an_action_from_the_wrong_player_is_rejected() {
        let state = base_state();

        let result = apply(&state, &end_turn(PlayerId::Two));

        assert_eq!(result, Err(ActionError::NotYourDecision));
    }

    #[test]
    fn a_pending_decision_names_the_only_legal_actor() {
        let mut state = base_state();
        state.turn.active_player = PlayerId::Two;
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        });
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        assert!(matches!(
            apply(&state, &end_turn(PlayerId::One)),
            Err(ActionError::NotYetImplemented)
        ));
        assert_eq!(
            apply(&state, &end_turn(PlayerId::Two)),
            Err(ActionError::NotYourDecision)
        );
    }

    #[test]
    fn an_open_priority_window_beats_the_active_player() {
        let mut state = base_state();
        state.turn.active_player = PlayerId::Two;
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });

        assert!(matches!(
            apply(&state, &end_turn(PlayerId::One)),
            Err(ActionError::NotYetImplemented)
        ));
        assert_eq!(
            apply(&state, &end_turn(PlayerId::Two)),
            Err(ActionError::NotYourDecision)
        );
    }

    #[test]
    fn an_open_priority_window_beats_the_active_player_outside_combat_too() {
        // A window can open in Main (a proactive Spell cast) or Upkeep (a
        // respondable trigger mid-resolution), not only in Combat. The
        // window must win regardless of which phase is resting.
        let mut state = base_state();
        state.turn.active_player = PlayerId::Two;
        state.turn.phase = Phase::Main;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });

        assert!(matches!(
            apply(&state, &end_turn(PlayerId::One)),
            Err(ActionError::NotYetImplemented)
        ));
        assert_eq!(
            apply(&state, &end_turn(PlayerId::Two)),
            Err(ActionError::NotYourDecision)
        );
    }

    #[test]
    fn with_no_pending_and_no_window_only_the_active_player_may_act() {
        let state = base_state();

        assert!(matches!(
            apply(&state, &end_turn(PlayerId::One)),
            Err(ActionError::NotYetImplemented)
        ));
    }

    #[test]
    fn every_dispatch_arm_currently_rejects_as_not_yet_implemented() {
        let state = base_state();
        let actions = vec![
            GameAction::PlaySummon {
                player: PlayerId::One,
                card: CardInstanceId(1),
                slot: BenchSlot::First,
            },
            GameAction::UpgradeSummon {
                player: PlayerId::One,
                card: CardInstanceId(1),
                position: crate::domain::ids::Position::Main,
            },
            GameAction::CastSpell {
                player: PlayerId::One,
                card: CardInstanceId(1),
                targets: vec![],
                mana_hint: None,
            },
            GameAction::ActivateSkill {
                player: PlayerId::One,
                position: crate::domain::ids::Position::Main,
                skill: crate::domain::actions::SkillIndex(0),
                targets: vec![],
                mana_hint: None,
            },
            GameAction::Retreat {
                player: PlayerId::One,
                slot: BenchSlot::First,
                mana_hint: None,
            },
            GameAction::DeclareAttack {
                player: PlayerId::One,
                target: crate::domain::ids::Position::Main,
                mana_hint: None,
            },
            end_turn(PlayerId::One),
            GameAction::PassPriority {
                player: PlayerId::One,
            },
            GameAction::ConvertCoin {
                player: PlayerId::One,
                mana_type: crate::domain::ids::ManaType::Matter,
            },
            GameAction::ChooseManaType {
                player: PlayerId::One,
                mana_type: crate::domain::ids::ManaType::Matter,
            },
            GameAction::ChoosePromotion {
                player: PlayerId::One,
                slot: BenchSlot::First,
            },
            GameAction::ChoosePrize {
                player: PlayerId::One,
                prize_index: 0,
            },
        ];

        assert_eq!(actions.len(), 12);
        for action in &actions {
            assert_eq!(apply(&state, action), Err(ActionError::NotYetImplemented));
        }
    }

    #[test]
    fn a_rejected_action_leaves_the_caller_free_to_reuse_its_state() {
        let state = base_state();
        let before = state.clone();

        let _ = apply(&state, &end_turn(PlayerId::Two));

        assert_eq!(state, before);
    }
}
