//! Destruction, Prize recovery, and Promotion (rules §23–25, §28, §41).
//!
//! `WorkItem::DestructionCheck` and `WorkItem::DiscardDestroyedChain` carry
//! only a `Position`, not a `PlayerId`, because the same position exists on
//! both players' boards. Both re-derive the acting player by checking which
//! board still holds an over-damaged Summon there, opponent of the active
//! player first (rules §41; see `ordered_players`). This stays unambiguous
//! because one player's whole destruction chain — discard through
//! `LossCheck` — is always enqueued as one contiguous block (`check`), so a
//! later step for the same position never has to choose between two players
//! still holding an over-damaged Summon there at once.

use crate::domain::cards::{Breakage, CardSet, ComponentKind, Life, TriggerEvent};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{BenchSlot, PlayerId, Position};
use crate::domain::state::{
    GameState, MovementStep, PendingInput, PlayerState, SummonInstance, WorkItem,
};
use crate::engine::apply::ActionOutcome;
use crate::engine::triggers;

// ---------------------------------------------------------------------------
// Shared reads
// ---------------------------------------------------------------------------

/// The Summon at `position`, if any.
fn summon_at(player_state: &PlayerState, position: Position) -> Option<&SummonInstance> {
    match position {
        Position::Main => player_state.main.as_ref(),
        Position::Bench(slot) => player_state.bench[slot.index()].as_ref(),
    }
}

/// Remove and return the Summon at `position`, if any.
fn take_at(player_state: &mut PlayerState, position: Position) -> Option<SummonInstance> {
    match position {
        Position::Main => player_state.main.take(),
        Position::Bench(slot) => player_state.bench[slot.index()].take(),
    }
}

/// The printed Life on `summon`'s current, topmost card. Every battlefield
/// card is expected to print its own Life (rules §23); an entity absent from
/// the `CardSet` entirely, and one present but missing the `Life` component,
/// both break the game the same way — `demand` cannot tell them apart, since
/// a card that isn't there is a card that fails to answer for `Life` no
/// differently than one that answers everything else and stays silent about
/// this one fact.
fn life_of(cards: &CardSet, summon: &SummonInstance) -> Result<u32, Breakage> {
    let def_id = summon.chain.top().def;
    let entity = cards.get(def_id).ok_or(Breakage {
        rule: "destruction",
        entity: def_id,
        expected: ComponentKind::Life,
    })?;
    Ok(entity.demand::<Life>("destruction")?.0)
}

/// Whether `player`'s Summon at `position` has accumulated Damage at or past
/// its printed Life (rules §23). `Ok(false)` for an empty position — nothing
/// to check; `Err` when the occupying card's printed Life could not be read.
fn is_destroyed(state: &GameState, player: PlayerId, position: Position) -> Result<bool, Breakage> {
    let Some(summon) = summon_at(state.players.get(player), position) else {
        return Ok(false);
    };
    let life = life_of(&state.cards, summon)?;
    Ok(summon.damage >= life)
}

/// The active player's opponent, then the active player (rules §41): when
/// one event destroys Summons for both players at once, the opponent of the
/// active player resolves their whole destruction first. `pub(crate)` so
/// `engine::triggers` can order simultaneous trigger candidates the same
/// way.
pub(crate) fn ordered_players(state: &GameState) -> [PlayerId; 2] {
    let active = state.turn.active_player;
    [active.opponent(), active]
}

/// Whichever candidate still has an over-damaged Summon at `position`,
/// opponent of the active player first. Safe to call at any
/// `DiscardDestroyedChain` dequeue — see the module doc comment.
fn destroyed_controller(
    state: &GameState,
    position: Position,
) -> Result<Option<PlayerId>, Breakage> {
    for player in ordered_players(state) {
        if is_destroyed(state, player, position)? {
            return Ok(Some(player));
        }
    }
    Ok(None)
}

/// `player`'s occupied Bench slots, in index order. `pub(crate)` so
/// `engine::triggers` can build the same Main-then-Bench ordering (rules
/// §41) for trigger discovery.
pub(crate) fn occupied_bench_slots(state: &GameState, player: PlayerId) -> Vec<BenchSlot> {
    let bench = &state.players.get(player).bench;
    BenchSlot::ALL
        .into_iter()
        .filter(|slot| bench[slot.index()].is_some())
        .collect()
}

