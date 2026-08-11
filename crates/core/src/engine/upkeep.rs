//! Upkeep work — Ready, draw, and natural Mana production (rules §10–12) —
//! plus the turn handoff, the Coin, and the anchoring rule they share
//! (rules §7, §11–12; design decisions 12–13).
//!
//! The anchoring rule: a Mana Type is available to a player only if some
//! Summon they control in Main or on the Bench currently prints it (read
//! through the card tree, never off a cached value, since the topmost card
//! of a chain can change between Upkeeps). Natural production and the Coin
//! both anchor to the union of every controlled Summon's printed Types;
//! decision 12 makes that union a pause point only when it holds more than
//! one Type.

use crate::domain::cards::{Query, QueryResult, find_def};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{BenchSlot, ManaType, PlayerId, Position};
use crate::domain::state::{
    GameState, ManaSource, PendingInput, Phase, PlayerState, SummonInstance, WorkItem,
};
use crate::engine::apply::ActionOutcome;

// ---------------------------------------------------------------------------
// The anchoring rule
// ---------------------------------------------------------------------------

/// Every Summon `player_state` controls, Main first then Bench in slot
/// order — a fixed, deterministic order shared with `ready_all` and
/// `scenario::positioned_summons`.
fn controlled_summons(player_state: &PlayerState) -> Vec<&SummonInstance> {
    let mut summons = Vec::new();
    if let Some(summon) = &player_state.main {
        summons.push(summon);
    }
    for summon in player_state.bench.iter().flatten() {
        summons.push(summon);
    }
    summons
}

/// Every position `player_state` occupies, in the same fixed order.
fn controlled_positions(player_state: &PlayerState) -> Vec<Position> {
    let mut positions = Vec::new();
    if player_state.main.is_some() {
        positions.push(Position::Main);
    }
    for slot in BenchSlot::ALL {
        if player_state.bench[slot.index()].is_some() {
            positions.push(Position::Bench(slot));
        }
    }
    positions
}

/// The Mana Types printed on one Summon's current (topmost) card, read
/// through the card tree. An unknown definition prints no Types — parsing
/// already guarantees every card on the board resolves, so this only
/// happens if a caller builds a `GameState` by hand with a bad reference;
/// treating it as "produces nothing" is the honest fallback for a pure
/// function that cannot error.
fn produced_types(summon: &SummonInstance) -> Vec<ManaType> {
    match find_def(summon.chain.top().def).and_then(|def| def.find(Query::ProducedManaTypes)) {
        Some(QueryResult::ProducedManaTypes(types)) => types,
        _ => Vec::new(),
    }
}

fn mana_type_index(mana_type: ManaType) -> usize {
    match mana_type {
        ManaType::Matter => 0,
        ManaType::Mind => 1,
        ManaType::Spirit => 2,
    }
}

/// Every Mana Type available to `player_state` through the Summons it
/// controls in Main or on the Bench (rules §11–12, decision 13's anchoring
/// rule). Deduplicated and returned in the fixed Matter, Mind, Spirit
/// order, so the result is deterministic regardless of board layout.
pub(crate) fn anchor_types(player_state: &PlayerState) -> Vec<ManaType> {
    let mut seen = [false; 3];
    for summon in controlled_summons(player_state) {
        for mana_type in produced_types(summon) {
            seen[mana_type_index(mana_type)] = true;
        }
    }
    [ManaType::Matter, ManaType::Mind, ManaType::Spirit]
        .into_iter()
        .filter(|mana_type| seen[mana_type_index(*mana_type)])
        .collect()
}

/// The Mana Types printed on the one Summon at `position`, or none if that
/// position is empty. Used when a `WorkItem::ProduceMana(ManaSource::Summon(_))`
/// names one Summon's own production rather than the player's natural
/// production — no caller enqueues that path yet, so this exists for the
/// type to have a total, non-panicking implementation.
fn summon_types(player_state: &PlayerState, position: Position) -> Vec<ManaType> {
    let summon = match position {
        Position::Main => player_state.main.as_ref(),
        Position::Bench(slot) => player_state.bench[slot.index()].as_ref(),
    };
    summon.map(produced_types).unwrap_or_default()
}

/// The Mana Types available for one `ManaSource`, dispatching to the
/// player-wide anchor or one Summon's own printed Types.
fn available_types(player_state: &PlayerState, source: ManaSource) -> Vec<ManaType> {
    match source {
        ManaSource::Player => anchor_types(player_state),
        ManaSource::Summon(position) => summon_types(player_state, position),
    }
}

