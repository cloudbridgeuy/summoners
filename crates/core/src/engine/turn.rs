//! The turn handoff, the §47 response window, and the Coin (rules §7, §9,
//! §11–12, §47–48; design decisions 7 and 13).
//!
//! `EndTurn` (decision 7) does not hand the turn over directly: it opens a
//! final Combat response window, defender first, so the active player's
//! opponent always gets one last chance to act before the turn actually
//! changes hands (rules §47). The real handoff (`handover`, rules §48) runs
//! once both players pass consecutively with an empty Stack; see
//! `engine::stack::pass`, which calls `handover` directly rather than going
//! back through `dispatch`. `ConvertCoin` (decision 13) is legal both during
//! the owner's own resting Main Phase and at any point the owner holds
//! Priority in an open window, since a defender may want to convert their
//! Coin in response to a declared attack.

use crate::domain::cards::TriggerEvent;
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{ManaType, PlayerId};
use crate::domain::state::{
    DurationMarker, GameState, ManaSource, PendingInput, Phase, PlayerState, TurnState, WorkItem,
};
use crate::engine::apply::ActionOutcome;
use crate::engine::stack::window_after_play;
use crate::engine::triggers::discover_back;
use crate::engine::upkeep::{anchor_types, available_types, bank, reset_per_turn_summon_records};

// ---------------------------------------------------------------------------
// Dispatch handlers: `EndTurn`, `ConvertCoin`, `ChooseManaType`
// ---------------------------------------------------------------------------

/// `EndTurn` (decision 7, rules §9, §47): open the final Combat response
/// window instead of handing the turn over directly. The active player is
/// not required to attack; declining still provides a last response
/// opportunity, defender first (rules §47). Legal from a resting Main
/// Phase, or from Combat once an attack has resolved and the Stack is
/// clear again, with no pending decision and no window already open —
/// from any other shape it is `WrongPhase`. This does not change
/// `state.turn.phase`: the phase to rest in once the window closes is
/// never lost (see `TurnState.window`'s own doc comment). The actual
/// handoff (`handover`, rules §48) runs once both players pass
/// consecutively with an empty Stack; see `engine::stack::pass`.
pub(crate) fn end_turn(state: &GameState, player: PlayerId) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some()
        || state.turn.window.is_some()
        || !matches!(state.turn.phase, Phase::Main | Phase::Combat)
        || player != state.turn.active_player
    {
        return Err(ActionError::WrongPhase);
    }

    let mut state = state.clone();
    state.turn.window = Some(window_after_play(player));

    Ok(ActionOutcome {
        state,
        events: Vec::new(),
    })
}

/// Clear `DurationMarker::CannotBeMovedByOpponent` from every Summon on
/// `player_state`'s own board (Main and Bench) — the Old Sow's `Root and
/// Renew` expiring (rules §44). Distinct from
/// `reset_per_turn_summon_records`, which runs for both players every
/// handover; this runs only for the player becoming newly active, since the
/// marker is only ever cleared from its own holder's board once that
/// holder's next turn comes around (see the call site in `handover`).
fn expire_cannot_be_moved_by_opponent(player_state: &mut PlayerState) {
    if let Some(summon) = player_state.main.as_mut() {
        summon
            .duration_markers
            .retain(|marker| *marker != DurationMarker::CannotBeMovedByOpponent);
    }
    for slot in player_state.bench.iter_mut().flatten() {
        slot.duration_markers
            .retain(|marker| *marker != DurationMarker::CannotBeMovedByOpponent);
    }
}

