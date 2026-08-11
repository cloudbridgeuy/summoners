//! Priority windows, passing, declaring the normal attack, and casting a
//! Spell (rules §29–34, §46–47).
//!
//! A window opens with a named first holder and no pass recorded. Every
//! pass moves Priority to the other player; a pass while `prior_pass` is
//! already set is the second one in a row and closes the window (rules
//! §32–33). Closing the window is where control hands off: if the Stack
//! still holds something above the current segment, `engine::resolution`'s
//! drain picks it up next; if the Stack is completely empty, Combat (if it
//! ever began) is over and the turn hands to the opponent immediately
//! (rules §47–48), so `pass` calls straight into that handover rather than
//! leaving the state resting with nobody able to act.

use crate::domain::cards::{CardKind, Query, QueryResult, SpellTiming, find_def};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{CardInstanceId, ManaType, PlayerId, Position};
use crate::domain::state::{GameState, Phase, StackItem, StackWindow};
use crate::engine::apply::ActionOutcome;
use crate::engine::payment::{self, PaymentError};
use crate::engine::turn;

/// Declare the active player's one normal attack (rules §29–31). Nothing
/// requires the attacking Summon to be Ready: rules §15's Ready
/// requirement governs Skills only. Declaring begins Combat and pays the
/// printed Attack cost immediately, exactly like `engine::board::retreat`
/// pays a Retreat Cost; the attack itself goes onto the Stack rather than
/// resolving here, and the defending player receives Priority first (rules
/// §31, §46).
pub(crate) fn declare_attack(
    state: &GameState,
    player: PlayerId,
    target: Position,
    mana_hint: Option<ManaType>,
) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }
    // Checked before the phase/window guard so a second declaration is
    // reported as `NormalAttackAlreadyUsed` — the specific rule it broke —
    // even once the first declaration has already moved the phase to
    // Combat and closed the window behind it.
    if state.turn.normal_attack_used {
        return Err(ActionError::NormalAttackAlreadyUsed);
    }
    if state.turn.phase != Phase::Main || state.turn.window.is_some() {
        return Err(ActionError::WrongPhase);
    }
    if target != Position::Main {
        // Rules §30: a normal attack targets Main unless a card's text
        // grants another target; no vanilla fixture grants that yet.
        return Err(ActionError::InvalidTarget);
    }

    let player_state = state.players.get(player);
    let Some(main_summon) = &player_state.main else {
        return Err(ActionError::EmptyPosition);
    };
    let Some(top_def) = find_def(main_summon.chain.top().def) else {
        return Err(ActionError::UnknownCard);
    };
    let Some(QueryResult::Attack { cost, .. }) = top_def.find(Query::Attack) else {
        return Err(ActionError::UnknownCard);
    };

    let payment = match payment::deduct(player_state.mana, cost, mana_hint) {
        Ok(payment) => payment,
        Err(PaymentError::Insufficient(short)) => {
            return Err(ActionError::InsufficientMana { short });
        }
        Err(PaymentError::InvalidHint) => return Err(ActionError::InvalidManaHint),
    };

    let mut next = state.clone();
    next.turn.normal_attack_used = true;
    next.turn.phase = Phase::Combat;
    next.turn.window = Some(window_after_play(player));
    next.stack.push(StackItem::Attack {
        attacker: player,
        target,
    });
    next.players.get_mut(player).mana = payment.bank;

    let mut events: Vec<GameEvent> = payment
        .deductions
        .iter()
        .map(|deduction| GameEvent::ManaDeducted {
            player,
            mana_type: deduction.mana_type,
            amount: deduction.amount,
        })
        .collect();
    events.push(GameEvent::AttackDeclared { player, target });

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