fn bank(player_state: &mut PlayerState, mana_type: ManaType) {
    match mana_type {
        ManaType::Matter => player_state.mana.matter += 1,
        ManaType::Mind => player_state.mana.mind += 1,
        ManaType::Spirit => player_state.mana.spirit += 1,
    }
}

// ---------------------------------------------------------------------------
// Per-turn Summon flag reset
// ---------------------------------------------------------------------------

/// Clear the per-turn flags on every Summon in `player_state`, Main and
/// Bench alike: `played_this_turn` (rules §17, §52 — a Base Summon cannot
/// be upgraded during the turn it was played), `upgraded_this_turn` (rules
/// §18, §52 — a Summon may be upgraded only once per turn), and
/// `entered_main_this_turn` (read by an opposing attacker's conditional
/// bonus during that attacker's own turn; no fixture carries the effect
/// that reads it yet, so the design document does not pin down its exact
/// clearing point — this is a deferred decision, resolved below).
///
/// All three share one reset point: the moment a turn changes hands, for
/// both players' boards (see `end_turn`). The rules describe each flag
/// against "this turn" as a single game-wide concept (§9: "players
/// alternate complete turns"), not a clock that runs only while a Summon's
/// controller happens to be active. That reading is provably correct for
/// `played_this_turn` and `upgraded_this_turn`: both are read only during
/// their controller's own Main Phase, which cannot arrive before their own
/// next Upkeep, so clearing them at the handoff that starts the *other*
/// player's turn already lands before that next read. It is also the only
/// reading available for `entered_main_this_turn`, which can be set on
/// either player's Summon and must stay true for the remainder of the turn
/// that set it, whether that Summon belongs to the player about to act or
/// not — clearing only the newly active player's own board would leave a
/// stale `true` sitting on the other player's board indefinitely.
fn reset_per_turn_summon_flags(player_state: &mut PlayerState) {
    if let Some(summon) = player_state.main.as_mut() {
        clear_per_turn_flags(summon);
    }
    for summon in player_state.bench.iter_mut().flatten() {
        clear_per_turn_flags(summon);
    }
}

fn clear_per_turn_flags(summon: &mut SummonInstance) {
    summon.played_this_turn = false;
    summon.upgraded_this_turn = false;
    summon.entered_main_this_turn = false;
}

// ---------------------------------------------------------------------------
// `WorkItem` executors
// ---------------------------------------------------------------------------

/// Rules §10 step 1: ready every Summon the active player controls.
pub(crate) fn ready_all(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let player = state.turn.active_player;
    let player_state = state.players.get_mut(player);
    let positions = controlled_positions(player_state);

    if let Some(summon) = player_state.main.as_mut() {
        summon.ready = true;
    }
    for summon in player_state.bench.iter_mut().flatten() {
        summon.ready = true;
    }

    (state, vec![GameEvent::SummonsReadied { player, positions }])
}

/// What `draw_card` found in the Deck.
pub(crate) enum DrawOutcome {
    Drew,
    DeckEmpty,
}

/// Rules §10 step 2: draw one card for the active player from the top of
/// their Deck (the front of the vector — ordered top to bottom). An empty
/// Deck produces no event and `DrawOutcome::DeckEmpty`; the caller decides
/// what that means (rules §2, §58: only a failed *required* draw is a
/// loss).
pub(crate) fn draw_card(state: &GameState) -> (GameState, Vec<GameEvent>, DrawOutcome) {
    let mut state = state.clone();
    let player = state.turn.active_player;
    let player_state = state.players.get_mut(player);

    if player_state.deck.is_empty() {
        return (state, Vec::new(), DrawOutcome::DeckEmpty);
    }

    let card = player_state.deck.remove(0);
    player_state.hand.push(card);

    (
        state,
        vec![GameEvent::CardDrawn {
            player,
            card: card.instance,
        }],
        DrawOutcome::Drew,
    )
}

