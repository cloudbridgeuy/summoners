//! Upkeep work — Ready, draw, and natural Mana production (rules §10–12) —
//! and the anchoring rule and per-turn flag reset they, and `engine::turn`,
//! share (rules §11–12; design decisions 12–13). The turn handoff, the §47
//! window, and the Coin live in `engine::turn`, kept separate purely to
//! stay under this crate's file-length cap.
//!
//! The anchoring rule: a Mana Type is available to a player only if some
//! Summon they control in Main or on the Bench currently prints it (read
//! through the card tree, never off a cached value, since the topmost card
//! of a chain can change between Upkeeps). Natural production and the Coin
//! both anchor to the union of every controlled Summon's printed Types;
//! decision 12 makes that union a pause point only when it holds more than
//! one Type.

use crate::domain::cards::{CardSet, ManaTypes};
use crate::domain::events::GameEvent;
use crate::domain::ids::{BenchSlot, ManaType, PlayerId, Position};
use crate::domain::state::{
    GameState, ManaSource, PendingInput, Phase, PlayerState, Readiness, SummonInstance,
    SummonTurnRecord,
};

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
/// straight off its entity. A card printing no `Produces` component at all,
/// and an unresolvable definition — parsing already guarantees every card
/// on the board resolves, so the latter only happens if a caller builds a
/// `GameState` by hand with a bad reference — both honestly answer "produces
/// nothing" for a pure function that cannot error. A card printing more than
/// one `Produces` component anchors on the first.
fn produced_types(cards: &CardSet, summon: &SummonInstance) -> Vec<ManaType> {
    cards
        .get(summon.chain.top().def)
        .and_then(|entity| entity.get::<ManaTypes>())
        .map(|types| types.0.clone())
        .unwrap_or_default()
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
pub(crate) fn anchor_types(cards: &CardSet, player_state: &PlayerState) -> Vec<ManaType> {
    let mut seen = [false; 3];
    for summon in controlled_summons(player_state) {
        for mana_type in produced_types(cards, summon) {
            seen[mana_type_index(mana_type)] = true;
        }
    }
    [ManaType::Matter, ManaType::Mind, ManaType::Spirit]
        .into_iter()
        .filter(|mana_type| seen[mana_type_index(*mana_type)])
        .collect()
}

/// The Mana Types printed on the one Summon at `position`, or none if that
/// position is empty. Summon-source production uses this instead of the
/// player's natural production anchor.
fn summon_types(cards: &CardSet, player_state: &PlayerState, position: Position) -> Vec<ManaType> {
    let summon = match position {
        Position::Main => player_state.main.as_ref(),
        Position::Bench(slot) => player_state.bench[slot.index()].as_ref(),
    };
    summon
        .map(|summon| produced_types(cards, summon))
        .unwrap_or_default()
}

/// The Mana Types available for one `ManaSource`, dispatching to the
/// player-wide anchor or one Summon's own printed Types. `pub(crate)`
/// because `engine::turn::choose_mana_type` answers a paused
/// `ManaProduction` decision against the same set this module's own
/// `produce_mana` used to pause it.
pub(crate) fn available_types(
    cards: &CardSet,
    player_state: &PlayerState,
    source: ManaSource,
) -> Vec<ManaType> {
    match source {
        ManaSource::Player => anchor_types(cards, player_state),
        ManaSource::Summon(position) => summon_types(cards, player_state, position),
    }
}

/// `pub(crate)` because `engine::turn::convert_coin` and
/// `engine::turn::choose_mana_type` both bank Mana the same way
/// `produce_mana` does here.
pub(crate) fn bank(player_state: &mut PlayerState, mana_type: ManaType) {
    match mana_type {
        ManaType::Matter => player_state.mana.matter += 1,
        ManaType::Mind => player_state.mana.mind += 1,
        ManaType::Spirit => player_state.mana.spirit += 1,
    }
}

// ---------------------------------------------------------------------------
// Per-turn Summon record reset
// ---------------------------------------------------------------------------

/// Clear the per-turn record on every Summon in `player_state`, Main and
/// Bench alike. Upgrade activity enforces rules §17, §18, and §52. Main
/// entry is read by an opposing attacker's conditional bonus during that
/// attacker's own turn.
///
/// All three share one reset point: the moment a turn changes hands, for
/// both players' boards (see `engine::turn::handover`). The rules describe
/// each fact against "this turn" as a single game-wide concept (§9:
/// "players alternate complete turns"), not a clock that runs only while a
/// Summon's controller happens to be active. That reading is provably
/// correct for upgrade activity: it is read
/// only during their controller's own Main Phase, which cannot arrive
/// before their own next Upkeep, so clearing them at the handoff that
/// starts the *other* player's turn already lands before that next read.
/// It is also the only reading available for Main entry,
/// which can be set on either player's Summon and must stay true for the
/// remainder of the turn that set it, whether that Summon belongs to the
/// player about to act or not — clearing only the newly active player's
/// own board would leave a stale `true` sitting on the other player's
/// board indefinitely. `pub(crate)` because `engine::turn::handover` is
/// where this reset actually runs.
pub(crate) fn reset_per_turn_summon_records(player_state: &mut PlayerState) {
    if let Some(summon) = player_state.main.as_mut() {
        clear_per_turn_flags(summon);
    }
    for summon in player_state.bench.iter_mut().flatten() {
        clear_per_turn_flags(summon);
    }
}

fn clear_per_turn_flags(summon: &mut SummonInstance) {
    summon.turn = SummonTurnRecord::fresh();
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
        summon.readiness = Readiness::Ready;
    }
    for summon in player_state.bench.iter_mut().flatten() {
        summon.readiness = Readiness::Ready;
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

/// Rules §10 step 3, §11–12, decision 12: generate Mana for `player` from
/// `source`. A single available Type produces without pausing;
/// several Types pause on `PendingInput::ManaProduction` for
/// `ChooseManaType` to answer; no available Type (an empty anchor) produces
/// nothing and is a documented no-op rather than an error, since a
/// `WorkItem` executor cannot reject.
pub(crate) fn produce_mana(
    state: &GameState,
    player: PlayerId,
    source: ManaSource,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let available = available_types(&state.cards, state.players.get(player), source);

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

/// `WorkItem::BeginMainPhase` (rules §9): once every other Upkeep step has
/// drained, the turn moves forward into the Main Phase on its own — phases
/// only move forward, so nothing else ever advances out of `Phase::Upkeep`.
/// `engine::turn::handover` queues this last, after `ReadyAll`, any
/// `YourUpkeep` triggers, `DrawCard`, and player-wide Mana production,
/// so it always runs once every one of those has finished — including a
/// `ManaProduction` pause and its answer, since the drain loop resumes this
/// same queue exactly where it paused once `pending` clears. A plain state
/// change with nothing to report, the same way `engine::loss::check`'s
/// short-of-a-loss path reports nothing.
pub(crate) fn begin_main_phase(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    state.turn.phase = Phase::Main;
    (state, Vec::new())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::fixtures;
    use crate::domain::ids::{CardInstanceId, PlayerId};
    use crate::domain::state::{
        CardRef, GameStatus, ManaBank, PerPlayer, Phase, TurnState, UpgradeChain,
    };
    use std::collections::VecDeque;

    fn chain_summon(owner: PlayerId, def: &'static str, instance: u32) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(instance),
                    def: fixtures::id(def),
                },
                vec![],
            ),
            damage: 0,
            readiness: Readiness::Exhausted,
            owner,
            controller: owner,
            duration_markers: vec![],
            turn: crate::domain::state::SummonTurnRecord::fresh(),
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
            enchantments: vec![],
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
            coin: None,
            turn: TurnState {
                active_player: PlayerId::One,
                phase: Phase::Main,
                window: None,
                normal_attack_used: false,
                normal_retreat_used: false,
                spell_played_this_turn: PerPlayer::new(false, false),
            },
            stack: vec![],
            stack_segment_bases: vec![],
            work: VecDeque::new(),
            pending: None,
            status: GameStatus::Playing,
            cards: fixtures::card_set(),
        }
    }

    // -- anchor_types: mono, dual, multi-Summon, and empty boards --------

    #[test]
    fn anchor_types_reads_a_single_type_from_one_mono_type_summon() {
        let player_state = PlayerState {
            main: Some(whelp(PlayerId::One)),
            ..empty_player_state()
        };

        assert_eq!(
            anchor_types(&fixtures::card_set(), &player_state),
            vec![ManaType::Matter]
        );
    }

    #[test]
    fn anchor_types_reads_both_types_from_one_dual_type_summon() {
        let player_state = PlayerState {
            main: Some(adept(PlayerId::One)),
            ..empty_player_state()
        };

        assert_eq!(
            anchor_types(&fixtures::card_set(), &player_state),
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
            anchor_types(&fixtures::card_set(), &player_state),
            vec![ManaType::Matter, ManaType::Mind]
        );
    }

    #[test]
    fn anchor_types_is_empty_for_a_board_with_no_summons() {
        assert_eq!(
            anchor_types(&fixtures::card_set(), &empty_player_state()),
            Vec::<ManaType>::new()
        );
    }

    // -- ready_all ---------------------------------------------------------

    #[test]
    fn ready_all_readies_every_controlled_position_main_first_then_bench_in_order() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).bench[1] = Some(adept(PlayerId::One));

        let (state, events) = ready_all(&state);

        let player_state = state.players.get(PlayerId::One);
        assert_eq!(
            player_state.main.as_ref().expect("main set").readiness,
            Readiness::Ready
        );
        assert_eq!(
            player_state.bench[1].as_ref().expect("bench set").readiness,
            Readiness::Ready
        );
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
            def: fixtures::id("quarry-whelp"),
        };
        state.players.get_mut(PlayerId::One).deck = vec![
            top,
            CardRef {
                instance: CardInstanceId(51),
                def: fixtures::id("quarry-whelp"),
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

        let (state, events) = produce_mana(&state, PlayerId::One, ManaSource::Player);

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

        let (state, events) = produce_mana(&state, PlayerId::One, ManaSource::Player);

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

        let (state, events) = produce_mana(&state, PlayerId::One, ManaSource::Player);

        assert!(events.is_empty());
        assert_eq!(state.pending, None);
        assert_eq!(state.players.get(PlayerId::One).mana, ManaBank::default());
    }

    // -- begin_main_phase -----------------------------------------------------

    #[test]
    fn begin_main_phase_advances_upkeep_to_main_and_reports_nothing() {
        let mut state = base_state();
        state.turn.phase = Phase::Upkeep;

        let (state, events) = begin_main_phase(&state);

        assert_eq!(state.turn.phase, Phase::Main);
        assert!(events.is_empty());
    }

    // -- reset_per_turn_summon_records ---------------------------------------

    #[test]
    fn reset_per_turn_summon_flags_clears_every_controlled_summon_main_and_bench() {
        let played = SummonInstance {
            turn: crate::domain::state::SummonTurnRecord {
                upgrade: crate::domain::state::UpgradeActivity::PlayedThisTurn,
                main_entry: Some(crate::domain::state::EnteredMain),
            },
            ..whelp(PlayerId::One)
        };
        let upgraded = SummonInstance {
            turn: crate::domain::state::SummonTurnRecord {
                upgrade: crate::domain::state::UpgradeActivity::UpgradedThisTurn,
                main_entry: Some(crate::domain::state::EnteredMain),
            },
            ..whelp(PlayerId::One)
        };
        let mut player_state = PlayerState {
            main: Some(played),
            ..empty_player_state()
        };
        player_state.bench[0] = Some(SummonInstance {
            controller: PlayerId::One,
            ..upgraded
        });

        reset_per_turn_summon_records(&mut player_state);

        for summon in [
            player_state.main.as_ref().expect("main set"),
            player_state.bench[0].as_ref().expect("bench set"),
        ] {
            assert_eq!(summon.turn, SummonTurnRecord::fresh());
        }
    }
}
