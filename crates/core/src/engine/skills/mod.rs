//! The `ActivateSkill` handler (rules §15).
//!
//! Rules §15 fixes the order: the Summon must be Ready, its controller
//! declares one Skill, the Mana cost (which may be zero) is paid, the Summon
//! turns sideways and becomes Exhausted, then the Skill resolves. This
//! handler follows that order exactly — payment before exhaustion, and
//! exhaustion before the Skill's effects run through `engine::effects`.
//!
//! Whether a Skill's effects resolve immediately or create a respondable
//! Stack effect is a property of the Skill's own printed text, not of
//! activation itself (rules §43: "A Skill does not automatically use the
//! Stack merely because it is activated. Its card text determines whether it
//! resolves immediately or creates a respondable Stack effect."). None of
//! this crate's Skill fixtures print Stack-effect text, so every Skill this
//! handler activates resolves immediately, the same turn its cost is paid —
//! consistent with §45 listing Skill activation among the active player's
//! ordinary Main Phase actions, alongside playing a Summon or a normal
//! Retreat, none of which use the Stack either.

use crate::domain::cards::{Cost, EffectLeaf, EntityId, Skill};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{ManaType, PlayerId, Position};
use crate::domain::state::{
    DurationMarker, GameState, Phase, PlayerState, Readiness, SummonInstance,
};
use crate::engine::apply::ActionOutcome;
use crate::engine::effects;
use crate::engine::payment::{self, PaymentError};

/// The Summon at `position`, if any.
fn summon_at(player: &PlayerState, position: Position) -> Option<&SummonInstance> {
    match position {
        Position::Main => player.main.as_ref(),
        Position::Bench(slot) => player.bench[slot.index()].as_ref(),
    }
}

/// The Summon at `position`, mutably, if any.
fn summon_at_mut(player: &mut PlayerState, position: Position) -> Option<&mut SummonInstance> {
    match position {
        Position::Main => player.main.as_mut(),
        Position::Bench(slot) => player.bench[slot.index()].as_mut(),
    }
}

/// Reject an action outside Main with nothing open on the Stack, or while a
/// paused decision names a different kind of answer than this action. Rules
/// §45 lists Skill activation among the active player's ordinary Main Phase
/// actions; nothing in §15 or its neighbours grants a Skill the extra
/// response timing a Spell gets (rules §32–34), so this handler gates
/// identically to `engine::board::play_summon` and `engine::board::retreat`.
fn require_free_main_phase(state: &GameState) -> Result<(), ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }
    if state.turn.phase != Phase::Main || state.turn.window.is_some() {
        return Err(ActionError::WrongPhase);
    }
    Ok(())
}

/// Fold `effects` over `state` through the shared leaf interpreter, in
/// printed order, the same way `engine::resolution` runs a Spell's or an
/// attack's effects — including `engine::resolution::apply_leaves`'s same
/// immutable-Damage gate on `ConditionalBonus` (rules §30); no current
/// Skill fixture pairs the two, but a Skill's effects fold through the same
/// vocabulary a Trigger or an Attack does, so this copy stays consistent
/// with it.
fn apply_leaves(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    effects: &[EffectLeaf],
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();
    let damage_is_immutable = effects::immutable_damage_in(effects);
    for leaf in effects {
        if damage_is_immutable && matches!(leaf, EffectLeaf::ConditionalBonus { .. }) {
            continue;
        }
        let (next_state, leaf_events) = effects::apply_leaf(&state, controller, targets, leaf);
        state = next_state;
        events.extend(leaf_events);
    }
    (state, events)
}

/// Reject a `MoveSummon` or `SwapPositions` leaf's targets before any
/// payment or mutation happens — the same eager-validation shape
/// `engine::board::play_summon` uses for an occupied Bench slot. A Skill
/// resolves immediately (rules §43), so there is no later Stack-resolution
/// moment where a changed target could turn a bad choice into a legal miss;
/// an illegal target simply means the whole action is illegal.
fn validate_targets(
    state: &GameState,
    controller: PlayerId,
    leaves: &[EffectLeaf],
    targets: &[Position],
) -> Result<(), ActionError> {
    let player_state = state.players.get(controller);
    for leaf in leaves {
        match leaf {
            EffectLeaf::MoveSummon => validate_move_summon(player_state, targets)?,
            EffectLeaf::SwapPositions => validate_swap_positions(player_state, targets)?,
            EffectLeaf::SwapOpposingPositions => {
                let opponent_state = state.players.get(controller.opponent());
                validate_swap_opposing_positions(opponent_state, targets)?
            }
            _ => {}
        }
    }
    Ok(())
}

/// `MoveSummon` moves one of `player_state`'s own Summons between two Bench
/// slots (rules §27: Main can never be voluntarily emptied without a
/// replacement, so a plain move never touches it — `SwapPositions` is the
/// leaf that replaces Main's occupant). The source must hold a Summon and
/// the destination must be empty.
fn validate_move_summon(
    player_state: &PlayerState,
    targets: &[Position],
) -> Result<(), ActionError> {
    let (Some(&from), Some(&to)) = (targets.first(), targets.get(1)) else {
        return Err(ActionError::InvalidTarget);
    };
    let (Position::Bench(from_slot), Position::Bench(to_slot)) = (from, to) else {
        return Err(ActionError::InvalidTarget);
    };
    if player_state.bench[from_slot.index()].is_none() {
        return Err(ActionError::InvalidTarget);
    }
    if player_state.bench[to_slot.index()].is_some() {
        return Err(ActionError::InvalidTarget);
    }
    Ok(())
}