/// Rules §10 step 3, §11–12, decision 12: generate Mana for the active
/// player from `source`. A single available Type produces without pausing;
/// several Types pause on `PendingInput::ManaProduction` for
/// `ChooseManaType` to answer; no available Type (an empty anchor) produces
/// nothing and is a documented no-op rather than an error, since a
/// `WorkItem` executor cannot reject.
pub(crate) fn produce_mana(state: &GameState, source: ManaSource) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let player = state.turn.active_player;
    let available = available_types(state.players.get(player), source);

    match available.as_slice() {
        [] => (state, Vec::new()),
        [single] => {
            let mana_type = *single;
            bank(state.players.get_mut(player), mana_type);
            (
                state,
                vec![GameEvent::ManaProduced {
                    player,
                    source,
                    mana_type,
                }],
            )
        }
        _ => {
            state.pending = Some(PendingInput::ManaProduction { player, source });
            (state, Vec::new())
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch handlers: `EndTurn`, `ConvertCoin`, `ChooseManaType`
// ---------------------------------------------------------------------------

/// `EndTurn` (decision 7, rules §9, §47–48): the direct turn handoff.
///
/// The rule §47 final Priority window — a last response opportunity before
/// Combat ends when the active player declines to attack — is not built
/// yet. Until it lands, `EndTurn` is only legal from a resting Main Phase
/// with no pending decision and no open Priority window; from any other
/// shape it is honestly rejected with `WrongPhase` rather than simulating a
/// window that does not exist yet. When legal, it hands the turn straight
/// to the opponent: every Summon's per-turn flags reset (see
/// `reset_per_turn_summon_flags`), the opponent's Upkeep begins, and
/// `Ready`, draw, and natural production are queued as work for the
/// resolution loop to drain.
pub(crate) fn end_turn(state: &GameState, player: PlayerId) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some()
        || state.turn.window.is_some()
        || state.turn.phase != Phase::Main
        || player != state.turn.active_player
    {
        return Err(ActionError::WrongPhase);
    }

    let mut state = state.clone();
    let opponent = player.opponent();
    state.turn = crate::domain::state::TurnState {
        active_player: opponent,
        phase: Phase::Upkeep,
        window: None,
        normal_attack_used: false,
        normal_retreat_used: false,
        spell_played_this_turn: false,
    };
    reset_per_turn_summon_flags(state.players.get_mut(player));
    reset_per_turn_summon_flags(state.players.get_mut(opponent));
    state.work.push_back(WorkItem::ReadyAll);
    state.work.push_back(WorkItem::DrawCard);
    state
        .work
        .push_back(WorkItem::ProduceMana(ManaSource::Player));

    Ok(ActionOutcome {
        state,
        events: vec![GameEvent::TurnBegan { player: opponent }],
    })
}

/// `ConvertCoin` (rules §7, decision 13): exchange the one-use Coin for one
/// anchored Mana.
///
/// Legal only in the owner's own Main Phase for now — the "or while the
/// owner holds Priority" clause in decision 13 arrives with the Priority
/// work, so an open window rejects this today rather than granting a
/// legality nothing yet polices. The chosen Type must be in the
/// owner's anchor, the same rule `produce_mana` uses; a missing Coin or an
/// out-of-anchor Type are both reported as `InvalidTarget`, since neither
/// names a legal target for this conversion.
pub(crate) fn convert_coin(
    state: &GameState,
    player: PlayerId,
    mana_type: ManaType,
) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some()
        || state.turn.window.is_some()
        || state.turn.phase != Phase::Main
        || player != state.turn.active_player
    {
        return Err(ActionError::WrongPhase);
    }

    let mut state = state.clone();
    let player_state = state.players.get_mut(player);

    if !player_state.has_coin {
        return Err(ActionError::InvalidTarget);
    }
    if !anchor_types(player_state).contains(&mana_type) {
        return Err(ActionError::InvalidTarget);
    }

    player_state.has_coin = false;
    bank(player_state, mana_type);

    Ok(ActionOutcome {
        state,
        events: vec![GameEvent::CoinConverted { player, mana_type }],
    })
}

