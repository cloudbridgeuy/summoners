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

use crate::domain::cards::{Query, QueryResult, TriggerEvent, find_def};
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

/// The printed Life on `summon`'s current, topmost card, or none for a card
/// with no `Life` node.
fn life_of(summon: &SummonInstance) -> Option<u32> {
    match find_def(summon.chain.top().def)?.find(Query::Life) {
        Some(QueryResult::Life(life)) => Some(life),
        _ => None,
    }
}

/// Whether `player`'s Summon at `position` has accumulated Damage at or past
/// its printed Life (rules §23).
fn is_destroyed(state: &GameState, player: PlayerId, position: Position) -> bool {
    let Some(summon) = summon_at(state.players.get(player), position) else {
        return false;
    };
    let Some(life) = life_of(summon) else {
        return false;
    };
    summon.damage >= life
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
fn destroyed_controller(state: &GameState, position: Position) -> Option<PlayerId> {
    ordered_players(state)
        .into_iter()
        .find(|player| is_destroyed(state, *player, position))
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
/// that stays below Life, leaves the queue untouched.
pub(crate) fn check(state: &GameState, position: Position) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    for player in ordered_players(&state) {
        if is_destroyed(&state, player, position) {
            enqueue_destruction(&mut state, player, position);
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
/// controller — rules §5: a card always leaves play to its owner's zone.
pub(crate) fn discard_destroyed_chain(
    state: &GameState,
    position: Position,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let Some(controller) = destroyed_controller(&state, position) else {
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
/// forces it to Exhausted. `entered_main_this_turn` is left alone: the
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
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::CardInstanceId;
    use crate::domain::state::{
        CardRef, GameOutcome, LossReason, ManaBank, PerPlayer, Phase, TurnState, UpgradeChain,
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
            enchantments: vec![],
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

    fn lethal(owner: PlayerId) -> SummonInstance {
        SummonInstance {
            damage: 40, // Quarry Whelp's printed Life.
            ..summon(owner)
        }
    }

    // -- check ---------------------------------------------------------

    #[test]
    fn check_is_a_no_op_for_a_non_lethal_hit() {
        let state = base_state();

        let (next_state, events) = check(&state, Position::Main);

        assert_eq!(next_state, state);
        assert!(events.is_empty());
    }

    #[test]
    fn check_enqueues_the_six_step_main_chain_for_an_over_damaged_main() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));

        let (state, events) = check(&state, Position::Main);

        assert!(events.is_empty());
        assert_eq!(
            state.work,
            VecDeque::from(vec![
                WorkItem::DiscardDestroyedChain(Position::Main),
                WorkItem::RecordMainLoss(PlayerId::Two),
                WorkItem::RecoverPrize(PlayerId::Two),
                WorkItem::PromoteBenchSummon(PlayerId::Two),
                WorkItem::ResolveMovementConsequences(PlayerId::Two),
                WorkItem::LossCheck(PlayerId::Two),
            ])
        );
    }

    #[test]
    fn check_enqueues_only_discard_and_loss_check_for_an_over_damaged_bench_summon() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).bench[0] = Some(lethal(PlayerId::Two));

        let (state, _events) = check(&state, Position::Bench(BenchSlot::First));

        assert_eq!(
            state.work,
            VecDeque::from(vec![
                WorkItem::DiscardDestroyedChain(Position::Bench(BenchSlot::First)),
                WorkItem::LossCheck(PlayerId::Two),
            ])
        );
    }

    #[test]
    fn check_orders_the_opponent_of_the_active_player_first_when_both_are_destroyed() {
        // Rules §41: one event destroying both players' Summons at once
        // resolves the active player's opponent's chain first.
        let mut state = base_state();
        state.turn.active_player = PlayerId::One;
        state.players.get_mut(PlayerId::One).main = Some(lethal(PlayerId::One));
        state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));

        let (state, _events) = check(&state, Position::Main);

        let players_in_order: Vec<PlayerId> = state
            .work
            .iter()
            .filter_map(|item| match item {
                WorkItem::RecordMainLoss(player) => Some(*player),
                _ => None,
            })
            .collect();
        assert_eq!(players_in_order, vec![PlayerId::Two, PlayerId::One]);
    }

    // -- discard_destroyed_chain -----------------------------------------

    #[test]
    fn discard_destroyed_chain_moves_the_chain_to_the_owners_discard() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));

        let (state, events) = discard_destroyed_chain(&state, Position::Main);

        assert_eq!(
            events,
            vec![GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            }]
        );
        assert_eq!(state.players.get(PlayerId::Two).main, None);
        assert_eq!(
            state.players.get(PlayerId::Two).discard,
            vec![CardRef {
                instance: CardInstanceId(1),
                def: CardDefId("quarry-whelp"),
            }]
        );
    }

    #[test]
    fn discard_destroyed_chain_is_a_no_op_when_nothing_here_is_over_damaged() {
        let state = base_state();

        let (next_state, events) = discard_destroyed_chain(&state, Position::Main);

        assert_eq!(next_state, state);
        assert!(events.is_empty());
    }

    #[test]
    fn discard_destroyed_chain_queues_a_matching_any_summon_destroyed_trigger() {
        // Rules §37, §41: Spite Thorn's respondable trigger fires whenever
        // any Summon is destroyed, including its own controller's own
        // Bench Summon being destroyed elsewhere.
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(lethal(PlayerId::Two));
        state.players.get_mut(PlayerId::One).bench[0] = Some(SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(9),
                    def: CardDefId("spite-thorn"),
                },
                vec![],
            ),
            ..summon(PlayerId::One)
        });

        let (state, events) = discard_destroyed_chain(&state, Position::Main);

        assert_eq!(
            events,
            vec![GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            }]
        );
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::FireTrigger(
                PlayerId::One,
                Position::Bench(BenchSlot::First),
                TriggerEvent::AnySummonDestroyed,
            )])
        );
    }

    // -- record_main_loss --------------------------------------------------

    #[test]
    fn record_main_loss_increments_the_losers_count() {
        let state = base_state();

        let (state, events) = record_main_loss(&state, PlayerId::Two);

        assert_eq!(state.players.get(PlayerId::Two).main_losses, 1);
        assert!(events.is_empty());
    }

    // -- recover_prize / answer_prize ---------------------------------------

    fn prize(instance: u32) -> CardRef {
        CardRef {
            instance: CardInstanceId(instance),
            def: CardDefId("quarry-whelp"),
        }
    }

    #[test]
    fn recover_prize_pauses_for_the_opponent_to_choose() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).prizes = vec![prize(9)];

        let (state, events) = recover_prize(&state, PlayerId::Two);

        assert_eq!(
            state.pending,
            Some(PendingInput::PrizePick {
                chooser: PlayerId::One
            })
        );
        assert!(events.is_empty());
    }

    #[test]
    fn recover_prize_is_a_no_op_with_no_prizes_left() {
        let state = base_state();

        let (state, events) = recover_prize(&state, PlayerId::Two);

        assert_eq!(state.pending, None);
        assert!(events.is_empty());
    }

    #[test]
    fn answer_prize_moves_the_chosen_prize_to_the_recovering_players_hand() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).prizes = vec![prize(9), prize(10)];
        state.pending = Some(PendingInput::PrizePick {
            chooser: PlayerId::One,
        });

        let outcome = answer_prize(&state, PlayerId::One, 0).expect("a prize is pending");

        assert_eq!(outcome.state.pending, None);
        assert_eq!(
            outcome.state.players.get(PlayerId::Two).prizes,
            vec![prize(10)]
        );
        assert_eq!(
            outcome.state.players.get(PlayerId::Two).hand,
            vec![prize(9)]
        );
        assert_eq!(
            outcome.events,
            vec![GameEvent::PrizeRecovered {
                player: PlayerId::Two,
                card: CardInstanceId(9),
            }]
        );
    }

    #[test]
    fn answer_prize_rejects_a_mismatched_pending() {
        let state = base_state();

        assert_eq!(
            answer_prize(&state, PlayerId::One, 0),
            Err(ActionError::PendingInputMismatch)
        );
    }

    #[test]
    fn answer_prize_rejects_the_wrong_chooser() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).prizes = vec![prize(9)];
        state.pending = Some(PendingInput::PrizePick {
            chooser: PlayerId::One,
        });

        assert_eq!(
            answer_prize(&state, PlayerId::Two, 0),
            Err(ActionError::PendingInputMismatch)
        );
    }

    #[test]
    fn answer_prize_rejects_an_out_of_range_index() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).prizes = vec![prize(9)];
        state.pending = Some(PendingInput::PrizePick {
            chooser: PlayerId::One,
        });

        assert_eq!(
            answer_prize(&state, PlayerId::One, 1),
            Err(ActionError::InvalidTarget)
        );
    }

    // -- promote_bench_summon / answer_promotion -----------------------------

    #[test]
    fn promote_bench_summon_is_automatic_with_exactly_one_bench_summon() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = None;
        state.players.get_mut(PlayerId::Two).bench[1] = Some(summon(PlayerId::Two));

        let (state, events) = promote_bench_summon(&state, PlayerId::Two);

        assert_eq!(state.pending, None);
        assert!(state.players.get(PlayerId::Two).main.is_some());
        assert_eq!(state.players.get(PlayerId::Two).bench[1], None);
        assert_eq!(
            events,
            vec![GameEvent::SummonPromoted {
                player: PlayerId::Two,
                from: BenchSlot::Second,
            }]
        );
    }

    #[test]
    fn promote_bench_summon_pauses_with_several_bench_summons() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = None;
        state.players.get_mut(PlayerId::Two).bench = [
            Some(summon(PlayerId::Two)),
            Some(summon(PlayerId::Two)),
            None,
        ];

        let (state, events) = promote_bench_summon(&state, PlayerId::Two);

        assert_eq!(
            state.pending,
            Some(PendingInput::Promotion {
                player: PlayerId::Two
            })
        );
        assert!(events.is_empty());
    }

    #[test]
    fn promote_bench_summon_is_a_no_op_with_an_empty_bench() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = None;

        let (state, events) = promote_bench_summon(&state, PlayerId::Two);

        assert_eq!(state.pending, None);
        assert_eq!(state.players.get(PlayerId::Two).main, None);
        assert!(events.is_empty());
    }

    #[test]
    fn answer_promotion_moves_the_chosen_slot_to_main_and_preserves_ready() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = None;
        let mut resting = summon(PlayerId::Two);
        resting.ready = false;
        state.players.get_mut(PlayerId::Two).bench = [None, Some(resting), None];
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::Two,
        });
        // Mirrors what `enqueue_destruction` already left queued, right
        // behind the paused `PromoteBenchSummon` step, before the multi-slot
        // Bench forced this pause (rules §24 step 5 follows step 4).
        state
            .work
            .push_back(WorkItem::ResolveMovementConsequences(PlayerId::Two));

        let outcome =
            answer_promotion(&state, PlayerId::Two, BenchSlot::Second).expect("a slot is filled");

        assert_eq!(outcome.state.pending, None);
        let promoted = outcome
            .state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("promotion filled Main");
        assert!(
            !promoted.ready,
            "Promotion is a movement, not a new arrival"
        );
        assert_eq!(outcome.state.players.get(PlayerId::Two).bench[1], None);
        assert_eq!(
            outcome.events,
            vec![GameEvent::SummonPromoted {
                player: PlayerId::Two,
                from: BenchSlot::Second,
            }]
        );

        // `answer_promotion` itself only moves the Summon (rules §24 step
        // 4); `entered_main_this_turn` is set later, when the queued
        // `ResolveMovementConsequences` step (already sitting in `work`
        // above, exactly as it would be mid-destruction-chain) enqueues an
        // `EnteringMain` trigger and draining runs it through
        // `engine::triggers::movement_trigger`.
        let (drained, _) = crate::engine::resolution::drain(&outcome.state);
        let drained_promoted = drained
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("promotion still filled Main after drain");
        assert!(drained_promoted.entered_main_this_turn);
    }

    #[test]
    fn answer_promotion_rejects_a_mismatched_pending() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).bench[0] = Some(summon(PlayerId::Two));

        assert_eq!(
            answer_promotion(&state, PlayerId::Two, BenchSlot::First),
            Err(ActionError::PendingInputMismatch)
        );
    }

    #[test]
    fn answer_promotion_rejects_an_empty_slot() {
        let mut state = base_state();
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::Two,
        });

        assert_eq!(
            answer_promotion(&state, PlayerId::Two, BenchSlot::First),
            Err(ActionError::EmptyPosition)
        );
    }

    // -- resolve_movement_consequences ---------------------------------------

    #[test]
    fn resolve_movement_consequences_queues_the_two_steps_ahead_of_a_pending_loss_check() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)]);

        let (state, events) = resolve_movement_consequences(&state, PlayerId::Two);

        assert!(events.is_empty());
        assert_eq!(
            state.work,
            VecDeque::from(vec![
                WorkItem::MovementTrigger(
                    MovementStep::LeavingBench,
                    PlayerId::Two,
                    Position::Main
                ),
                WorkItem::MovementTrigger(
                    MovementStep::EnteringMain,
                    PlayerId::Two,
                    Position::Main
                ),
                WorkItem::LossCheck(PlayerId::Two),
            ])
        );
    }

    #[test]
    fn resolve_movement_consequences_is_a_no_op_when_no_promotion_landed() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = None;

        let (state, events) = resolve_movement_consequences(&state, PlayerId::Two);

        assert!(state.work.is_empty());
        assert!(events.is_empty());
    }

    // -- freezing on a third Main loss mid-resolution -------------------------

    #[test]
    fn a_third_main_loss_freezes_the_rest_of_the_chain() {
        // Rules §2: the third Main loss ends the game outright, even with a
        // Promotion still queued behind `LossCheck`.
        let mut state = base_state();
        let two = state.players.get_mut(PlayerId::Two);
        two.main_losses = 2;
        two.main = Some(lethal(PlayerId::Two));
        two.bench[0] = Some(summon(PlayerId::Two));

        let (state, _events) = check(&state, Position::Main);
        let (state, _events) = discard_destroyed_chain(&state, Position::Main);
        let (state, _events) = record_main_loss(&state, PlayerId::Two);
        let (state, _events) = recover_prize(&state, PlayerId::Two);
        let (state, _events) = promote_bench_summon(&state, PlayerId::Two);
        let (state, _events) = crate::engine::loss::check(&state, PlayerId::Two);

        assert_eq!(
            state.outcome,
            Some(GameOutcome {
                winner: PlayerId::One,
                reason: LossReason::ThirdMainLoss,
            })
        );
        // The `ResolveMovementConsequences` step queued by `check` is still
        // sitting untouched: the resolution loop's own pending/outcome
        // check (not tested here — see `resolution::drain`) is what stops
        // it from ever running once `outcome` is set.
        assert!(
            state
                .work
                .contains(&WorkItem::ResolveMovementConsequences(PlayerId::Two))
        );
    }
}