/// The direct turn handoff (rules §48), reached once both players pass
/// consecutively with an empty Stack (see `engine::stack::pass`). Every
/// Summon's per-turn flags reset for both players (see
/// `reset_per_turn_summon_records`), the opponent's Upkeep begins, and
/// `Ready`, draw, natural production, and finally the Main Phase advance
/// itself (`WorkItem::BeginMainPhase`, rules §9) are queued as work for the
/// resolution loop to drain. Queuing the advance last, behind everything
/// else Upkeep does, is what keeps it correct even when `ProduceMana`
/// pauses for a Mana Type choice: the drain loop resumes this same queue
/// once that choice is answered, so the Main Phase is still reached only
/// after the choice lands, never before. `state.turn.active_player` is
/// still the player whose turn is ending — opening the §47 window and
/// passing Priority back and forth never changes it — so this needs no
/// separate player argument.
pub(crate) fn handover(state: &GameState) -> ActionOutcome {
    let mut state = state.clone();
    let player = state.turn.active_player;
    let opponent = player.opponent();
    state.turn = TurnState {
        active_player: opponent,
        phase: Phase::Upkeep,
        window: None,
        normal_attack_used: false,
        normal_retreat_used: false,
        spell_played_this_turn: crate::domain::state::PerPlayer::new(false, false),
    };
    reset_per_turn_summon_records(state.players.get_mut(player));
    reset_per_turn_summon_records(state.players.get_mut(opponent));
    // The Old Sow's `Root and Renew` (rules §44) sets `CannotBeMovedByOpponent`
    // on the Sow's own controller's board during that controller's Main
    // Phase, to survive exactly one opposing turn. It only ever expires on
    // the handover that makes its holder newly active again — the handover
    // right after `player` set it hands play to `opponent` and leaves the
    // marker alone (it protects through the whole of `opponent`'s coming
    // turn); the handover after that makes the original holder active again
    // and is where the marker is cleared. Clearing unconditionally from
    // whoever is newly active on every handover reaches that same holder on
    // exactly that later handover, and is a harmless no-op the rest of the
    // time, since the marker can only ever sit on its own holder's board.
    expire_cannot_be_moved_by_opponent(state.players.get_mut(opponent));
    state.work.push_back(WorkItem::ReadyAll);
    // Rules §36, §41: a Trigger on `YourUpkeep` fires for the newly active
    // player's own board, Main then Bench, ahead of the draw and natural
    // production this same Upkeep also queues.
    state = discover_back(&state, &[opponent], TriggerEvent::YourUpkeep);
    state.work.push_back(WorkItem::DrawCard);
    state.work.push_back(WorkItem::ProduceMana {
        player: opponent,
        source: ManaSource::Player,
    });
    // Rules §9: "Phases only move forward." Queued last, so the Main Phase
    // is reached only once every other Upkeep step above has drained.
    state.work.push_back(WorkItem::BeginMainPhase);

    ActionOutcome {
        state,
        events: vec![GameEvent::TurnBegan { player: opponent }],
    }
}