/// Cast a Spell (rules §34). A Support Spell may be cast proactively during
/// its controller's own resting Main Phase (rules §45: "cast legal Support
/// Spells") or, like an Attack Spell, as a response while the caster holds
/// Priority (rules §34: "Support Spells may normally be played proactively
/// during their controller's turn or as a legal response"; §46: "the
/// defending player may play a legal Support Spell, play a legal Attack
/// Spell if its timing allows, or pass"). An Attack Spell may only be cast
/// as a response while the caster holds Priority — casting one proactively,
/// with no window open, is rejected the same as casting one out of Combat
/// altogether. Casting pays the printed cost exactly like `declare_attack`
/// pays an Attack's cost, pushes the Spell onto the Stack, opens the same
/// Priority window `declare_attack` and `engine::turn::end_turn` already
/// open (rules §32), and marks the turn's Spell flag.
pub(crate) fn cast_spell(
    state: &GameState,
    player: PlayerId,
    card: CardInstanceId,
    targets: Vec<Position>,
    mana_hint: Option<ManaType>,
) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }

    let player_state = state.players.get(player);
    let Some(hand_index) = player_state.hand.iter().position(|c| c.instance == card) else {
        return Err(ActionError::UnknownCard);
    };
    let card_ref = player_state.hand[hand_index];

    let Some(def) = find_def(card_ref.def) else {
        return Err(ActionError::UnknownCard);
    };
    if def.kind != CardKind::Spell {
        return Err(ActionError::UnknownCard);
    }
    let Some(QueryResult::Spell { timing, cost, .. }) = def.find(Query::Spell) else {
        return Err(ActionError::UnknownCard);
    };

    let resting_in_own_main = state.turn.window.is_none()
        && state.turn.phase == Phase::Main
        && player == state.turn.active_player;
    let holds_priority = matches!(state.turn.window, Some(window) if window.holder == player);
    let legal_timing = match timing {
        SpellTiming::Support => resting_in_own_main || holds_priority,
        SpellTiming::Attack => holds_priority,
    };
    if !legal_timing {
        return Err(ActionError::WrongPhase);
    }

    let payment = match payment::deduct(player_state.mana, cost, mana_hint) {
        Ok(payment) => payment,
        Err(PaymentError::Insufficient(short)) => {
            return Err(ActionError::InsufficientMana { short });
        }
        Err(PaymentError::InvalidHint) => return Err(ActionError::InvalidManaHint),
    };

    let mut next = state.clone();
    next.turn.window = Some(window_after_play(player));
    next.turn.spell_played_this_turn = true;
    next.stack.push(StackItem::Spell {
        caster: player,
        card: card_ref,
        targets: targets.clone(),
    });
    let next_player = next.players.get_mut(player);
    next_player.mana = payment.bank;
    next_player.hand.remove(hand_index);

    let mut events: Vec<GameEvent> = payment
        .deductions
        .iter()
        .map(|deduction| GameEvent::ManaDeducted {
            player,
            mana_type: deduction.mana_type,
            amount: deduction.amount,
        })
        .collect();
    events.push(GameEvent::SpellCast {
        player,
        card,
        targets,
    });

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

/// The Priority window that follows `player` declaring an attack, ending
/// their turn without attacking, or playing an effect (rules §31, §32,
/// §47). Rules §32 states the general fact: "whenever a player plays a
/// Spell or passes, Priority moves to the other player." `pass` below is
/// the passing half of that sentence; this is the playing half, and it is
/// the same value in every case — the other player becomes the holder and
/// the pass streak starts clean — so `declare_attack`, `cast_spell`, and
/// `engine::turn::end_turn`, which each open that window for their own
/// rule (§31, §34, and §47 respectively), build it here instead of
/// restating it and risking disagreement on the pass streak. Activating a
/// Skill that creates a Stack effect and a trigger that opens a response
/// opportunity (rules §38) will call this too.
pub(crate) fn window_after_play(player: PlayerId) -> StackWindow {
    StackWindow {
        holder: player.opponent(),
        prior_pass: false,
    }
}

/// `PassPriority` (rules §32–33). Priority moves to the other player;
/// nothing else changes until a second consecutive pass closes the window.
/// On that second pass, if the Stack has nothing left above the current
/// segment, `engine::turn::handover` runs immediately (rules §47–48) —
/// otherwise the window simply closes and `engine::resolution::drain`
/// resolves what is left the next time `apply` calls it.
pub(crate) fn pass(state: &GameState, player: PlayerId) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }
    let Some(window) = state.turn.window else {
        return Err(ActionError::WrongPhase);
    };
    if window.holder != player {
        return Err(ActionError::NotYourDecision);
    }

    let mut next = state.clone();
    let mut events = vec![GameEvent::PriorityPassed { player }];

    if window.prior_pass {
        next.turn.window = None;
        // The turn is only over once the Stack itself is completely empty
        // — not just the innermost segment settled. That is still a safe
        // test with a segment open (rules §38): `engine::triggers::fire`
        // pushes a segment's base at the same index it pushes that
        // segment's first item, and `engine::resolution` pops the base the
        // instant popping an item drains the Stack back down to it, so the
        // Stack can never be empty while an unpopped segment base remains.
        if next.stack.is_empty() {
            let handover = turn::handover(&next);
            events.extend(handover.events);
            next = handover.state;
        }
    } else {
        next.turn.window = Some(StackWindow {
            holder: player.opponent(),
            prior_pass: true,
        });
    }

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