// ---------------------------------------------------------------------------
// WorkItem executors
// ---------------------------------------------------------------------------

/// `WorkItem::DestructionCheck` (rules §23): for each player with an
/// over-damaged Summon at `position`, opponent of the active player first
/// (rules §41), queue that player's destruction procedure. A miss, or a hit
/// that stays below Life, leaves the queue untouched. A card occupying
/// `position` that cannot answer for its own printed Life breaks the game
/// instead — nothing already queued is lost; see `GameState::break_game`.
pub(crate) fn check(state: &GameState, position: Position) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    for player in ordered_players(&state) {
        match is_destroyed(&state, player, position) {
            Ok(true) => enqueue_destruction(&mut state, player, position),
            Ok(false) => {}
            Err(breakage) => return (state.break_game(breakage), Vec::new()),
        }
    }
    (state, Vec::new())
}

/// Queue the fixed step order for one player's destruction at `position`
/// (rules §24). A Main destruction runs all six steps; a Bench destruction
/// only discards and checks for a loss (rules §23: Bench Summons carry no
/// Main loss, Prize, or Promotion).
fn enqueue_destruction(state: &mut GameState, player: PlayerId, position: Position) {
    state
        .work
        .push_back(WorkItem::DiscardDestroyedChain(position));
    if position == Position::Main {
        state.work.push_back(WorkItem::RecordMainLoss(player));
        state.work.push_back(WorkItem::RecoverPrize(player));
        state.work.push_back(WorkItem::PromoteBenchSummon(player));
        state
            .work
            .push_back(WorkItem::ResolveMovementConsequences(player));
    }
    state.work.push_back(WorkItem::LossCheck(player));
}

/// `WorkItem::DiscardDestroyedChain` (rules §24 step 1): move the destroyed
/// Summon's whole upgrade chain to its owner's discard pile. Owner, not
/// controller — rules §5: a card always leaves play to its owner's zone. A
/// card occupying `position` that cannot answer for its own printed Life
/// breaks the game instead of continuing the chain.
pub(crate) fn discard_destroyed_chain(
    state: &GameState,
    position: Position,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let controller = match destroyed_controller(&state, position) {
        Ok(controller) => controller,
        Err(breakage) => return (state.break_game(breakage), Vec::new()),
    };
    let Some(controller) = controller else {
        return (state, Vec::new());
    };
    let Some(summon) = take_at(state.players.get_mut(controller), position) else {
        return (state, Vec::new());
    };
    let owner = summon.owner;
    state
        .players
        .get_mut(owner)
        .discard
        .extend(summon.chain.layers().copied());

    // Rules §36–38, §41: any Summon still in play may carry an
    // `AnySummonDestroyed` trigger; queue every match, opponent of the
    // active player first, ahead of the rest of this destruction chain
    // (`enqueue_destruction` already queued the steps that follow this one
    // as one contiguous block — see the module doc comment — so pushing to
    // the front here still keeps that whole block contiguous, just behind
    // these new items instead of immediately next).
    state = triggers::discover_front(
        &state,
        &ordered_players(&state),
        TriggerEvent::AnySummonDestroyed,
    );

    (state, vec![GameEvent::SummonDestroyed { position, owner }])
}

/// `WorkItem::RecordMainLoss` (rules §24 step 2, §2).
pub(crate) fn record_main_loss(state: &GameState, player: PlayerId) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    state.players.get_mut(player).main_losses += 1;
    (state, Vec::new())
}

/// `WorkItem::RecoverPrize` (rules §24 step 3, §25): pause for the
/// opponent's choice when a Prize remains, otherwise a documented no-op —
/// a second or third Main loss can find the Prize pool already spent.
pub(crate) fn recover_prize(state: &GameState, player: PlayerId) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    if state.players.get(player).prizes.is_empty() {
        return (state, Vec::new());
    }
    state.pending = Some(PendingInput::PrizePick {
        chooser: player.opponent(),
    });
    (state, Vec::new())
}

