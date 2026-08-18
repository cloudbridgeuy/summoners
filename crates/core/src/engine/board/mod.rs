//! Handlers for `PlaySummon`, `UpgradeSummon`, and `Retreat` (rules §17,
//! §18–20, §26).
//!
//! Each handler reads whatever it needs from the `GameState` it is given,
//! then clones that state and mutates the clone, so a rejected action never
//! touches the caller's original value (the design's transition contract).

use crate::domain::cards::{
    CardKind, Cost, Entity, Form, ManaTypes, Modifier, RetreatCost, family,
};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{BenchSlot, CardInstanceId, ManaType, PlayerId, Position};
use crate::domain::state::{
    GameState, MovementStep, Phase, PlayerState, SummonInstance, UpgradeChain, WorkItem,
};
use crate::engine::apply::ActionOutcome;
use crate::engine::payment::{self, PaymentError};

/// The Summon at `position`, if any.
fn summon_at(player: &PlayerState, position: Position) -> Option<&SummonInstance> {
    match position {
        Position::Main => player.main.as_ref(),
        Position::Bench(slot) => player.bench[slot.index()].as_ref(),
    }
}

/// The Mana Types `entity`'s topmost card prints, or none for a card with no
/// `Produces` component. A card may print more than one `Produces`
/// component; only the first anchors this Summon's characteristics.
fn produced_types(entity: &Entity) -> Vec<ManaType> {
    entity
        .get::<ManaTypes>()
        .map(|types| types.0.clone())
        .unwrap_or_default()
}

/// The Form `entity` prints, or none for a card with no `Form` component.
fn current_form(entity: &Entity) -> Option<Form> {
    entity.get::<Form>().copied()
}

/// The printed Retreat Cost on `entity`, or none for a card with no
/// `RetreatCost` component.
fn retreat_cost(entity: &Entity) -> Option<u32> {
    entity.get::<RetreatCost>().map(|cost| cost.0)
}

/// Rules §16: `player`'s opponent may print a Passive that raises the cost
/// of this Retreat (`Modifier::OpposingRetreatCostDelta`, the Warden of Set
/// Paths). Scans every Summon `player`'s opponent controls, reading each
/// one's topmost card only — the same "topmost card alone defines current
/// characteristics" reading `retreat_cost` and `produced_types` already
/// give the chain (rules §20) — and sums every matching Passive found. A
/// card may print more than one Passive; only the first of each card counts
/// toward this sum, the same "first anchors" reading `produced_types` gives
/// a card with more than one `Produces`.
fn opposing_retreat_cost_delta(state: &GameState, player: PlayerId) -> i32 {
    let opponent_state = state.players.get(player.opponent());
    let positions = [
        Position::Main,
        Position::Bench(BenchSlot::First),
        Position::Bench(BenchSlot::Second),
        Position::Bench(BenchSlot::Third),
    ];
    positions
        .iter()
        .filter_map(|&position| summon_at(opponent_state, position))
        .filter_map(|summon| state.cards.get(summon.chain.top().def))
        .filter_map(|top| match top.get::<Modifier>() {
            Some(Modifier::OpposingRetreatCostDelta(delta)) => Some(*delta),
            _ => None,
        })
        .sum()
}

/// Reject an action outside Main with nothing open on the Stack, or while a
/// paused decision names a different kind of answer than this action.
fn require_free_main_phase(state: &GameState) -> Result<(), ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }
    if state.turn.phase != Phase::Main || state.turn.window.is_some() {
        return Err(ActionError::WrongPhase);
    }
    Ok(())
}

/// Play a Base Summon from hand into an empty Bench slot (rules §17). It
/// enters Exhausted, `played_this_turn` set.
pub(crate) fn play_summon(
    state: &GameState,
    player: PlayerId,
    card: CardInstanceId,
    slot: BenchSlot,
) -> Result<ActionOutcome, ActionError> {
    require_free_main_phase(state)?;

    let player_state = state.players.get(player);
    if player_state.bench[slot.index()].is_some() {
        return Err(ActionError::InvalidTarget);
    }

    let Some(hand_index) = player_state.hand.iter().position(|c| c.instance == card) else {
        return Err(ActionError::UnknownCard);
    };
    let card_ref = player_state.hand[hand_index];

    let Some(entity) = state.cards.get(card_ref.def) else {
        return Err(ActionError::UnknownCard);
    };
    if family(entity) != CardKind::Summon || current_form(entity) != Some(Form::Base) {
        return Err(ActionError::UnknownCard);
    }

    let mut next = state.clone();
    let next_player = next.players.get_mut(player);
    next_player.hand.remove(hand_index);
    next_player.bench[slot.index()] = Some(SummonInstance {
        chain: UpgradeChain::new(card_ref, vec![]),
        damage: 0,
        ready: false,
        owner: player,
        controller: player,
        duration_markers: vec![],
        played_this_turn: true,
        upgraded_this_turn: false,
        entered_main_this_turn: false,
    });

    Ok(ActionOutcome {
        state: next,
        events: vec![GameEvent::SummonPlayed { player, card, slot }],
    })
}