/// `ChooseManaType` (rules §11–12): answer a paused `ManaProduction`
/// decision. Anything other than a matching `PendingInput::ManaProduction`
/// is `PendingInputMismatch`; a Type outside the anchor is `InvalidTarget`.
pub(crate) fn choose_mana_type(
    state: &GameState,
    player: PlayerId,
    mana_type: ManaType,
) -> Result<ActionOutcome, ActionError> {
    let Some(PendingInput::ManaProduction {
        player: pending_player,
        source,
    }) = state.pending
    else {
        return Err(ActionError::PendingInputMismatch);
    };
    if player != pending_player {
        return Err(ActionError::PendingInputMismatch);
    }

    let mut state = state.clone();
    let available = available_types(state.players.get(pending_player), source);
    if !available.contains(&mana_type) {
        return Err(ActionError::InvalidTarget);
    }

    state.pending = None;
    bank(state.players.get_mut(pending_player), mana_type);

    Ok(ActionOutcome {
        state,
        events: vec![GameEvent::ManaProduced {
            player: pending_player,
            source,
            mana_type,
        }],
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::CardInstanceId;
    use crate::domain::state::{
        CardRef, ManaBank, PerPlayer, StackWindow, TurnState, UpgradeChain,
    };
    use std::collections::VecDeque;

    fn chain_summon(owner: PlayerId, def: &'static str, instance: u32) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(instance),
                    def: CardDefId(def),
                },
                vec![],
            ),
            damage: 0,
            ready: false,
            owner,
            controller: owner,
            duration_markers: vec![],
            played_this_turn: false,
            upgraded_this_turn: false,
            entered_main_this_turn: false,
        }
    }

    fn whelp(owner: PlayerId) -> SummonInstance {
        chain_summon(owner, "quarry-whelp", 1)
    }

    fn adept(owner: PlayerId) -> SummonInstance {
        chain_summon(owner, "set-path-adept", 2)
    }

    fn brute(owner: PlayerId) -> SummonInstance {
        chain_summon(owner, "quarry-brute", 3)
    }

    fn empty_player_state() -> PlayerState {
        PlayerState {
            main: None,
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
        let one = PlayerState {
            main: Some(whelp(PlayerId::One)),
            ..empty_player_state()
        };
        let two = PlayerState {
            main: Some(whelp(PlayerId::Two)),
            ..empty_player_state()
        };
        GameState {
            players: PerPlayer::new(one, two),
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

    // -- anchor_types: mono, dual, multi-Summon, and empty boards --------

    #[test]
    fn anchor_types_reads_a_single_type_from_one_mono_type_summon() {
        let player_state = PlayerState {
            main: Some(whelp(PlayerId::One)),
            ..empty_player_state()
        };

        assert_eq!(anchor_types(&player_state), vec![ManaType::Matter]);
    }

    #[test]
    fn anchor_types_reads_both_types_from_one_dual_type_summon() {
        let player_state = PlayerState {
            main: Some(adept(PlayerId::One)),
            ..empty_player_state()
        };

        assert_eq!(
            anchor_types(&player_state),
            vec![ManaType::Matter, ManaType::Mind]
        );
    }

    #[test]
    fn anchor_types_unions_across_a_multi_summon_board_without_duplicates() {
        let mut player_state = PlayerState {
            main: Some(whelp(PlayerId::One)),
            ..empty_player_state()
        };
        player_state.bench[0] = Some(adept(PlayerId::One));
        player_state.bench[1] = Some(brute(PlayerId::One));

        // Whelp and Brute both print Matter only; Adept adds Mind. The
        // union still holds exactly two Types, proving the board-wide
        // anchor deduplicates rather than accumulating one entry per
        // Summon.
        assert_eq!(
            anchor_types(&player_state),
            vec![ManaType::Matter, ManaType::Mind]
        );
    }

    #[test]
    fn anchor_types_is_empty_for_a_board_with_no_summons() {
        assert_eq!(anchor_types(&empty_player_state()), Vec::<ManaType>::new());
    }

    // -- ready_all ---------------------------------------------------------

    #[test]
    fn ready_all_readies_every_controlled_position_main_first_then_bench_in_order() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).bench[1] = Some(adept(PlayerId::One));

        let (state, events) = ready_all(&state);

        let player_state = state.players.get(PlayerId::One);
        assert!(player_state.main.as_ref().expect("main set").ready);
        assert!(player_state.bench[1].as_ref().expect("bench set").ready);
        assert_eq!(
            events,
            vec![GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main, Position::Bench(BenchSlot::Second)],
            }]
        );
    }

    // -- draw_card -----------------------------------------------------------

    #[test]
    fn draw_card_moves_the_top_card_from_deck_to_hand() {
        let mut state = base_state();
        let top = CardRef {
            instance: CardInstanceId(50),
            def: CardDefId("quarry-whelp"),
        };
        state.players.get_mut(PlayerId::One).deck = vec![
            top,
            CardRef {
                instance: CardInstanceId(51),
                def: CardDefId("quarry-whelp"),
            },
        ];

        let (state, events, outcome) = draw_card(&state);

        assert!(matches!(outcome, DrawOutcome::Drew));
        let player_state = state.players.get(PlayerId::One);
        assert_eq!(player_state.hand, vec![top]);
        assert_eq!(player_state.deck.len(), 1);
        assert_eq!(
            events,
            vec![GameEvent::CardDrawn {
                player: PlayerId::One,
                card: top.instance,
            }]
        );
    }

    #[test]
    fn draw_card_from_an_empty_deck_produces_no_event() {
        let state = base_state();

        let (state, events, outcome) = draw_card(&state);

        assert!(matches!(outcome, DrawOutcome::DeckEmpty));
        assert!(events.is_empty());
        assert!(state.players.get(PlayerId::One).hand.is_empty());
    }

    // -- produce_mana: auto versus pause -------------------------------------

    #[test]
    fn produce_mana_auto_produces_when_exactly_one_type_is_anchored() {
        let state = base_state();

        let (state, events) = produce_mana(&state, ManaSource::Player);

        assert_eq!(state.pending, None);
        assert_eq!(state.players.get(PlayerId::One).mana.matter, 1);
        assert_eq!(
            events,
            vec![GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Player,
                mana_type: ManaType::Matter,
            }]
        );
    }

    #[test]
    fn produce_mana_pauses_when_several_types_are_anchored() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(adept(PlayerId::One));

        let (state, events) = produce_mana(&state, ManaSource::Player);

        assert_eq!(
            state.pending,
            Some(PendingInput::ManaProduction {
                player: PlayerId::One,
                source: ManaSource::Player,
            })
        );
        assert!(events.is_empty());
        assert_eq!(state.players.get(PlayerId::One).mana, ManaBank::default());
    }

    #[test]
    fn produce_mana_with_no_anchor_is_a_no_op() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = None;
        state.players.get_mut(PlayerId::One).bench[0] = None;

        let (state, events) = produce_mana(&state, ManaSource::Player);

        assert!(events.is_empty());
        assert_eq!(state.pending, None);
        assert_eq!(state.players.get(PlayerId::One).mana, ManaBank::default());
    }

    // -- end_turn -------------------------------------------------------------

    #[test]
    fn end_turn_hands_off_and_queues_the_opponents_upkeep() {
        let state = base_state();

        let outcome = end_turn(&state, PlayerId::One).expect("legal from Main");

        assert_eq!(
            outcome.events,
            vec![GameEvent::TurnBegan {
                player: PlayerId::Two
            }]
        );
        assert_eq!(outcome.state.turn.active_player, PlayerId::Two);
        assert_eq!(outcome.state.turn.phase, Phase::Upkeep);
        assert_eq!(
            outcome.state.work,
            VecDeque::from(vec![
                WorkItem::ReadyAll,
                WorkItem::DrawCard,
                WorkItem::ProduceMana(ManaSource::Player),
            ])
        );
    }

    #[test]
    fn end_turn_is_rejected_outside_a_resting_main_phase() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;

        assert_eq!(
            end_turn(&state, PlayerId::One),
            Err(ActionError::WrongPhase)
        );
    }

    #[test]
    fn end_turn_is_rejected_while_a_window_is_open() {
        let mut state = base_state();
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });

        assert_eq!(
            end_turn(&state, PlayerId::One),
            Err(ActionError::WrongPhase)
        );
    }

    #[test]
    fn end_turn_is_rejected_while_a_decision_is_pending() {
        let mut state = base_state();
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        assert_eq!(
            end_turn(&state, PlayerId::One),
            Err(ActionError::WrongPhase)
        );
    }

    // -- reset_per_turn_summon_flags -----------------------------------------

    #[test]
    fn reset_per_turn_summon_flags_clears_every_controlled_summon_main_and_bench() {
        let played = SummonInstance {
            played_this_turn: true,
            upgraded_this_turn: true,
            entered_main_this_turn: true,
            ..whelp(PlayerId::One)
        };
        let mut player_state = PlayerState {
            main: Some(played.clone()),
            ..empty_player_state()
        };
        player_state.bench[0] = Some(SummonInstance {
            controller: PlayerId::One,
            ..played
        });

        reset_per_turn_summon_flags(&mut player_state);

        for summon in [
            player_state.main.as_ref().expect("main set"),
            player_state.bench[0].as_ref().expect("bench set"),
        ] {
            assert!(!summon.played_this_turn);
            assert!(!summon.upgraded_this_turn);
            assert!(!summon.entered_main_this_turn);
        }
    }

    #[test]
    fn end_turn_resets_per_turn_summon_flags_for_both_players() {
        let flagged = SummonInstance {
            played_this_turn: true,
            upgraded_this_turn: true,
            entered_main_this_turn: true,
            ..whelp(PlayerId::One)
        };
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(flagged.clone());
        state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
            controller: PlayerId::Two,
            owner: PlayerId::Two,
            ..flagged
        });

        let outcome = end_turn(&state, PlayerId::One).expect("legal from Main");

        for player in [PlayerId::One, PlayerId::Two] {
            let summon = outcome
                .state
                .players
                .get(player)
                .main
                .as_ref()
                .expect("main set");
            assert!(!summon.played_this_turn, "{player:?}");
            assert!(!summon.upgraded_this_turn, "{player:?}");
            assert!(!summon.entered_main_this_turn, "{player:?}");
        }
    }

    #[test]
    fn a_summon_played_this_turn_can_be_upgraded_on_its_controllers_next_turn() {
        // Simulates what `PlaySummon` will set once it lands: a freshly
        // played Base Summon carries `played_this_turn = true` for the rest
        // of the turn it was played (rules §17, §52).
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            played_this_turn: true,
            ..whelp(PlayerId::One)
        });

        // Turn 1 (One) ends; Two's turn runs.
        let after_one = end_turn(&state, PlayerId::One).expect("legal from Main");
        assert!(
            !after_one
                .state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main set")
                .played_this_turn,
            "the flag is already clear as soon as One's own turn ends"
        );

        // Two's turn ends; play returns to One.
        let mut two_state = after_one.state;
        two_state.turn.phase = Phase::Main;
        let after_two = end_turn(&two_state, PlayerId::Two).expect("legal from Main");

        assert_eq!(after_two.state.turn.active_player, PlayerId::One);
        assert!(
            !after_two
                .state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main set")
                .played_this_turn,
            "still clear on One's next turn, so upgrading is no longer blocked"
        );
    }

    // -- convert_coin: anchoring and one-use ----------------------------------

    #[test]
    fn convert_coin_banks_an_anchored_type_and_removes_the_coin() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).has_coin = true;

        let outcome = convert_coin(&state, PlayerId::One, ManaType::Matter).expect("anchored");

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
    }

    #[test]
    fn convert_coin_rejects_an_out_of_anchor_type() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).has_coin = true;

        assert_eq!(
            convert_coin(&state, PlayerId::One, ManaType::Spirit),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_is_one_use() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).has_coin = true;
        let outcome = convert_coin(&state, PlayerId::One, ManaType::Matter).expect("first use");

        assert_eq!(
            convert_coin(&outcome.state, PlayerId::One, ManaType::Matter),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_rejects_a_player_with_no_coin() {
        let state = base_state();

        assert_eq!(
            convert_coin(&state, PlayerId::One, ManaType::Matter),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_is_rejected_outside_the_owners_main_phase() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).has_coin = true;
        state.turn.phase = Phase::Combat;

        assert_eq!(
            convert_coin(&state, PlayerId::One, ManaType::Matter),
            Err(ActionError::WrongPhase)
        );
    }

    // -- choose_mana_type -------------------------------------------------------

    #[test]
    fn choose_mana_type_banks_the_chosen_anchored_type_and_clears_pending() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(adept(PlayerId::One));
        state.pending = Some(PendingInput::ManaProduction {
            player: PlayerId::One,
            source: ManaSource::Player,
        });

        let outcome =
            choose_mana_type(&state, PlayerId::One, ManaType::Mind).expect("mind is anchored");

        assert_eq!(outcome.state.pending, None);
        assert_eq!(outcome.state.players.get(PlayerId::One).mana.mind, 1);
        assert_eq!(
            outcome.events,
            vec![GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Player,
                mana_type: ManaType::Mind,
            }]
        );
    }

    #[test]
    fn choose_mana_type_rejects_a_type_outside_the_anchor() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(adept(PlayerId::One));
        state.pending = Some(PendingInput::ManaProduction {
            player: PlayerId::One,
            source: ManaSource::Player,
        });

        assert_eq!(
            choose_mana_type(&state, PlayerId::One, ManaType::Spirit),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn choose_mana_type_without_a_pending_production_is_a_mismatch() {
        let state = base_state();

        assert_eq!(
            choose_mana_type(&state, PlayerId::One, ManaType::Matter),
            Err(ActionError::PendingInputMismatch)
        );
    }

    #[test]
    fn choose_mana_type_rejects_a_pending_decision_of_a_different_kind() {
        let mut state = base_state();
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        assert_eq!(
            choose_mana_type(&state, PlayerId::One, ManaType::Matter),
            Err(ActionError::PendingInputMismatch)
        );
    }
}