/// `ChoosePrize`: answer a paused `PrizePick`. The recovering player is
/// derived as `player.opponent()` — the chooser named on `PendingInput` is
/// always the opponent of the player recovering the Prize (rules §25), so
/// that relationship, not a second stored id, is what names them.
pub(crate) fn answer_prize(
    state: &GameState,
    player: PlayerId,
    prize_index: usize,
) -> Result<ActionOutcome, ActionError> {
    match state.pending {
        Some(PendingInput::PrizePick { chooser }) if chooser == player => {}
        _ => return Err(ActionError::PendingInputMismatch),
    }

    let recovering = player.opponent();
    let mut state = state.clone();
    let recovering_state = state.players.get_mut(recovering);
    if prize_index >= recovering_state.prizes.len() {
        return Err(ActionError::InvalidTarget);
    }
    let card = recovering_state.prizes.remove(prize_index);
    recovering_state.hand.push(card);
    state.pending = None;

    Ok(ActionOutcome {
        state,
        events: vec![GameEvent::PrizeRecovered {
            player: recovering,
            card: card.instance,
        }],
    })
}

/// `WorkItem::PromoteBenchSummon` (rules §24 step 4): automatic with exactly
/// one Benched Summon, paused for a choice with several, and a documented
/// no-op with none — that last shape is caught by the `LossCheck` right
/// after it (`LossReason::NoPromotionAvailable`).
pub(crate) fn promote_bench_summon(
    state: &GameState,
    player: PlayerId,
) -> (GameState, Vec<GameEvent>) {
    match occupied_bench_slots(state, player).as_slice() {
        [] => (state.clone(), Vec::new()),
        [slot] => promote_from_slot(state, player, *slot),
        _ => {
            let mut state = state.clone();
            state.pending = Some(PendingInput::Promotion { player });
            (state, Vec::new())
        }
    }
}

/// `ChoosePromotion`: answer a paused `Promotion` decision.
pub(crate) fn answer_promotion(
    state: &GameState,
    player: PlayerId,
    slot: BenchSlot,
) -> Result<ActionOutcome, ActionError> {
    match state.pending {
        Some(PendingInput::Promotion {
            player: pending_player,
        }) if pending_player == player => {}
        _ => return Err(ActionError::PendingInputMismatch),
    }
    if state.players.get(player).bench[slot.index()].is_none() {
        return Err(ActionError::EmptyPosition);
    }

    let (mut state, events) = promote_from_slot(state, player, slot);
    state.pending = None;
    Ok(ActionOutcome { state, events })
}

/// Move the Bench Summon at `slot` to the empty Main (rules §24 step 4).
/// Ready carries over unchanged — unlike a played or upgraded Summon (rules
/// §17, §19), Promotion is a movement, not a new arrival, so nothing here
/// forces it to Exhausted. The Main-entry record is left alone: the
/// caller's `resolve_movement_consequences` (step 5, right after this one)
/// always queues an `EnteringMain` step for the same Summon, and draining
/// that through `engine::triggers::movement_trigger` sets the flag — the one
/// place in this crate that does.
fn promote_from_slot(
    state: &GameState,
    player: PlayerId,
    slot: BenchSlot,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let player_state = state.players.get_mut(player);
    let Some(summon) = player_state.bench[slot.index()].take() else {
        return (state, Vec::new());
    };
    player_state.main = Some(summon);

    (
        state,
        vec![GameEvent::SummonPromoted { player, from: slot }],
    )
}

/// `WorkItem::ResolveMovementConsequences` (rules §24 step 5): once a
/// Promotion has actually landed — automatic or answered — queue the two
/// movement-trigger steps a one-way Bench-to-Main move fires. Rules §28
/// lists all four steps for a full Main/Bench exchange; nothing vacates
/// Main here, so only `LeavingBench` and `EnteringMain` apply. The vacated
/// slot is already gone by the time this runs, so both steps are recorded
/// against `Position::Main` rather than the Bench slot they left — an
/// honest simplification, harmless today because the promoted Summon is the
/// only one either step could name. A Promotion that found no Bench Summon
/// leaves Main empty here too, so nothing is queued.
pub(crate) fn resolve_movement_consequences(
    state: &GameState,
    player: PlayerId,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    if state.players.get(player).main.is_some() {
        // Pushed to the front, not the back: `LossCheck` for this same
        // player is already queued immediately after this step (rules §24
        // step 6 comes after step 5), and a plain `push_back` would land
        // these two behind it instead of ahead of it.
        state.work.push_front(WorkItem::MovementTrigger(
            MovementStep::EnteringMain,
            player,
            Position::Main,
        ));
        state.work.push_front(WorkItem::MovementTrigger(
            MovementStep::LeavingBench,
            player,
            Position::Main,
        ));
    }
    (state, Vec::new())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests;
