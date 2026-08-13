use super::*;
use crate::domain::cards::fixtures;
use crate::domain::ids::{BenchSlot, CardInstanceId, ManaType, Position};
use crate::domain::state::{
    CardRef, GameOutcome, GameStatus, LossReason, ManaBank, ManaSource, PerPlayer, Phase,
    PlayerState, StackItem, StackWindow, SummonInstance, TurnState, UpgradeChain,
};
use std::collections::VecDeque;

fn summon(owner: PlayerId) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def: fixtures::id("quarry-whelp"),
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
        status: GameStatus::Playing,
        cards: fixtures::card_set(),
    }
}

fn end_turn(player: PlayerId) -> GameAction {
    GameAction::EndTurn { player }
}

fn card_ref(instance: u32, def: &'static str) -> CardRef {
    CardRef {
        instance: CardInstanceId(instance),
        def: fixtures::id(def),
    }
}

/// Run the full `EndTurn` sequence through `apply` — opening the §47
/// window, the defender passing first, then the active player passing
/// second — as three separate submitted actions, and report the
/// combined ordered event batch across all three the way a caller
/// watching the whole exchange would see it.
fn full_end_turn(state: &GameState, player: PlayerId) -> Result<ActionOutcome, ActionError> {
    let opened = apply(state, &end_turn(player))?;
    let defender = player.opponent();
    let after_defender_pass = apply(
        &opened.state,
        &GameAction::PassPriority { player: defender },
    )?;
    let last = apply(
        &after_defender_pass.state,
        &GameAction::PassPriority { player },
    )?;

    let mut events = opened.events;
    events.extend(after_defender_pass.events);
    events.extend(last.events);

    Ok(ActionOutcome {
        state: last.state,
        events,
    })
}

#[test]
fn a_finished_game_rejects_every_action_first() {
    let mut state = base_state();
    state.status = GameStatus::Ended(GameOutcome {
        winner: PlayerId::One,
        reason: LossReason::ThirdMainLoss,
    });

    // The active player would otherwise be the legal actor; the wrong
    // player is used here to prove GameAlreadyOver wins even over a
    // mismatched actor (it must not read as NotYourDecision).
    let result = apply(&state, &end_turn(PlayerId::Two));

    assert_eq!(result, Err(ActionError::GameAlreadyOver));
}

#[test]
fn a_broken_game_rejects_every_action_first() {
    let mut state = base_state();
    state.status = GameStatus::Broken(crate::domain::cards::Breakage {
        rule: "destruction",
        entity: fixtures::id("quarry-whelp"),
        expected: crate::domain::cards::ComponentKind::Life,
    });

    // Same proof as the finished-game case just above: the wrong player is
    // used here to prove GameBroken wins even over a mismatched actor.
    let result = apply(&state, &end_turn(PlayerId::Two));

    assert_eq!(result, Err(ActionError::GameBroken));
}

#[test]
fn an_action_from_the_wrong_player_is_rejected() {
    let state = base_state();

    let result = apply(&state, &end_turn(PlayerId::Two));

    assert_eq!(result, Err(ActionError::NotYourDecision));
}

#[test]
fn a_pending_decision_names_the_only_legal_actor() {
    let mut state = base_state();
    state.turn.active_player = PlayerId::Two;
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::Two,
        prior_pass: false,
    });
    state.pending = Some(PendingInput::Promotion {
        player: PlayerId::One,
    });

    // The actor gate lets `PlayerId::One` (the named pending answerer)
    // reach `end_turn`'s own handler, which then rejects it on its own
    // terms: `EndTurn` is not a decision answer, so it is illegal while
    // one is pending (`WrongPhase`), never a bare `NotYourDecision`.
    assert!(matches!(
        apply(&state, &end_turn(PlayerId::One)),
        Err(ActionError::WrongPhase)
    ));
    assert_eq!(
        apply(&state, &end_turn(PlayerId::Two)),
        Err(ActionError::NotYourDecision)
    );
}

#[test]
fn an_open_priority_window_beats_the_active_player() {
    let mut state = base_state();
    state.turn.active_player = PlayerId::Two;
    state.turn.phase = Phase::Combat;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });

    // The actor gate lets the window holder through; `end_turn` then
    // rejects it because a window is open (`WrongPhase`), not because
    // the actor gate stopped it.
    assert!(matches!(
        apply(&state, &end_turn(PlayerId::One)),
        Err(ActionError::WrongPhase)
    ));
    assert_eq!(
        apply(&state, &end_turn(PlayerId::Two)),
        Err(ActionError::NotYourDecision)
    );
}

#[test]
fn an_open_priority_window_beats_the_active_player_outside_combat_too() {
    // A window can open in Main (a proactive Spell cast) or Upkeep (a
    // respondable trigger mid-resolution), not only in Combat. The
    // window must win regardless of which phase is resting.
    let mut state = base_state();
    state.turn.active_player = PlayerId::Two;
    state.turn.phase = Phase::Main;
    state.turn.window = Some(StackWindow {
        holder: PlayerId::One,
        prior_pass: false,
    });

    assert!(matches!(
        apply(&state, &end_turn(PlayerId::One)),
        Err(ActionError::WrongPhase)
    ));
    assert_eq!(
        apply(&state, &end_turn(PlayerId::Two)),
        Err(ActionError::NotYourDecision)
    );
}

#[test]
fn with_no_pending_and_no_window_only_the_active_player_may_act() {
    // `ActivateSkill` now has a real handler in `engine::skills`; this
    // isolates the actor-gate boundary from that handler's own rule
    // checks by using a target the actor gate lets through but the
    // handler itself must still reject on its own terms (this fixture's
    // quarry-whelp prints no Skill).
    let state = base_state();
    let activate_skill = GameAction::ActivateSkill {
        player: PlayerId::One,
        position: Position::Main,
        skill: crate::domain::actions::SkillIndex(0),
        targets: vec![],
        mana_hint: None,
    };

    assert!(matches!(
        apply(&state, &activate_skill),
        Err(ActionError::InvalidTarget)
    ));
}

mod broken_game;
mod demo;
mod demo_triggers;
mod scenario_probes;

#[test]
fn play_summon_upgrade_summon_and_retreat_reach_their_own_handlers() {
    // Confirms dispatch actually routes to `engine::board` now, without
    // duplicating that module's own coverage of its rules.
    let state = base_state();

    assert_eq!(
        apply(
            &state,
            &GameAction::PlaySummon {
                player: PlayerId::One,
                card: CardInstanceId(1),
                slot: BenchSlot::First,
            },
        ),
        Err(ActionError::UnknownCard)
    );
    assert_eq!(
        apply(
            &state,
            &GameAction::UpgradeSummon {
                player: PlayerId::One,
                card: CardInstanceId(1),
                position: crate::domain::ids::Position::Main,
            },
        ),
        Err(ActionError::UnknownCard)
    );
    assert_eq!(
        apply(
            &state,
            &GameAction::Retreat {
                player: PlayerId::One,
                slot: BenchSlot::First,
                mana_hint: None,
            },
        ),
        Err(ActionError::EmptyPosition)
    );
}

#[test]
fn a_rejected_action_leaves_the_caller_free_to_reuse_its_state() {
    let state = base_state();
    let before = state.clone();

    let _ = apply(&state, &end_turn(PlayerId::Two));

    assert_eq!(state, before);
}