/// Stack an Enhanced or Elite card from hand onto the chain at `position`
/// (rules §18–20). The Form must strictly climb and the new topmost card
/// must print every Mana Type the current top prints (rules §18); either
/// failure is `IllegalUpgradeTarget`.
pub(crate) fn upgrade_summon(
    state: &GameState,
    player: PlayerId,
    card: CardInstanceId,
    position: Position,
) -> Result<ActionOutcome, ActionError> {
    require_free_main_phase(state)?;

    let player_state = state.players.get(player);
    let Some(summon) = summon_at(player_state, position) else {
        return Err(ActionError::EmptyPosition);
    };
    if summon.played_this_turn {
        return Err(ActionError::PlayedThisTurn);
    }
    if summon.upgraded_this_turn {
        return Err(ActionError::AlreadyUpgradedThisTurn);
    }

    let Some(hand_index) = player_state.hand.iter().position(|c| c.instance == card) else {
        return Err(ActionError::UnknownCard);
    };
    let card_ref = player_state.hand[hand_index];

    let Some(entity) = state.cards.get(card_ref.def) else {
        return Err(ActionError::UnknownCard);
    };
    if family(entity) != CardKind::Summon {
        return Err(ActionError::UnknownCard);
    }
    let Some(new_form) = current_form(entity) else {
        return Err(ActionError::UnknownCard);
    };

    let Some(top_entity) = state.cards.get(summon.chain.top().def) else {
        return Err(ActionError::UnknownCard);
    };
    let Some(top_form) = current_form(top_entity) else {
        return Err(ActionError::UnknownCard);
    };

    let new_types = produced_types(entity);
    let top_types = produced_types(top_entity);
    let types_carry_forward = top_types.iter().all(|t| new_types.contains(t));

    if new_form <= top_form || !types_carry_forward {
        return Err(ActionError::IllegalUpgradeTarget);
    }

    let mut new_layers: Vec<_> = summon.chain.layers().skip(1).copied().collect();
    new_layers.push(card_ref);
    let mut next_summon = summon.clone();
    next_summon.chain = UpgradeChain::new(summon.chain.base(), new_layers);
    next_summon.ready = false;
    next_summon.upgraded_this_turn = true;

    let mut next = state.clone();
    let next_player = next.players.get_mut(player);
    next_player.hand.remove(hand_index);
    match position {
        Position::Main => next_player.main = Some(next_summon),
        Position::Bench(slot) => next_player.bench[slot.index()] = Some(next_summon),
    }

    Ok(ActionOutcome {
        state: next,
        events: vec![GameEvent::SummonUpgraded {
            player,
            card,
            position,
        }],
    })
}

/// Perform the normal Retreat: pay the printed Retreat Cost and swap Main
/// with the named Bench slot (rules §26).
pub(crate) fn retreat(
    state: &GameState,
    player: PlayerId,
    slot: BenchSlot,
    mana_hint: Option<ManaType>,
) -> Result<ActionOutcome, ActionError> {
    require_free_main_phase(state)?;
    if state.turn.normal_retreat_used {
        return Err(ActionError::NormalRetreatAlreadyUsed);
    }

    let player_state = state.players.get(player);
    let Some(main_summon) = &player_state.main else {
        return Err(ActionError::EmptyPosition);
    };
    if player_state.bench[slot.index()].is_none() {
        return Err(ActionError::EmptyPosition);
    }

    let Some(top_entity) = state.cards.get(main_summon.chain.top().def) else {
        return Err(ActionError::UnknownCard);
    };
    let Some(printed_cost) = retreat_cost(top_entity) else {
        return Err(ActionError::UnknownCard);
    };

    let delta = opposing_retreat_cost_delta(state, player);
    let adjusted_cost = printed_cost.saturating_add_signed(delta);
    let cost = Cost {
        matter: 0,
        mind: 0,
        spirit: 0,
        generic: adjusted_cost,
    };

    let payment = match payment::deduct(player_state.mana, cost, mana_hint) {
        Ok(payment) => payment,
        Err(PaymentError::Insufficient(short)) => {
            return Err(ActionError::InsufficientMana { short });
        }
        Err(PaymentError::InvalidHint) => return Err(ActionError::InvalidManaHint),
    };

    let mut next = state.clone();
    next.turn.normal_retreat_used = true;
    let next_player = next.players.get_mut(player);
    next_player.mana = payment.bank;

    let Some(vacating_main) = next_player.main.take() else {
        return Err(ActionError::EmptyPosition);
    };
    let Some(incoming_main) = next_player.bench[slot.index()].take() else {
        return Err(ActionError::EmptyPosition);
    };
    next_player.main = Some(incoming_main);
    next_player.bench[slot.index()] = Some(vacating_main);

    // Rules §28: a Main/Bench exchange fires these four movement triggers
    // in a fixed order. The swap above already moved both Summons, so each
    // step names the position its Summon occupies now: the one that left
    // Main is at `Bench(slot)` for both `LeavingMain` and `EnteringBench`;
    // the one that left the Bench is at `Main` for both `LeavingBench` and
    // `EnteringMain`. `entered_main_this_turn` is not set here: draining the
    // queued `EnteringMain` step through `engine::triggers::movement_trigger`
    // sets it — the one place in this crate that does, since every path onto
    // Main enqueues that same step.
    next.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingMain,
        player,
        Position::Bench(slot),
    ));
    next.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringBench,
        player,
        Position::Bench(slot),
    ));
    next.work.push_back(WorkItem::MovementTrigger(
        MovementStep::LeavingBench,
        player,
        Position::Main,
    ));
    next.work.push_back(WorkItem::MovementTrigger(
        MovementStep::EnteringMain,
        player,
        Position::Main,
    ));

    let mut events: Vec<GameEvent> = payment
        .deductions
        .iter()
        .map(|deduction| GameEvent::ManaDeducted {
            player,
            mana_type: deduction.mana_type,
            amount: deduction.amount,
        })
        .collect();
    events.push(GameEvent::SummonsSwapped { player, main: slot });

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

#[cfg(test)]
mod tests;
