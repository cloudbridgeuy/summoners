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

use crate::domain::cards::{EffectLeaf, Query, QueryResult, find_def};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{ManaType, PlayerId, Position};
use crate::domain::state::{GameState, Phase, PlayerState, SummonInstance};
use crate::engine::apply::ActionOutcome;
use crate::engine::effects;
use crate::engine::payment::{self, PaymentError};

use crate::domain::actions::SkillIndex;

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
/// attack's effects.
fn apply_leaves(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    effects: &[EffectLeaf],
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();
    for leaf in effects {
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
    player_state: &PlayerState,
    leaves: &[EffectLeaf],
    targets: &[Position],
) -> Result<(), ActionError> {
    for leaf in leaves {
        match leaf {
            EffectLeaf::MoveSummon => validate_move_summon(player_state, targets)?,
            EffectLeaf::SwapPositions => validate_swap_positions(player_state, targets)?,
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

/// One `ActivateSkill` action's fields, bundled so `activate_skill` reads
/// them as one value instead of five separate arguments.
pub(crate) struct SkillActivation {
    pub player: PlayerId,
    pub position: Position,
    pub skill: SkillIndex,
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
        skill,
        targets,
        mana_hint,
    } = activation;

    require_free_main_phase(state)?;

    let player_state = state.players.get(player);
    let Some(summon) = summon_at(player_state, position) else {
        return Err(ActionError::EmptyPosition);
    };
    if !summon.ready {
        return Err(ActionError::SummonExhausted);
    }

    let Some(top_def) = find_def(summon.chain.top().def) else {
        return Err(ActionError::UnknownCard);
    };
    // No printed Skill at this index reads the same as any other illegal
    // target named on this action: `InvalidTarget` already covers "the
    // named target position is not legal for this action" for occupied
    // Bench slots (`play_summon`) and empty ones (`retreat`); an
    // out-of-range `SkillIndex` is the same shape of mistake, naming
    // something this Summon does not actually have.
    let Some(QueryResult::Skill { cost, effects }) = top_def.find(Query::Skill(skill)) else {
        return Err(ActionError::InvalidTarget);
    };

    validate_targets(player_state, &effects, &targets)?;

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
        summon_mut.ready = false;
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
        skill,
    });

    let (next, leaf_events) = apply_leaves(&next, player, &targets, &effects);
    events.extend(leaf_events);

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::actions::GameAction;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{BenchSlot, CardInstanceId};
    use crate::domain::state::{
        CardRef, ManaBank, ManaSource, MovementStep, PerPlayer, TurnState, UpgradeChain, WorkItem,
    };
    use std::collections::VecDeque;

    fn summon(owner: PlayerId, def: &'static str, ready: bool) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(1),
                    def: CardDefId(def),
                },
                vec![],
            ),
            damage: 0,
            ready,
            owner,
            controller: owner,
            duration_markers: vec![],
            played_this_turn: false,
            upgraded_this_turn: false,
            entered_main_this_turn: false,
        }
    }

    fn empty_player(owner: PlayerId, def: &'static str, ready: bool) -> PlayerState {
        PlayerState {
            main: Some(summon(owner, def, ready)),
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

    fn base_state(def: &'static str, ready: bool) -> GameState {
        GameState {
            players: PerPlayer::new(
                empty_player(PlayerId::One, def, ready),
                empty_player(PlayerId::Two, "quarry-whelp", true),
            ),
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

    // --- Ready vs Exhausted gating --------------------------------------

    #[test]
    fn an_exhausted_summon_rejects_activation() {
        let mut state = base_state("quarry-scout", false);
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::SummonExhausted));
    }

    #[test]
    fn a_missing_summon_rejects_activation() {
        let mut state = base_state("quarry-scout", true);
        state.players.get_mut(PlayerId::One).main = None;

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::EmptyPosition));
    }

    #[test]
    fn an_out_of_range_skill_index_is_an_invalid_target() {
        let state = base_state("quarry-scout", true);

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(1),
                targets: vec![],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    #[test]
    fn a_summon_with_no_skill_nodes_is_an_invalid_target() {
        let state = base_state("quarry-whelp", true);

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    // --- exhaustion ordering ---------------------------------------------

    #[test]
    fn exhaustion_happens_before_the_skills_effects_resolve() {
        // Rules §15's fixed order pays the cost and exhausts the Summon
        // before its Skill resolves. `MoveSummon` moving the activating
        // Summon itself makes that order observable: if exhaustion ran
        // after the move, the mutation would land on the now-empty
        // originating slot and silently miss, leaving the moved Summon
        // still Ready.
        let mut state = base_state("quarry-whelp", true);
        state.players.get_mut(PlayerId::One).bench[0] =
            Some(summon(PlayerId::One, "quarry-scout", true));
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let outcome = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                skill: SkillIndex(0),
                targets: vec![
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Second),
                ],
                mana_hint: None,
            },
        )
        .expect("Quarry Scout is Ready and the Generic cost is covered");

        let moved = outcome.state.players.get(PlayerId::One).bench[1]
            .as_ref()
            .expect("the Scout relocated to the second Bench slot");
        assert!(
            !moved.ready,
            "exhaustion applied to the original slot before the move ran"
        );
        assert!(outcome.state.players.get(PlayerId::One).bench[0].is_none());
    }

    // --- movement leaves against occupied and empty positions ------------

    #[test]
    fn move_summon_onto_an_occupied_bench_slot_is_rejected_before_payment() {
        let mut state = base_state("quarry-whelp", true);
        state.players.get_mut(PlayerId::One).bench[0] =
            Some(summon(PlayerId::One, "quarry-scout", true));
        state.players.get_mut(PlayerId::One).bench[1] =
            Some(summon(PlayerId::One, "quarry-whelp", true));
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                skill: SkillIndex(0),
                targets: vec![
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Second),
                ],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    #[test]
    fn move_summon_from_an_empty_bench_slot_is_rejected() {
        let mut state = base_state("quarry-whelp", true);
        state.players.get_mut(PlayerId::One).bench[0] =
            Some(summon(PlayerId::One, "quarry-scout", true));
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                skill: SkillIndex(0),
                targets: vec![
                    Position::Bench(BenchSlot::Second),
                    Position::Bench(BenchSlot::Third),
                ],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    #[test]
    fn swap_positions_against_an_empty_bench_slot_is_rejected() {
        let state = base_state("quarry-warden-guard", true);

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![Position::Bench(BenchSlot::First)],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    // --- payment and event batches ----------------------------------------

    #[test]
    fn activate_skill_pays_a_nonzero_cost_before_skill_activated_and_the_move_leaf_emits_no_event()
    {
        let mut state = base_state("quarry-whelp", true);
        state.players.get_mut(PlayerId::One).bench[0] =
            Some(summon(PlayerId::One, "quarry-scout", true));
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let outcome = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                skill: SkillIndex(0),
                targets: vec![
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Second),
                ],
                mana_hint: None,
            },
        )
        .expect("Quarry Scout is Ready and the Generic cost is covered");

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::ManaDeducted {
                    player: PlayerId::One,
                    mana_type: ManaType::Matter,
                    amount: 1,
                },
                GameEvent::SkillActivated {
                    player: PlayerId::One,
                    position: Position::Bench(BenchSlot::First),
                    skill: SkillIndex(0),
                },
            ],
            "MoveSummon itself emits no event"
        );
        assert_eq!(
            outcome.state.work,
            VecDeque::from(vec![
                WorkItem::MovementTrigger(
                    MovementStep::LeavingBench,
                    Position::Bench(BenchSlot::First)
                ),
                WorkItem::MovementTrigger(
                    MovementStep::EnteringBench,
                    Position::Bench(BenchSlot::Second)
                ),
            ])
        );
    }

    #[test]
    fn activate_skill_with_a_free_cost_skips_mana_deducted_and_emits_the_leafs_event() {
        let state = base_state("quarry-well-tender", true);

        let outcome = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![Position::Main],
                mana_hint: None,
            },
        )
        .expect("Quarry Well-Tender's Skill is free");

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::SkillActivated {
                    player: PlayerId::One,
                    position: Position::Main,
                    skill: SkillIndex(0),
                },
                GameEvent::ManaProduced {
                    player: PlayerId::One,
                    source: ManaSource::Summon(Position::Main),
                    mana_type: ManaType::Matter,
                },
            ]
        );
    }

    #[test]
    fn activate_skill_reports_a_mana_shortfall() {
        let mut state = base_state("quarry-whelp", true);
        state.players.get_mut(PlayerId::One).bench[0] =
            Some(summon(PlayerId::One, "quarry-scout", true));

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                skill: SkillIndex(0),
                targets: vec![
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Second),
                ],
                mana_hint: None,
            },
        );

        assert!(matches!(result, Err(ActionError::InsufficientMana { .. })));
    }

    #[test]
    fn activate_skill_rejects_a_hint_naming_an_empty_pool() {
        // The binding mana_hint rule: a hint naming an empty pool rejects
        // with InvalidManaHint rather than falling back to another pool,
        // the same as `declare_attack` and `cast_spell`.
        let mut state = base_state("quarry-whelp", true);
        state.players.get_mut(PlayerId::One).bench[0] =
            Some(summon(PlayerId::One, "quarry-scout", true));
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let result = activate_skill(
            &state,
            SkillActivation {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                skill: SkillIndex(0),
                targets: vec![
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Second),
                ],
                mana_hint: Some(ManaType::Spirit),
            },
        );

        assert_eq!(result, Err(ActionError::InvalidManaHint));
    }

    // --- end-to-end acceptance through the real `apply` entry point -------

    #[test]
    fn apply_activates_a_ready_summons_skill_pays_exhausts_and_resolves_its_effects_end_to_end() {
        // Acceptance: a Ready Summon activates a Skill through the real
        // `apply` entry point — its cost is paid, it becomes Exhausted,
        // `SkillActivated` is emitted, and its effect resolves (rules
        // §15, §43).
        let state = base_state("quarry-well-tender", true);

        let outcome = crate::engine::apply::apply(
            &state,
            &GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![Position::Main],
                mana_hint: None,
            },
        )
        .expect("a Ready Summon may activate its own free Skill");

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::SkillActivated {
                    player: PlayerId::One,
                    position: Position::Main,
                    skill: SkillIndex(0),
                },
                GameEvent::ManaProduced {
                    player: PlayerId::One,
                    source: ManaSource::Summon(Position::Main),
                    mana_type: ManaType::Matter,
                },
            ]
        );
        assert!(
            !outcome
                .state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .ready
        );
        assert_eq!(outcome.state.players.get(PlayerId::One).mana.matter, 1);
    }

    #[test]
    fn apply_rejects_activation_from_an_exhausted_summon() {
        let state = base_state("quarry-well-tender", false);

        let result = crate::engine::apply::apply(
            &state,
            &GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![Position::Main],
                mana_hint: None,
            },
        );

        assert_eq!(result, Err(ActionError::SummonExhausted));
    }

    #[test]
    fn apply_lets_the_ready_effect_spell_reenable_a_skill_activation() {
        // Rules §53: "If a Summon activates a Skill, becomes Exhausted,
        // and is later Readied by an effect, it may activate another
        // Skill." Second Wind is this crate's Ready-effect Spell fixture.
        let mut state = base_state("quarry-well-tender", false);
        state.players.get_mut(PlayerId::One).hand = vec![CardRef {
            instance: CardInstanceId(50),
            def: CardDefId("second-wind"),
        }];
        state.players.get_mut(PlayerId::One).mana.matter = 1;

        let cast = crate::engine::apply::apply(
            &state,
            &GameAction::CastSpell {
                player: PlayerId::One,
                card: CardInstanceId(50),
                targets: vec![Position::Main],
                mana_hint: None,
            },
        )
        .expect("Second Wind is affordable and legal in the caster's own Main");

        let defender_pass = crate::engine::apply::apply(
            &cast.state,
            &GameAction::PassPriority {
                player: PlayerId::Two,
            },
        )
        .expect("Two holds Priority after the cast opens a window");

        let caster_pass = crate::engine::apply::apply(
            &defender_pass.state,
            &GameAction::PassPriority {
                player: PlayerId::One,
            },
        )
        .expect("One's second consecutive pass closes the window and resolves the Spell");

        assert!(
            caster_pass
                .state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .ready,
            "Second Wind's ReadySummon leaf turned the Exhausted Summon Ready again"
        );

        let reactivation = crate::engine::apply::apply(
            &caster_pass.state,
            &GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
                skill: SkillIndex(0),
                targets: vec![Position::Main],
                mana_hint: None,
            },
        )
        .expect("rules §53: a Summon Readied by an effect may activate another Skill");

        assert!(
            !reactivation
                .state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .ready
        );
    }
}
