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
use crate::engine::{resolution, upkeep};

/// The result of one accepted action: the next state and the ordered facts
/// that describe how it got there (decision 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionOutcome {
    pub state: GameState,
    pub events: Vec<GameEvent>,
}

/// Apply one action to `state`, returning the next state and its events, or
/// the typed reason the action could not proceed.
///
/// After a handler accepts the action, the resolution loop (`engine::resolution`)
/// drains any work it queued — Upkeep steps today, Stack resolution once
/// that lands — before the state and events are handed back (the design's
/// resolution loop: "the loop runs inside `apply` after every accepted
/// action"). A rejected action never reaches the drain, so its typed error
/// is the only thing the caller sees.
pub fn apply(state: &GameState, action: &GameAction) -> Result<ActionOutcome, ActionError> {
    if state.outcome.is_some() {
        return Err(ActionError::GameAlreadyOver);
    }

    check_actor(state, action.actor())?;

    let outcome = dispatch(state, action)?;
    let (state, drained_events) = resolution::drain(&outcome.state);

    let mut events = outcome.events;
    events.extend(drained_events);

    Ok(ActionOutcome { state, events })
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
/// Most arms are still placeholders that reject with
/// `ActionError::NotYetImplemented`; `EndTurn`, `ConvertCoin`, and
/// `ChooseManaType` now call into `engine::upkeep`. Later work replaces the
/// remaining arms one at a time; this shape exists so those changes touch a
/// single line each.
fn dispatch(state: &GameState, action: &GameAction) -> Result<ActionOutcome, ActionError> {
    match action {
        GameAction::PlaySummon { .. } => Err(ActionError::NotYetImplemented),
        GameAction::UpgradeSummon { .. } => Err(ActionError::NotYetImplemented),
        GameAction::CastSpell { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ActivateSkill { .. } => Err(ActionError::NotYetImplemented),
        GameAction::Retreat { .. } => Err(ActionError::NotYetImplemented),
        GameAction::DeclareAttack { .. } => Err(ActionError::NotYetImplemented),
        GameAction::EndTurn { player } => upkeep::end_turn(state, *player),
        GameAction::PassPriority { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ConvertCoin { player, mana_type } => {
            upkeep::convert_coin(state, *player, *mana_type)
        }
        GameAction::ChooseManaType { player, mana_type } => {
            upkeep::choose_mana_type(state, *player, *mana_type)
        }
        GameAction::ChoosePromotion { .. } => Err(ActionError::NotYetImplemented),
        GameAction::ChoosePrize { .. } => Err(ActionError::NotYetImplemented),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{BenchSlot, CardInstanceId, ManaType, Position};
    use crate::domain::state::{
        CardRef, GameOutcome, LossReason, ManaBank, ManaSource, PerPlayer, Phase, PlayerState,
        StackWindow, SummonInstance, TurnState, UpgradeChain,
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

    fn card_ref(instance: u32, def: &'static str) -> CardRef {
        CardRef {
            instance: CardInstanceId(instance),
            def: CardDefId(def),
        }
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

        // The actor gate lets `PlayerId::One` (the named pending answerer)
        // reach `end_turn`'s own handler, which then rejects it on its own
        // terms: `EndTurn` is not a decision answer, so it is illegal while
        // one is pending (`WrongPhase`), never a bare `NotYourDecision`.
        assert!(matches!(
            apply(&state, &end_turn(PlayerId::One)),
            Err(ActionError::WrongPhase)
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

        // The actor gate lets the window holder through; `end_turn` then
        // rejects it because a window is open (`WrongPhase`), not because
        // the actor gate stopped it.
        assert!(matches!(
            apply(&state, &end_turn(PlayerId::One)),
            Err(ActionError::WrongPhase)
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
            Err(ActionError::WrongPhase)
        ));
        assert_eq!(
            apply(&state, &end_turn(PlayerId::Two)),
            Err(ActionError::NotYourDecision)
        );
    }

    #[test]
    fn with_no_pending_and_no_window_only_the_active_player_may_act() {
        // `PlaySummon` has no handler yet, so it isolates the actor-gate
        // boundary from `EndTurn`'s now-real behavior.
        let state = base_state();
        let play_summon = GameAction::PlaySummon {
            player: PlayerId::One,
            card: CardInstanceId(1),
            slot: BenchSlot::First,
        };

        assert!(matches!(
            apply(&state, &play_summon),
            Err(ActionError::NotYetImplemented)
        ));
    }

    #[test]
    fn every_remaining_dispatch_arm_still_rejects_as_not_yet_implemented() {
        // `EndTurn`, `ConvertCoin`, and `ChooseManaType` now have real
        // handlers (`upkeep::end_turn`, `upkeep::convert_coin`,
        // `upkeep::choose_mana_type`), covered by their own behavior tests
        // above and in `engine::upkeep`. The other nine arms are still
        // this list's job to prove.
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
                position: Position::Main,
            },
            GameAction::CastSpell {
                player: PlayerId::One,
                card: CardInstanceId(1),
                targets: vec![],
                mana_hint: None,
            },
            GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
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
                target: Position::Main,
                mana_hint: None,
            },
            GameAction::PassPriority {
                player: PlayerId::One,
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

        assert_eq!(actions.len(), 9);
        for action in &actions {
            assert_eq!(apply(&state, action), Err(ActionError::NotYetImplemented));
        }
    }

    // -- Demo: EndTurn hands off, runs the opponent's Upkeep, and Mana
    // production either auto-produces or pauses for ChooseManaType;
    // ConvertCoin banks one anchored Mana and is one-use; an empty-deck
    // draw ends the game immediately. ---------------------------------------

    #[test]
    fn demo_end_turn_runs_a_mono_type_upkeep_without_pausing() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).deck = vec![card_ref(100, "quarry-whelp")];

        let outcome = apply(&state, &end_turn(PlayerId::One)).expect("legal from a resting Main");

        assert_eq!(outcome.state.pending, None);
        assert_eq!(outcome.state.turn.active_player, PlayerId::Two);
        assert_eq!(outcome.state.players.get(PlayerId::Two).mana.matter, 1);
        assert_eq!(
            outcome.events,
            vec![
                GameEvent::TurnBegan {
                    player: PlayerId::Two,
                },
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::CardDrawn {
                    player: PlayerId::Two,
                    card: CardInstanceId(100),
                },
                GameEvent::ManaProduced {
                    player: PlayerId::Two,
                    source: ManaSource::Player,
                    mana_type: ManaType::Matter,
                },
            ]
        );
    }

    #[test]
    fn demo_end_turn_pauses_on_a_dual_type_board_and_choose_mana_type_answers_it() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
            chain: UpgradeChain::new(card_ref(200, "set-path-adept"), vec![]),
            ..summon(PlayerId::Two)
        });
        state.players.get_mut(PlayerId::Two).deck = vec![card_ref(201, "quarry-whelp")];

        let outcome = apply(&state, &end_turn(PlayerId::One)).expect("legal from a resting Main");

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::TurnBegan {
                    player: PlayerId::Two,
                },
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::CardDrawn {
                    player: PlayerId::Two,
                    card: CardInstanceId(201),
                },
            ],
            "Set-Path Adept produces both Matter and Mind, so production pauses \
             before its own event"
        );
        assert_eq!(
            outcome.state.pending,
            Some(PendingInput::ManaProduction {
                player: PlayerId::Two,
                source: ManaSource::Player,
            })
        );
        assert_eq!(
            outcome.state.players.get(PlayerId::Two).mana,
            ManaBank::default()
        );

        let answered = apply(
            &outcome.state,
            &GameAction::ChooseManaType {
                player: PlayerId::Two,
                mana_type: ManaType::Mind,
            },
        )
        .expect("Mind is anchored to Set-Path Adept");

        assert_eq!(answered.state.pending, None);
        assert_eq!(answered.state.players.get(PlayerId::Two).mana.mind, 1);
        assert_eq!(
            answered.events,
            vec![GameEvent::ManaProduced {
                player: PlayerId::Two,
                source: ManaSource::Player,
                mana_type: ManaType::Mind,
            }]
        );
    }

    #[test]
    fn demo_convert_coin_banks_one_anchored_mana_and_removes_the_coin() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).has_coin = true;

        let outcome = apply(
            &state,
            &GameAction::ConvertCoin {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
            },
        )
        .expect("anchored, owner's own Main Phase");

        assert_eq!(
            outcome.events,
            vec![GameEvent::CoinConverted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
            }]
        );
        let player_state = outcome.state.players.get(PlayerId::One);
        assert_eq!(player_state.mana.matter, 1);
        assert!(!player_state.has_coin);

        // The Coin is one-use: converting again with no Coin left to spend
        // is rejected.
        assert_eq!(
            apply(
                &outcome.state,
                &GameAction::ConvertCoin {
                    player: PlayerId::One,
                    mana_type: ManaType::Matter,
                },
            ),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn demo_a_draw_from_an_empty_deck_ends_the_game_immediately() {
        // `base_state` already gives both players an empty Deck.
        let state = base_state();

        let outcome = apply(&state, &end_turn(PlayerId::One)).expect("legal from a resting Main");

        assert_eq!(
            outcome.state.outcome,
            Some(GameOutcome {
                winner: PlayerId::One,
                reason: LossReason::EmptyDeckDraw,
            })
        );
        assert_eq!(
            outcome.events,
            vec![
                GameEvent::TurnBegan {
                    player: PlayerId::Two,
                },
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::GameEnded {
                    winner: PlayerId::One,
                    reason: LossReason::EmptyDeckDraw,
                },
            ],
            "CardDrawn never fires and ManaProduced never runs after the loss"
        );

        // Loss is immediate (rules §2): every later action is rejected.
        assert_eq!(
            apply(&outcome.state, &end_turn(PlayerId::Two)),
            Err(ActionError::GameAlreadyOver)
        );
    }

    #[test]
    fn a_rejected_action_leaves_the_caller_free_to_reuse_its_state() {
        let state = base_state();
        let before = state.clone();

        let _ = apply(&state, &end_turn(PlayerId::Two));

        assert_eq!(state, before);
    }
}
