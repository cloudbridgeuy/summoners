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
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::domain::cards::fixtures;
    use crate::domain::ids::CardInstanceId;
    use crate::domain::state::{
        CardRef, GameStatus, ManaBank, PendingInput, PerPlayer, StackWindow, TurnState,
    };
    use std::collections::VecDeque;

    fn card_ref(instance: u32, def: &'static str) -> CardRef {
        CardRef {
            instance: CardInstanceId(instance),
            def: fixtures::id(def),
        }
    }

    fn base_summon(owner: PlayerId, def: &'static str) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(card_ref(1, def), vec![]),
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

    fn empty_player(owner: PlayerId) -> PlayerState {
        PlayerState {
            main: Some(base_summon(owner, "quarry-whelp")),
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
            players: PerPlayer::new(empty_player(PlayerId::One), empty_player(PlayerId::Two)),
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
            status: GameStatus::Playing,
            cards: fixtures::card_set(),
        }
    }

    // --- play_summon ---------------------------------------------------

    #[test]
    fn play_summon_puts_a_base_into_an_empty_bench_slot_exhausted_and_played_this_turn() {
        let mut state = base_state();
        let card = card_ref(2, "quarry-whelp");
        state.players.one.hand.push(card);

        let outcome = play_summon(&state, PlayerId::One, card.instance, BenchSlot::First)
            .expect("a Base Summon may be played into an empty Bench slot");

        let bench = outcome.state.players.get(PlayerId::One).bench[0]
            .as_ref()
            .expect("the Bench slot now holds the played Summon");
        assert!(!bench.ready);
        assert!(bench.played_this_turn);
        assert_eq!(bench.chain.base(), card);
        assert!(outcome.state.players.get(PlayerId::One).hand.is_empty());
    }

    #[test]
    fn play_summon_emits_exactly_one_summon_played_event() {
        let mut state = base_state();
        let card = card_ref(2, "quarry-whelp");
        state.players.one.hand.push(card);

        let outcome = play_summon(&state, PlayerId::One, card.instance, BenchSlot::Second)
            .expect("a Base Summon may be played into an empty Bench slot");

        assert_eq!(
            outcome.events,
            vec![GameEvent::SummonPlayed {
                player: PlayerId::One,
                card: card.instance,
                slot: BenchSlot::Second,
            }]
        );
    }

    #[test]
    fn play_summon_rejects_a_pending_decision() {
        let mut state = base_state();
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        let result = play_summon(&state, PlayerId::One, CardInstanceId(2), BenchSlot::First);

        assert_eq!(result, Err(ActionError::PendingInputMismatch));
    }

    #[test]
    fn play_summon_rejects_outside_main_phase() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;

        let result = play_summon(&state, PlayerId::One, CardInstanceId(2), BenchSlot::First);

        assert_eq!(result, Err(ActionError::WrongPhase));
    }

    #[test]
    fn play_summon_rejects_an_open_priority_window() {
        let mut state = base_state();
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });

        let result = play_summon(&state, PlayerId::One, CardInstanceId(2), BenchSlot::First);

        assert_eq!(result, Err(ActionError::WrongPhase));
    }

    #[test]
    fn play_summon_rejects_an_occupied_bench_slot() {
        let mut state = base_state();
        let card = card_ref(2, "quarry-whelp");
        state.players.one.hand.push(card);
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "quarry-whelp"));

        let result = play_summon(&state, PlayerId::One, card.instance, BenchSlot::First);

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    #[test]
    fn play_summon_rejects_a_card_not_in_hand() {
        let state = base_state();

        let result = play_summon(&state, PlayerId::One, CardInstanceId(99), BenchSlot::First);

        assert_eq!(result, Err(ActionError::UnknownCard));
    }

    #[test]
    fn play_summon_rejects_a_non_base_form() {
        let mut state = base_state();
        let card = card_ref(2, "quarry-brute");
        state.players.one.hand.push(card);

        let result = play_summon(&state, PlayerId::One, card.instance, BenchSlot::First);

        assert_eq!(result, Err(ActionError::UnknownCard));
    }

    // --- upgrade_summon --------------------------------------------------

    #[test]
    fn upgrade_summon_stacks_the_new_top_ready_false_and_upgraded_flag_set() {
        let mut state = base_state();
        let upgrade = card_ref(2, "quarry-brute");
        state.players.one.hand.push(upgrade);

        let outcome = upgrade_summon(&state, PlayerId::One, upgrade.instance, Position::Main)
            .expect("Base to Enhanced is a legal climb with matching Mana Types");

        let main = outcome
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("Main still holds the Summon");
        assert_eq!(main.chain.top(), upgrade);
        assert_eq!(main.chain.base(), card_ref(1, "quarry-whelp"));
        assert!(!main.ready);
        assert!(main.upgraded_this_turn);
        assert!(outcome.state.players.get(PlayerId::One).hand.is_empty());
        assert_eq!(
            outcome.events,
            vec![GameEvent::SummonUpgraded {
                player: PlayerId::One,
                card: upgrade.instance,
                position: Position::Main,
            }]
        )
    }

    #[test]
    fn upgrade_summon_rejects_a_pending_decision() {
        let mut state = base_state();
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        let result = upgrade_summon(&state, PlayerId::One, CardInstanceId(2), Position::Main);

        assert_eq!(result, Err(ActionError::PendingInputMismatch));
    }

    #[test]
    fn upgrade_summon_rejects_outside_main_phase() {
        let mut state = base_state();
        state.turn.phase = Phase::Upkeep;

        let result = upgrade_summon(&state, PlayerId::One, CardInstanceId(2), Position::Main);

        assert_eq!(result, Err(ActionError::WrongPhase));
    }

    #[test]
    fn upgrade_summon_rejects_an_empty_position() {
        let state = base_state();

        let result = upgrade_summon(
            &state,
            PlayerId::One,
            CardInstanceId(2),
            Position::Bench(BenchSlot::First),
        );

        assert_eq!(result, Err(ActionError::EmptyPosition));
    }

    #[test]
    fn upgrade_summon_rejects_a_summon_played_this_turn() {
        let mut state = base_state();
        let upgrade = card_ref(2, "quarry-brute");
        state.players.one.hand.push(upgrade);
        let Some(main) = &mut state.players.one.main else {
            unreachable!("fixture always sets up a Main Summon")
        };
        main.played_this_turn = true;

        let result = upgrade_summon(&state, PlayerId::One, upgrade.instance, Position::Main);

        assert_eq!(result, Err(ActionError::PlayedThisTurn));
    }

    #[test]
    fn upgrade_summon_rejects_a_summon_already_upgraded_this_turn() {
        let mut state = base_state();
        let upgrade = card_ref(2, "quarry-brute");
        state.players.one.hand.push(upgrade);
        let Some(main) = &mut state.players.one.main else {
            unreachable!("fixture always sets up a Main Summon")
        };
        main.upgraded_this_turn = true;

        let result = upgrade_summon(&state, PlayerId::One, upgrade.instance, Position::Main);

        assert_eq!(result, Err(ActionError::AlreadyUpgradedThisTurn));
    }

    #[test]
    fn upgrade_summon_rejects_a_form_that_does_not_climb() {
        let mut state = base_state();
        let same_form = card_ref(2, "set-path-adept");
        state.players.one.hand.push(same_form);

        let result = upgrade_summon(&state, PlayerId::One, same_form.instance, Position::Main);

        assert_eq!(result, Err(ActionError::IllegalUpgradeTarget));
    }

    #[test]
    fn upgrade_summon_rejects_a_higher_form_from_a_different_lineage() {
        // Main holds a Set-Path Adept (Produces Matter, Mind). Quarry Brute
        // climbs Base -> Enhanced but only produces Matter, dropping Mind —
        // rules §18's Mana-Type-superset requirement, not just Form order.
        let mut state = base_state();
        state.players.one.main = Some(base_summon(PlayerId::One, "set-path-adept"));
        let cross_lineage = card_ref(2, "quarry-brute");
        state.players.one.hand.push(cross_lineage);

        let result = upgrade_summon(
            &state,
            PlayerId::One,
            cross_lineage.instance,
            Position::Main,
        );

        assert_eq!(result, Err(ActionError::IllegalUpgradeTarget));
    }

    #[test]
    fn upgrade_summon_rejects_a_card_not_in_hand() {
        let state = base_state();

        let result = upgrade_summon(&state, PlayerId::One, CardInstanceId(99), Position::Main);

        assert_eq!(result, Err(ActionError::UnknownCard));
    }

    // --- retreat ----------------------------------------------------------

    #[test]
    fn retreat_pays_the_printed_cost_and_swaps_main_with_the_chosen_bench_slot() {
        let mut state = base_state();
        state.players.one.mana = ManaBank {
            matter: 5,
            mind: 0,
            spirit: 0,
        };
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

        let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
            .expect("the bank can cover Quarry Whelp's printed Retreat Cost of 1");

        let player = outcome.state.players.get(PlayerId::One);
        assert_eq!(
            player.main.as_ref().map(|s| s.chain.base().def),
            Some(fixtures::id("set-path-adept"))
        );
        assert_eq!(
            player.bench[0].as_ref().map(|s| s.chain.base().def),
            Some(fixtures::id("quarry-whelp"))
        );
        assert_eq!(
            player.mana,
            ManaBank {
                matter: 4,
                mind: 0,
                spirit: 0,
            }
        );
        assert!(outcome.state.turn.normal_retreat_used);

        // `retreat` itself only queues the four movement triggers (rules
        // §28); it never sets `entered_main_this_turn` directly. Draining
        // the queue runs the `EnteringMain` step through
        // `engine::triggers::movement_trigger`, the one place that does.
        let (drained, _) = crate::engine::resolution::drain(&outcome.state);
        let drained_player = drained.players.get(PlayerId::One);
        assert!(
            drained_player
                .main
                .as_ref()
                .is_some_and(|s| s.entered_main_this_turn)
        );
    }

    #[test]
    fn retreat_emits_mana_deducted_then_summons_swapped() {
        let mut state = base_state();
        state.players.one.mana = ManaBank {
            matter: 5,
            mind: 0,
            spirit: 0,
        };
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

        let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
            .expect("the bank can cover the printed Retreat Cost");

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::ManaDeducted {
                    player: PlayerId::One,
                    mana_type: ManaType::Matter,
                    amount: 1,
                },
                GameEvent::SummonsSwapped {
                    player: PlayerId::One,
                    main: BenchSlot::First,
                },
            ]
        );
    }

    #[test]
    fn retreat_enqueues_the_four_movement_triggers_in_fixed_order() {
        let mut state = base_state();
        state.players.one.mana = ManaBank {
            matter: 5,
            mind: 0,
            spirit: 0,
        };
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

        let outcome = retreat(&state, PlayerId::One, BenchSlot::First, None)
            .expect("the bank can cover the printed Retreat Cost");

        let work: Vec<_> = outcome.state.work.into_iter().collect();
        assert_eq!(
            work,
            vec![
                WorkItem::MovementTrigger(
                    MovementStep::LeavingMain,
                    PlayerId::One,
                    Position::Bench(BenchSlot::First)
                ),
                WorkItem::MovementTrigger(
                    MovementStep::EnteringBench,
                    PlayerId::One,
                    Position::Bench(BenchSlot::First)
                ),
                WorkItem::MovementTrigger(
                    MovementStep::LeavingBench,
                    PlayerId::One,
                    Position::Main
                ),
                WorkItem::MovementTrigger(
                    MovementStep::EnteringMain,
                    PlayerId::One,
                    Position::Main
                ),
            ]
        );
    }

    #[test]
    fn retreat_rejects_a_pending_decision() {
        let mut state = base_state();
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

        assert_eq!(result, Err(ActionError::PendingInputMismatch));
    }

    #[test]
    fn retreat_rejects_outside_main_phase() {
        let mut state = base_state();
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
        state.turn.phase = Phase::Combat;

        let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

        assert_eq!(result, Err(ActionError::WrongPhase));
    }

    #[test]
    fn retreat_rejects_a_second_normal_retreat_this_turn() {
        let mut state = base_state();
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
        state.turn.normal_retreat_used = true;

        let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

        assert_eq!(result, Err(ActionError::NormalRetreatAlreadyUsed));
    }

    #[test]
    fn retreat_rejects_an_empty_bench_slot() {
        let state = base_state();

        let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

        assert_eq!(result, Err(ActionError::EmptyPosition));
    }

    #[test]
    fn retreat_reports_a_mana_shortfall() {
        let mut state = base_state();
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));
        // Quarry Whelp's Retreat Cost is 1, all-Generic; the bank is empty.

        let result = retreat(&state, PlayerId::One, BenchSlot::First, None);

        assert_eq!(
            result,
            Err(ActionError::InsufficientMana {
                short: ManaBank {
                    matter: 1,
                    mind: 0,
                    spirit: 0
                }
            })
        );
    }

    #[test]
    fn retreat_rejects_a_hint_naming_an_empty_pool() {
        let mut state = base_state();
        state.players.one.mana = ManaBank {
            matter: 5,
            mind: 0,
            spirit: 0,
        };
        state.players.one.bench[0] = Some(base_summon(PlayerId::One, "set-path-adept"));

        let result = retreat(
            &state,
            PlayerId::One,
            BenchSlot::First,
            Some(ManaType::Spirit),
        );

        assert_eq!(result, Err(ActionError::InvalidManaHint));
    }

    #[test]
    fn a_rejected_retreat_leaves_the_callers_state_untouched() {
        let state = base_state();
        let before = state.clone();

        let _ = retreat(&state, PlayerId::One, BenchSlot::First, None);

        assert_eq!(state, before);
    }

    // --- end to end, through the public `apply` entry point ---------------

    #[test]
    fn play_upgrade_then_retreat_chain_through_scenario_and_apply() {
        use crate::domain::actions::GameAction;
        use crate::engine::apply::apply;
        use crate::scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario};

        let scenario = Scenario {
            players: crate::domain::state::PerPlayer::new(
                ScenarioPlayer {
                    deck: vec![],
                    hand: vec![card_ref(2, "set-path-adept"), card_ref(3, "quarry-brute")],
                    prizes: vec![],
                    discard: vec![],
                    mana: ManaBank {
                        matter: 5,
                        mind: 0,
                        spirit: 0,
                    },
                    main_losses: 0,
                    has_coin: false,
                    main: Some(ScenarioSummon {
                        chain: vec![card_ref(1, "quarry-whelp")],
                        damage: 0,
                        ready: true,
                    }),
                    bench: [None, None, None],
                },
                ScenarioPlayer {
                    deck: vec![],
                    hand: vec![],
                    prizes: vec![],
                    discard: vec![],
                    mana: ManaBank::default(),
                    main_losses: 0,
                    has_coin: false,
                    main: Some(ScenarioSummon {
                        chain: vec![card_ref(101, "set-path-adept")],
                        damage: 0,
                        ready: true,
                    }),
                    bench: [None, None, None],
                },
            ),
            active_player: PlayerId::One,
        };
        let state =
            from_scenario(fixtures::card_set(), &scenario).expect("this board is a legal scenario");

        // 1. Play the Base Set-Path Adept from hand onto the empty Bench.
        let outcome = apply(
            &state,
            &GameAction::PlaySummon {
                player: PlayerId::One,
                card: CardInstanceId(2),
                slot: BenchSlot::First,
            },
        )
        .expect("the Bench slot is empty and the card is a Base Summon in hand");
        assert_eq!(
            outcome.events,
            vec![GameEvent::SummonPlayed {
                player: PlayerId::One,
                card: CardInstanceId(2),
                slot: BenchSlot::First,
            }]
        );

        // 2. Upgrade the Main Quarry Whelp with the Quarry Brute in hand.
        let outcome = apply(
            &outcome.state,
            &GameAction::UpgradeSummon {
                player: PlayerId::One,
                card: CardInstanceId(3),
                position: Position::Main,
            },
        )
        .expect("Base to Enhanced climbs with matching Mana Types and nothing upgraded yet");
        assert_eq!(
            outcome.events,
            vec![GameEvent::SummonUpgraded {
                player: PlayerId::One,
                card: CardInstanceId(3),
                position: Position::Main,
            }]
        );
        assert_eq!(
            outcome
                .state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .map(|s| s.chain.top().def),
            Some(fixtures::id("quarry-brute"))
        );

        // 3. Retreat: pay Quarry Brute's printed Retreat Cost of 2 and swap
        // Main with the newly played Set-Path Adept.
        let outcome = apply(
            &outcome.state,
            &GameAction::Retreat {
                player: PlayerId::One,
                slot: BenchSlot::First,
                mana_hint: None,
            },
        )
        .expect("the bank covers the printed Retreat Cost of 2");
        assert_eq!(
            outcome.events,
            vec![
                GameEvent::ManaDeducted {
                    player: PlayerId::One,
                    mana_type: ManaType::Matter,
                    amount: 2,
                },
                GameEvent::SummonsSwapped {
                    player: PlayerId::One,
                    main: BenchSlot::First,
                },
            ]
        );

        let one = outcome.state.players.get(PlayerId::One);
        assert_eq!(
            one.main.as_ref().map(|s| s.chain.base().def),
            Some(fixtures::id("set-path-adept"))
        );
        assert!(one.main.as_ref().is_some_and(|s| s.entered_main_this_turn));
        assert_eq!(
            one.bench[0].as_ref().map(|s| s.chain.top().def),
            Some(fixtures::id("quarry-brute"))
        );
        assert_eq!(
            one.mana,
            ManaBank {
                matter: 3,
                mind: 0,
                spirit: 0,
            }
        );
        assert!(outcome.state.turn.normal_retreat_used);
    }
}
