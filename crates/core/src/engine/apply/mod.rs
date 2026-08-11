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
use crate::engine::{destruction, resolution, stack, turn};

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
/// Eleven of the twelve arms now call into their owning module's handler;
/// the rest are still placeholders that reject with
/// `ActionError::NotYetImplemented`. Later work replaces the remaining arms
/// one at a time; this shape exists so those changes touch a single line
/// each.
fn dispatch(state: &GameState, action: &GameAction) -> Result<ActionOutcome, ActionError> {
    match action {
        GameAction::PlaySummon { player, card, slot } => {
            crate::engine::board::play_summon(state, *player, *card, *slot)
        }
        GameAction::UpgradeSummon {
            player,
            card,
            position,
        } => crate::engine::board::upgrade_summon(state, *player, *card, *position),
        GameAction::CastSpell {
            player,
            card,
            targets,
            mana_hint,
        } => stack::cast_spell(state, *player, *card, targets.clone(), *mana_hint),
        GameAction::ActivateSkill { .. } => Err(ActionError::NotYetImplemented),
        GameAction::Retreat {
            player,
            slot,
            mana_hint,
        } => crate::engine::board::retreat(state, *player, *slot, *mana_hint),
        GameAction::DeclareAttack {
            player,
            target,
            mana_hint,
        } => stack::declare_attack(state, *player, *target, *mana_hint),
        GameAction::EndTurn { player } => turn::end_turn(state, *player),
        GameAction::PassPriority { player } => stack::pass(state, *player),
        GameAction::ConvertCoin { player, mana_type } => {
            turn::convert_coin(state, *player, *mana_type)
        }
        GameAction::ChooseManaType { player, mana_type } => {
            turn::choose_mana_type(state, *player, *mana_type)
        }
        GameAction::ChoosePromotion { player, slot } => {
            destruction::answer_promotion(state, *player, *slot)
        }
        GameAction::ChoosePrize {
            player,
            prize_index,
        } => destruction::answer_prize(state, *player, *prize_index),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