/// `ConvertCoin` (rules §7, decision 13): exchange the one-use Coin for one
/// anchored Mana.
///
/// Legal during the owner's own resting Main Phase, or at any point the
/// owner holds Priority in an open window (decision 13) — the latter lets
/// a defender convert their Coin in response to a declared attack, for
/// instance. Anything else is `WrongPhase`. The chosen Type must be in the
/// owner's anchor, the same rule `produce_mana` uses; a missing Coin or an
/// out-of-anchor Type are both reported as `InvalidTarget`, since neither
/// names a legal target for this conversion.
pub(crate) fn convert_coin(
    state: &GameState,
    player: PlayerId,
    mana_type: ManaType,
) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::WrongPhase);
    }
    let resting_in_own_main = state.turn.window.is_none()
        && state.turn.phase == Phase::Main
        && player == state.turn.active_player;
    let holds_priority = matches!(state.turn.window, Some(window) if window.holder == player);
    if !resting_in_own_main && !holds_priority {
        return Err(ActionError::WrongPhase);
    }

    if player != PlayerId::Two || state.coin.is_none() {
        return Err(ActionError::InvalidTarget);
    }
    if !anchor_types(&state.cards, state.players.get(player)).contains(&mana_type) {
        return Err(ActionError::InvalidTarget);
    }

    let mut state = state.clone();
    state.coin = None;
    bank(state.players.get_mut(player), mana_type);

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
    let available = available_types(&state.cards, state.players.get(pending_player), source);
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
    use crate::domain::cards::fixtures;
    use crate::domain::ids::{CardInstanceId, Position};
    use crate::domain::state::{
        CardRef, Coin, GameStatus, ManaBank, PerPlayer, PlayerState, StackWindow, SummonInstance,
        UpgradeChain,
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
            readiness: crate::domain::state::Readiness::Exhausted,
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

    fn dawn_tender(owner: PlayerId) -> SummonInstance {
        chain_summon(owner, "dawn-tender", 3)
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

    fn player_two_main_state_with_coin() -> GameState {
        let mut state = base_state();
        state.turn.active_player = PlayerId::Two;
        state.coin = Some(Coin);
        state
    }

    // -- end_turn: opening the §47 window, and the handover it leads to ------

    /// Run the full `EndTurn` sequence — opening the §47 window, the
    /// defender passing first, then the active player passing second — to
    /// reach the handover the old single-call `end_turn` used to produce
    /// directly.
    fn full_end_turn(state: &GameState, player: PlayerId) -> ActionOutcome {
        let opened = end_turn(state, player).expect("legal from a resting Main Phase");
        let defender = player.opponent();
        let after_defender_pass = crate::engine::stack::pass(&opened.state, defender)
            .expect("defender holds Priority first (rules §47)");
        crate::engine::stack::pass(&after_defender_pass.state, player)
            .expect("active player holds Priority second")
    }

    #[test]
    fn end_turn_opens_the_final_combat_response_window_with_the_defender_first() {
        let state = base_state();

        let outcome = end_turn(&state, PlayerId::One).expect("legal from Main");

        assert!(outcome.events.is_empty());
        assert_eq!(outcome.state.turn.active_player, PlayerId::One);
        assert_eq!(
            outcome.state.turn.phase,
            Phase::Main,
            "the phase to rest in is preserved, not changed, by opening the window"
        );
        assert_eq!(
            outcome.state.turn.window,
            Some(StackWindow {
                holder: PlayerId::Two,
                prior_pass: false,
            })
        );
    }

    #[test]
    fn end_turn_hands_off_and_queues_the_opponents_upkeep_once_both_players_pass() {
        let state = base_state();

        let outcome = full_end_turn(&state, PlayerId::One);

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::PriorityPassed {
                    player: PlayerId::One
                },
                GameEvent::TurnBegan {
                    player: PlayerId::Two
                },
            ]
        );
        assert_eq!(outcome.state.turn.active_player, PlayerId::Two);
        assert_eq!(outcome.state.turn.phase, Phase::Upkeep);
        assert_eq!(
            outcome.state.work,
            VecDeque::from(vec![
                WorkItem::ReadyAll,
                WorkItem::DrawCard,
                WorkItem::ProduceMana {
                    player: PlayerId::Two,
                    source: ManaSource::Player,
                },
                WorkItem::BeginMainPhase,
            ])
        );
    }

    #[test]
    fn end_turn_hands_off_and_fires_the_new_active_players_your_upkeep_trigger() {
        // Rules §36, §41: Dawn Tender's immediate Heal fires as soon as the
        // newly active player's own Upkeep begins — `handover` queues it
        // (`discover_back`) ahead of the draw and natural production this
        // same Upkeep also queues, and the resolution loop then fires it in
        // that order, exactly the way a movement trigger or a destruction
        // trigger already fires through `engine::resolution::drain`.
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
            damage: 15,
            ..dawn_tender(PlayerId::Two)
        });
        state.players.get_mut(PlayerId::Two).deck = vec![CardRef {
            instance: CardInstanceId(20),
            def: fixtures::id("quarry-whelp"),
        }];

        let outcome = full_end_turn(&state, PlayerId::One);
        let (state, drained_events) = crate::engine::resolution::drain(&outcome.state);

        assert_eq!(
            drained_events,
            vec![
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::TriggerFired {
                    controller: PlayerId::Two,
                    position: Position::Main,
                    event: TriggerEvent::YourUpkeep,
                    ability: fixtures::trigger_id("dawn-tender"),
                },
                GameEvent::Healed {
                    position: Position::Main,
                    amount: 10,
                },
                GameEvent::CardDrawn {
                    player: PlayerId::Two,
                    card: CardInstanceId(20),
                },
                GameEvent::ManaProduced {
                    player: PlayerId::Two,
                    source: ManaSource::Player,
                    mana_type: ManaType::Spirit,
                },
            ]
        );
        assert!(state.work.is_empty());
        assert_eq!(
            state.turn.phase,
            Phase::Main,
            "the drained queue's own last item lands the new turn in its Main Phase (rules §9)"
        );
        assert_eq!(
            state
                .players
                .get(PlayerId::Two)
                .main
                .as_ref()
                .expect("main")
                .damage,
            5,
            "Dawn Tender's own Heal reduced its accumulated Damage"
        );
    }

    #[test]
    fn end_turn_is_rejected_outside_a_resting_main_or_combat_phase() {
        let mut state = base_state();
        state.turn.phase = Phase::Upkeep;

        assert_eq!(
            end_turn(&state, PlayerId::One),
            Err(ActionError::WrongPhase)
        );
    }

    #[test]
    fn end_turn_is_legal_from_combat_once_the_stack_is_clear_again() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;

        let outcome = end_turn(&state, PlayerId::One).expect("Combat rests once the Stack clears");

        assert_eq!(outcome.state.turn.phase, Phase::Combat);
        assert_eq!(
            outcome.state.turn.window,
            Some(StackWindow {
                holder: PlayerId::Two,
                prior_pass: false,
            })
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

    #[test]
    fn end_turn_resets_per_turn_summon_flags_for_both_players() {
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
            ..whelp(PlayerId::Two)
        };
        let mut state = base_state();
        state.turn.spell_played_this_turn = PerPlayer::new(true, true);
        state.players.get_mut(PlayerId::One).main = Some(played);
        state.players.get_mut(PlayerId::Two).main = Some(upgraded);

        let outcome = full_end_turn(&state, PlayerId::One);

        for player in [PlayerId::One, PlayerId::Two] {
            let summon = outcome
                .state
                .players
                .get(player)
                .main
                .as_ref()
                .expect("main set");
            assert_eq!(
                summon.turn,
                crate::domain::state::SummonTurnRecord::fresh(),
                "{player:?}"
            );
            assert!(!*outcome.state.turn.spell_played_this_turn.get(player));
        }
    }

    #[test]
    fn a_summon_played_this_turn_can_be_upgraded_on_its_controllers_next_turn() {
        // Simulates what `PlaySummon` will set once it lands: a freshly
        // played Base Summon records `PlayedThisTurn` for the rest
        // of the turn it was played (rules §17, §52).
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            turn: crate::domain::state::SummonTurnRecord {
                upgrade: crate::domain::state::UpgradeActivity::PlayedThisTurn,
                main_entry: None,
            },
            ..whelp(PlayerId::One)
        });
        // Each player needs a card to draw so their own Upkeep can drain
        // all the way through to `WorkItem::BeginMainPhase` below, instead
        // of stalling on an empty-Deck loss this test has no interest in.
        state.players.get_mut(PlayerId::One).deck = vec![CardRef {
            instance: CardInstanceId(30),
            def: fixtures::id("quarry-whelp"),
        }];
        state.players.get_mut(PlayerId::Two).deck = vec![CardRef {
            instance: CardInstanceId(31),
            def: fixtures::id("quarry-whelp"),
        }];

        // Turn 1 (One) ends; Two's turn runs, its Upkeep drained in full —
        // reaching Main on its own (rules §9), with no hand edit needed.
        let after_one = full_end_turn(&state, PlayerId::One);
        let (after_one, _) = crate::engine::resolution::drain(&after_one.state);
        assert_eq!(
            after_one
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main set")
                .turn
                .upgrade,
            crate::domain::state::UpgradeActivity::Available,
            "the activity is available as soon as One's own turn ends"
        );

        // Two's turn ends; play returns to One, whose own Upkeep drains the
        // same way.
        let after_two = full_end_turn(&after_one, PlayerId::Two);
        let (after_two, _) = crate::engine::resolution::drain(&after_two.state);

        assert_eq!(after_two.turn.active_player, PlayerId::One);
        assert_eq!(
            after_two.turn.phase,
            Phase::Main,
            "One's own Upkeep reaches Main on its own too, with no hand edit"
        );
        assert_eq!(
            after_two
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main set")
                .turn
                .upgrade,
            crate::domain::state::UpgradeActivity::Available,
            "still clear on One's next turn, so upgrading is no longer blocked"
        );
    }

    // -- convert_coin: anchoring and one-use ----------------------------------

    #[test]
    fn convert_coin_banks_an_anchored_type_and_removes_the_coin() {
        let state = player_two_main_state_with_coin();

        let outcome = convert_coin(&state, PlayerId::Two, ManaType::Matter).expect("anchored");

        assert_eq!(
            outcome.events,
            vec![GameEvent::CoinConverted {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
            }]
        );
        let player_state = outcome.state.players.get(PlayerId::Two);
        assert_eq!(player_state.mana.matter, 1);
        assert_eq!(outcome.state.coin, None);
    }

    #[test]
    fn convert_coin_rejects_an_out_of_anchor_type() {
        let state = player_two_main_state_with_coin();

        assert_eq!(
            convert_coin(&state, PlayerId::Two, ManaType::Spirit),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_is_one_use() {
        let state = player_two_main_state_with_coin();
        let outcome = convert_coin(&state, PlayerId::Two, ManaType::Matter).expect("first use");

        assert_eq!(
            convert_coin(&outcome.state, PlayerId::Two, ManaType::Matter),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_rejects_a_player_with_no_coin() {
        let mut state = base_state();
        state.turn.active_player = PlayerId::Two;

        assert_eq!(
            convert_coin(&state, PlayerId::Two, ManaType::Matter),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_is_legal_while_the_owner_holds_priority() {
        // The owner is Two, the non-active player, holding Priority in a
        // window opened during One's Combat — decision 13's second legal
        // context, exercised independently of whose turn it is.
        let mut state = base_state();
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        });
        state.coin = Some(Coin);

        let outcome =
            convert_coin(&state, PlayerId::Two, ManaType::Matter).expect("Two holds Priority");

        assert_eq!(
            outcome.events,
            vec![GameEvent::CoinConverted {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
            }]
        );
        assert_eq!(outcome.state.players.get(PlayerId::Two).mana.matter, 1);
    }

    #[test]
    fn convert_coin_still_rejects_a_window_holder_who_is_not_the_owner() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });
        state.coin = Some(Coin);

        assert_eq!(
            convert_coin(&state, PlayerId::One, ManaType::Matter),
            Err(ActionError::InvalidTarget)
        );
    }

    #[test]
    fn convert_coin_is_rejected_outside_the_owners_main_phase() {
        let mut state = player_two_main_state_with_coin();
        state.turn.phase = Phase::Combat;

        assert_eq!(
            convert_coin(&state, PlayerId::Two, ManaType::Matter),
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