/// `SwapPositions` exchanges `player_state`'s Main Summon with the one on
/// the named Bench slot (the same exchange rules §26's normal Retreat
/// performs). The named slot must hold a Summon.
fn validate_swap_positions(
    player_state: &PlayerState,
    targets: &[Position],
) -> Result<(), ActionError> {
    let Some(&target) = targets.first() else {
        return Err(ActionError::InvalidTarget);
    };
    let Position::Bench(slot) = target else {
        return Err(ActionError::InvalidTarget);
    };
    if player_state.bench[slot.index()].is_none() {
        return Err(ActionError::InvalidTarget);
    }
    if player_state.main.is_none() {
        return Err(ActionError::InvalidTarget);
    }
    Ok(())
}

/// `SwapOpposingPositions` exchanges the opponent's Main Summon with the one
/// on their named Bench slot — the Warden's `Rearrange` (exchange branch;
/// rules §16). Unlike `validate_move_summon` and `validate_swap_positions`,
/// this reaches into the OPPONENT's board rather than the activating
/// player's own. The named slot must hold a Summon, the opponent's Main must
/// hold a Summon, and that Main must not carry
/// `DurationMarker::CannotBeMovedByOpponent` — the Old Sow's `Root and
/// Renew` answering it (rules §44). Rejecting here, before the leaf ever
/// runs, is the primary enforcement point for that rule; `engine::effects::
/// swap_opposing_positions` also re-checks the marker on its own as
/// defense-in-depth, but a rejection observed through `apply` should always
/// come from here, as `ActionError::InvalidTarget`, not as a silent miss.
fn validate_swap_opposing_positions(
    opponent_state: &PlayerState,
    targets: &[Position],
) -> Result<(), ActionError> {
    let Some(&target) = targets.first() else {
        return Err(ActionError::InvalidTarget);
    };
    let Position::Bench(slot) = target else {
        return Err(ActionError::InvalidTarget);
    };
    if opponent_state.bench[slot.index()].is_none() {
        return Err(ActionError::InvalidTarget);
    }
    let Some(main_summon) = opponent_state.main.as_ref() else {
        return Err(ActionError::InvalidTarget);
    };
    if main_summon
        .duration_markers
        .contains(&DurationMarker::CannotBeMovedByOpponent)
    {
        return Err(ActionError::InvalidTarget);
    }
    Ok(())
}

/// One `ActivateSkill` action's fields, bundled so `activate_skill` reads
/// them as one value instead of five separate arguments.
pub(crate) struct SkillActivation {
    pub player: PlayerId,
    pub position: Position,
    pub ability: EntityId,
    pub targets: Vec<Position>,
    pub mana_hint: Option<ManaType>,
}

/// Activate one Skill of the Ready Summon at `activation.position` (rules
/// §15).
pub(crate) fn activate_skill(
    state: &GameState,
    activation: SkillActivation,
) -> Result<ActionOutcome, ActionError> {
    let SkillActivation {
        player,
        position,
        ability,
        targets,
        mana_hint,
    } = activation;

    require_free_main_phase(state)?;

    let player_state = state.players.get(player);
    let Some(summon) = summon_at(player_state, position) else {
        return Err(ActionError::EmptyPosition);
    };
    match summon.readiness {
        Readiness::Ready => {}
        Readiness::Exhausted => return Err(ActionError::SummonExhausted),
    }

    let Some(top_entity) = state.cards.get(summon.chain.top().def) else {
        return Err(ActionError::UnknownCard);
    };
    // Naming an ability this card does not print reads the same as any
    // other illegal target on this action: `InvalidTarget` already covers
    // "the named target is not legal for this action" for occupied Bench
    // slots (`play_summon`) and empty ones (`retreat`); an ability id this
    // Summon's topmost card does not print is the same shape of mistake,
    // naming something this Summon does not actually have.
    let Some(skill) = top_entity
        .all::<Skill>()
        .into_iter()
        .find(|ability_entity| ability_entity.id == ability)
    else {
        return Err(ActionError::InvalidTarget);
    };
    let cost = skill.get::<Cost>().copied().unwrap_or_default();
    let effects: Vec<EffectLeaf> = skill.all::<EffectLeaf>().into_iter().cloned().collect();

    validate_targets(state, player, &effects, &targets)?;

    let payment = match payment::deduct(player_state.mana, cost, mana_hint) {
        Ok(payment) => payment,
        Err(PaymentError::Insufficient(short)) => {
            return Err(ActionError::InsufficientMana { short });
        }
        Err(PaymentError::InvalidHint) => return Err(ActionError::InvalidManaHint),
    };

    let mut next = state.clone();
    let next_player = next.players.get_mut(player);
    next_player.mana = payment.bank;
    // Rules §15: the Summon turns sideways and becomes Exhausted before its
    // Skill resolves — never after.
    if let Some(summon_mut) = summon_at_mut(next_player, position) {
        summon_mut.readiness = Readiness::Exhausted;
    }

    let mut events: Vec<GameEvent> = payment
        .deductions
        .iter()
        .map(|deduction| GameEvent::ManaDeducted {
            player,
            mana_type: deduction.mana_type,
            amount: deduction.amount,
        })
        .collect();
    events.push(GameEvent::SkillActivated {
        player,
        position,
        ability,
    });

    let (next, leaf_events) = apply_leaves(&next, player, &targets, &effects);
    events.extend(leaf_events);

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

#[cfg(test)]
mod tests;
