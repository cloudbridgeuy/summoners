//! The resolution loop's full three-step contract: once a Priority window
//! has closed, resolve whatever is left on the current Stack segment
//! strictly top-first (step 1, rules §35) — a segment's own item(s) must
//! finish before anything queued underneath it resumes (rules §38, §40) —
//! then drain `state.work`, including whatever the segment's resolution
//! just fed back into it (step 2), and finally rest once both are settled
//! (step 3). A respondable trigger opening a new window mid-drain (rules
//! §38) pauses the whole loop immediately, leaving the rest of `work` — and
//! the segment base the trigger just pushed — exactly where they are until
//! that window closes.

use crate::domain::cards::{CardDefId, EffectLeaf, Query, QueryResult, find_def};
use crate::domain::events::GameEvent;
use crate::domain::ids::{PlayerId, Position};
use crate::domain::state::{CardRef, GameState, StackItem, WorkItem};
use crate::engine::{destruction, effects, loss, triggers, upkeep};

/// Drain `state.work`, then the Stack, until both are settled, a decision
/// pauses the loop (`pending` becomes set), or the game ends (`outcome`
/// becomes set — rules §2: losing is immediate, so nothing queued after
/// that point runs). Returns the resulting state and every event produced
/// along the way, in order.
pub(crate) fn drain(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();

    loop {
        if state.pending.is_some() || state.outcome.is_some() {
            break;
        }

        // A respondable trigger can open a Priority window mid-drain
        // (rules §38): once that happens, the rest of `work` — including
        // any `FireTrigger` items discovery queued behind it — must wait
        // for the window to close, the same way step 2 already waits
        // before touching the Stack.
        if state.turn.window.is_some() {
            break;
        }

        // Step 1: the window is closed (the loop above already broke out
        // otherwise), so if the current Stack segment still holds
        // something, resolve its top item first (rules §35 — strictly
        // top-first). A segment interrupts whatever `work` was doing to
        // open its own window (rules §38); once that window closes, the
        // segment's own item(s) must finish — and its base must come off
        // `stack_segment_bases` — before the `work` items it interrupted
        // resume underneath it (rules §40). Whatever this feeds back into
        // `work` is picked up by step 2 on a later iteration, once this
        // segment (and any it is nested in) is fully settled.
        if stack_has_unresolved_items(&state) {
            let (next_state, item_events) = resolve_top_stack_item(&state);
            state = next_state;
            events.extend(item_events);
            continue;
        }

        // Step 2: the current Stack segment is settled. Drain `work`.
        if let Some(item) = state.work.pop_front() {
            let (next_state, item_events) = execute(&state, &item);
            state = next_state;
            events.extend(item_events);
            continue;
        }

        // Step 3: both the current Stack segment and the work queue are
        // settled — rest here until the next action.
        break;
    }

    (state, events)
}

/// Whether the current Stack segment still holds an item to resolve. With
/// no segment open, the base defaults to 0 and this is the whole Stack;
/// with one open (rules §38 — a respondable trigger, and anything played
/// in response to it), it is only the part above that segment's base, so
/// an outer, interrupted segment cannot be touched until this one empties
/// back down to where it started.
fn stack_has_unresolved_items(state: &GameState) -> bool {
    let base = state.stack_segment_bases.last().copied().unwrap_or(0);
    state.stack.len() > base
}

/// Run one `WorkItem`, returning the resulting state and the events it
/// produced.
fn execute(state: &GameState, item: &WorkItem) -> (GameState, Vec<GameEvent>) {
    match item {
        WorkItem::ReadyAll => upkeep::ready_all(state),
        WorkItem::DrawCard => execute_draw(state),
        WorkItem::ProduceMana(source) => upkeep::produce_mana(state, *source),

        WorkItem::DestructionCheck(position) => destruction::check(state, *position),
        WorkItem::DiscardDestroyedChain(position) => {
            destruction::discard_destroyed_chain(state, *position)
        }
        WorkItem::RecordMainLoss(player) => destruction::record_main_loss(state, *player),
        WorkItem::RecoverPrize(player) => destruction::recover_prize(state, *player),
        WorkItem::PromoteBenchSummon(player) => destruction::promote_bench_summon(state, *player),
        WorkItem::ResolveMovementConsequences(player) => {
            destruction::resolve_movement_consequences(state, *player)
        }

        WorkItem::MovementTrigger(step, player, position) => {
            triggers::movement_trigger(state, *step, *player, *position)
        }
        WorkItem::FireTrigger(player, position, event) => {
            triggers::fire_queued(state, *player, *position, *event)
        }

        WorkItem::LossCheck(player) => loss::check(state, *player),
    }
}

/// `WorkItem::DrawCard`: draw for the active player, and treat an empty
/// Deck as the immediate loss it is (rules §2, §10 step 2, §58).
fn execute_draw(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let (state, mut events, outcome) = upkeep::draw_card(state);
    match outcome {
        upkeep::DrawOutcome::Drew => (state, events),
        upkeep::DrawOutcome::DeckEmpty => {
            let player = state.turn.active_player;
            let (state, loss_events) = loss::draw_failure(&state, player);
            events.extend(loss_events);
            (state, events)
        }
    }
}

/// Resolve the item on top of the Stack (rules §35: the Stack always
/// resolves top-first), through the shared `engine::effects` interpreter.
fn resolve_top_stack_item(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let Some(item) = state.stack.pop() else {
        return (state, Vec::new());
    };

    let mut events = vec![GameEvent::StackItemResolved { item: item.clone() }];

    match item {
        StackItem::Attack { attacker, target } => {
            let (next_state, leaf_events) = resolve_attack(&state, attacker, target);
            state = next_state;
            events.extend(leaf_events);
        }
        StackItem::Spell {
            caster,
            card,
            targets,
        } => {
            let (next_state, leaf_events) = resolve_spell(&state, caster, card, &targets);
            state = next_state;
            events.extend(leaf_events);
        }
        StackItem::Trigger {
            controller,
            targets,
            effects,
            ..
        } => {
            let (next_state, leaf_events) = apply_leaves(&state, controller, &targets, &effects);
            state = next_state;
            events.extend(leaf_events);
        }
    }

    // Rules §38, §40: once popping that item drains the Stack back down to
    // the current segment's own base, the segment is settled — drop the
    // base so the next iteration's `stack_has_unresolved_items` reads
    // whatever segment (or the implicit one at 0) sits below it, letting
    // the `work` that segment interrupted resume underneath it.
    if state.stack_segment_bases.last() == Some(&state.stack.len()) {
        state.stack_segment_bases.pop();
    }

    (state, events)
}

/// Apply an attack's printed effects against `target`, re-reading the
/// attacker's Attack node at resolution time rather than trusting whatever
/// was printed when the attack was declared, since `StackItem::Attack`
/// carries no effects field of its own (rules §30).
fn resolve_attack(
    state: &GameState,
    attacker: PlayerId,
    target: Position,
) -> (GameState, Vec<GameEvent>) {
    apply_leaves(
        state,
        attacker,
        &[target],
        &attacker_effects(state, attacker),
    )
}

/// Apply a Spell's printed effects against `targets`, then move the Spell
/// to its caster's discard pile — resolved Spells go to their Owner's
/// discard the same way a destroyed upgrade chain does (rules §56).
fn resolve_spell(
    state: &GameState,
    caster: PlayerId,
    card: CardRef,
    targets: &[Position],
) -> (GameState, Vec<GameEvent>) {
    let (mut state, events) = apply_leaves(state, caster, targets, &spell_effects(card.def));
    state.players.get_mut(caster).discard.push(card);
    (state, events)
}

/// Run every effect leaf in order through the shared interpreter, folding
/// its state and events forward. `pub(crate)` so `engine::triggers` can
/// resolve an immediate trigger's effects through the same single
/// interpreter path as an attack, a Spell, and a respondable trigger's own
/// Stack item.
pub(crate) fn apply_leaves(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    leaves: &[EffectLeaf],
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let mut events = Vec::new();
    for leaf in leaves {
        let (next_state, leaf_events) = effects::apply_leaf(&state, controller, targets, leaf);
        state = next_state;
        events.extend(leaf_events);
    }
    (state, events)
}

/// The attacker's currently printed Attack effects.
fn attacker_effects(state: &GameState, attacker: PlayerId) -> Vec<EffectLeaf> {
    let Some(main_summon) = &state.players.get(attacker).main else {
        return Vec::new();
    };
    let Some(def) = find_def(main_summon.chain.top().def) else {
        return Vec::new();
    };
    match def.find(Query::Attack) {
        Some(QueryResult::Attack { effects, .. }) => effects,
        _ => Vec::new(),
    }
}

/// A Spell's printed effects, read off its card definition.
fn spell_effects(def_id: CardDefId) -> Vec<EffectLeaf> {
    let Some(def) = find_def(def_id) else {
        return Vec::new();
    };
    match def.find(Query::Spell) {
        Some(QueryResult::Spell { effects, .. }) => effects,
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{CardInstanceId, PlayerId, Position};
    use crate::domain::state::{
        CardRef, ManaBank, ManaSource, MovementStep, PendingInput, PerPlayer, Phase, PlayerState,
        SummonInstance, TurnState, UpgradeChain,
    };
    use std::collections::VecDeque;

    fn whelp(owner: PlayerId) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(1),
                    def: CardDefId("quarry-whelp"),
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

    fn player_state(owner: PlayerId) -> PlayerState {
        PlayerState {
            main: Some(whelp(owner)),
            bench: [None, None, None],
            deck: vec![CardRef {
                instance: CardInstanceId(10),
                def: CardDefId("quarry-whelp"),
            }],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            has_coin: false,
        }
    }

    fn base_state() -> GameState {
        GameState {
            players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
            turn: TurnState {
                active_player: PlayerId::Two,
                phase: Phase::Upkeep,
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

    #[test]
    fn drain_runs_the_full_upkeep_sequence_in_order() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ]);

        let (state, events) = drain(&state);

        assert!(state.work.is_empty());
        assert_eq!(
            events,
            vec![
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::CardDrawn {
                    player: PlayerId::Two,
                    card: CardInstanceId(10),
                },
                GameEvent::ManaProduced {
                    player: PlayerId::Two,
                    source: ManaSource::Player,
                    mana_type: crate::domain::ids::ManaType::Matter,
                },
            ]
        );
    }

    #[test]
    fn drain_stops_as_soon_as_produce_mana_pauses_on_a_choice() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(2),
                    def: CardDefId("set-path-adept"),
                },
                vec![],
            ),
            ..whelp(PlayerId::Two)
        });
        state.work = VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ]);

        let (state, events) = drain(&state);

        assert_eq!(
            state.pending,
            Some(PendingInput::ManaProduction {
                player: PlayerId::Two,
                source: ManaSource::Player,
            })
        );
        assert!(state.work.is_empty());
        assert_eq!(
            events.len(),
            2,
            "ReadyAll and DrawCard ran; production paused before its own event"
        );
    }

    #[test]
    fn drain_stops_immediately_once_a_draw_failure_sets_outcome() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).deck = vec![];
        state.work = VecDeque::from(vec![
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ]);

        let (state, events) = drain(&state);

        assert!(state.outcome.is_some());
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::ProduceMana(ManaSource::Player)]),
            "the loop checks outcome before popping the next item, so the \
             unrun item is left queued rather than executed — harmless, since \
             a finished game rejects every later action outright"
        );
        assert_eq!(
            events,
            vec![
                GameEvent::SummonsReadied {
                    player: PlayerId::Two,
                    positions: vec![Position::Main],
                },
                GameEvent::GameEnded {
                    winner: PlayerId::One,
                    reason: crate::domain::state::LossReason::EmptyDeckDraw,
                },
            ],
            "CardDrawn never fires and ManaProduced never runs after the loss"
        );
    }

    #[test]
    fn drain_leaves_a_finished_game_untouched_even_with_queued_work() {
        let mut state = base_state();
        state.outcome = Some(crate::domain::state::GameOutcome {
            winner: PlayerId::One,
            reason: crate::domain::state::LossReason::EmptyDeckDraw,
        });
        state.work = VecDeque::from(vec![WorkItem::ReadyAll]);

        let (state, events) = drain(&state);

        assert!(events.is_empty());
        assert_eq!(state.work, VecDeque::from(vec![WorkItem::ReadyAll]));
    }

    #[test]
    fn drain_is_a_silent_no_op_for_a_movement_or_ability_trigger_with_no_matching_card() {
        // Quarry Whelp carries no `CardNode::Trigger`, so both items find
        // nothing to fire; `LeavingMain` also does not touch
        // `entered_main_this_turn` (only `EnteringMain` does).
        let mut state = base_state();
        state.work = VecDeque::from(vec![
            WorkItem::MovementTrigger(MovementStep::LeavingMain, PlayerId::Two, Position::Main),
            WorkItem::FireTrigger(
                PlayerId::Two,
                Position::Main,
                crate::domain::cards::TriggerEvent::YourUpkeep,
            ),
        ]);

        let (state, events) = drain(&state);

        assert!(events.is_empty());
        assert!(state.work.is_empty());
        assert_eq!(state.outcome, None);
    }

    #[test]
    fn drain_fires_an_entering_main_trigger_and_sets_the_entered_flag() {
        // Rules §28, §36, §39: Hearth Warden's immediate trigger heals
        // itself the moment it enters Main.
        let mut state = base_state();
        state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(2),
                    def: CardDefId("hearth-warden"),
                },
                vec![],
            ),
            damage: 20,
            ..whelp(PlayerId::Two)
        });
        state.work = VecDeque::from(vec![WorkItem::MovementTrigger(
            MovementStep::EnteringMain,
            PlayerId::Two,
            Position::Main,
        )]);

        let (state, events) = drain(&state);

        assert_eq!(
            events,
            vec![
                GameEvent::TriggerFired {
                    controller: PlayerId::Two,
                    position: Position::Main,
                    event: crate::domain::cards::TriggerEvent::EntersMain,
                },
                GameEvent::Healed {
                    position: Position::Main,
                    amount: 15,
                },
            ]
        );
        let healed = state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("main");
        assert_eq!(healed.damage, 5);
        assert!(healed.entered_main_this_turn);
    }

    #[test]
    fn drain_opens_a_window_for_a_respondable_trigger_and_resumes_the_interrupted_drain_below_it() {
        // Rules §38, §40: a respondable destruction trigger creates a
        // segment base and opens a window for the opponent of its
        // controller; the drain stops there instead of running the rest
        // of `work` or touching the Stack.
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(3),
                    def: CardDefId("spite-thorn"),
                },
                vec![],
            ),
            ..whelp(PlayerId::One)
        });
        state.work = VecDeque::from(vec![
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            ),
            WorkItem::LossCheck(PlayerId::Two),
        ]);

        let (state, events) = drain(&state);

        assert_eq!(
            events,
            vec![GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            }]
        );
        assert_eq!(state.stack.len(), 1);
        assert_eq!(state.stack_segment_bases, vec![0]);
        assert_eq!(
            state.turn.window,
            Some(crate::domain::state::StackWindow {
                holder: PlayerId::Two,
                prior_pass: false,
            })
        );
        assert_eq!(
            state.work,
            VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)]),
            "the interrupted item stays queued below the open segment"
        );
    }

    #[test]
    fn drain_resumes_the_interrupted_work_once_two_passes_close_the_triggers_window() {
        // Rules §38, §40: once the opponent of the trigger's controller and
        // then the controller both pass, the segment the trigger opened
        // resolves — top-first, like any other Stack item — and only then
        // does the destruction chain it interrupted continue underneath it.
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(3),
                    def: CardDefId("spite-thorn"),
                },
                vec![],
            ),
            ..whelp(PlayerId::One)
        });
        state.work = VecDeque::from(vec![
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            ),
            WorkItem::LossCheck(PlayerId::Two),
        ]);

        let (state, _opening_events) = drain(&state);
        // The window opened for the opponent of the trigger's controller
        // (rules §38); both must pass in a row to close it (rules §32–33).
        let after_first_pass = crate::engine::stack::pass(&state, PlayerId::Two)
            .expect("Two holds Priority first, opposite the trigger's controller");
        let after_second_pass = crate::engine::stack::pass(&after_first_pass.state, PlayerId::One)
            .expect("One passes second, closing the window");
        assert_eq!(
            after_second_pass.state.turn.window, None,
            "the window is closed, but the Stack still holds the Trigger \
             item, so no handover runs yet"
        );

        let (state, events) = drain(&after_second_pass.state);

        assert_eq!(
            events,
            vec![
                GameEvent::StackItemResolved {
                    item: StackItem::Trigger {
                        controller: PlayerId::One,
                        source: Position::Main,
                        event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
                        targets: vec![Position::Main],
                        effects: vec![crate::domain::cards::EffectLeaf::DealDamage {
                            amount: 15,
                            immutable: false,
                        }],
                    },
                },
                GameEvent::DamageApplied {
                    position: Position::Main,
                    before: 0,
                    after: 15,
                },
            ],
            "the segment's own Trigger item resolved, dealing its Damage to \
             Two's Main"
        );
        assert!(
            state.stack.is_empty(),
            "the segment's item is the only thing on the Stack"
        );
        assert!(
            state.stack_segment_bases.is_empty(),
            "the segment's base came off once its item resolved"
        );
        assert!(
            state.work.is_empty(),
            "the interrupted LossCheck, and the DestructionCheck the \
             Trigger's Damage enqueued, both ran once the segment settled"
        );
        assert_eq!(
            state
                .players
                .get(PlayerId::Two)
                .main
                .as_ref()
                .expect("main")
                .damage,
            15,
            "15 Damage lands, short of Quarry Whelp's 40 Life, so nothing \
             is destroyed and the chain stays quiet"
        );
    }

    #[test]
    fn drain_runs_a_loss_check_that_finds_no_losing_condition_as_a_silent_no_op() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![WorkItem::LossCheck(PlayerId::One)]);

        let (state, events) = drain(&state);

        assert!(events.is_empty());
        assert!(state.work.is_empty());
        assert_eq!(state.outcome, None);
    }

    // -- Stack resolution: step 2 of the loop ---------------------------------

    #[test]
    fn drain_leaves_the_stack_untouched_while_a_window_is_still_open() {
        let mut state = base_state();
        state.turn.window = Some(crate::domain::state::StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });
        state.stack = vec![StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        }];

        let (state, events) = drain(&state);

        assert!(events.is_empty());
        assert_eq!(
            state.stack.len(),
            1,
            "step 2 only runs once the window has closed"
        );
    }

    #[test]
    fn drain_resolves_the_stack_strictly_top_first_once_work_is_empty_and_the_window_is_closed() {
        let mut state = base_state();
        state.turn.window = None;
        state.stack = vec![
            StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            },
            StackItem::Attack {
                attacker: PlayerId::Two,
                target: Position::Main,
            },
        ];

        let (state, events) = drain(&state);

        assert!(state.stack.is_empty());
        assert!(
            state.work.is_empty(),
            "every DestructionCheck a resolution enqueued was drained too"
        );
        // Quarry Whelp deals 10; the item pushed last (Two attacking One) is
        // the top of the Stack and resolves first (rules §35).
        assert_eq!(
            events,
            vec![
                GameEvent::StackItemResolved {
                    item: StackItem::Attack {
                        attacker: PlayerId::Two,
                        target: Position::Main,
                    },
                },
                GameEvent::DamageApplied {
                    position: Position::Main,
                    before: 0,
                    after: 10,
                },
                GameEvent::StackItemResolved {
                    item: StackItem::Attack {
                        attacker: PlayerId::One,
                        target: Position::Main,
                    },
                },
                GameEvent::DamageApplied {
                    position: Position::Main,
                    before: 0,
                    after: 10,
                },
            ]
        );
    }

    #[test]
    fn drain_resolves_an_attack_on_an_empty_bench_position_as_a_miss() {
        let mut state = base_state();
        state.turn.window = None;
        state.stack = vec![StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Bench(crate::domain::ids::BenchSlot::First),
        }];

        let (state, events) = drain(&state);

        assert!(state.stack.is_empty());
        assert_eq!(
            events,
            vec![GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Bench(crate::domain::ids::BenchSlot::First),
                },
            }],
            "a miss still resolves the Stack item, but applies no Damage"
        );
        assert!(
            state.work.is_empty(),
            "nothing to check for destruction on an empty position"
        );
    }

    #[test]
    fn drain_resolves_a_support_spell_and_discards_it_to_its_casters_pile() {
        let mut state = base_state();
        state.turn.window = None;
        state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
            damage: 15,
            ..whelp(PlayerId::One)
        });
        let card = CardRef {
            instance: CardInstanceId(99),
            def: CardDefId("renewing-balm"),
        };
        state.stack = vec![StackItem::Spell {
            caster: PlayerId::One,
            card,
            targets: vec![Position::Main],
        }];

        let (state, events) = drain(&state);

        assert!(state.stack.is_empty());
        assert_eq!(
            events,
            vec![
                GameEvent::StackItemResolved {
                    item: StackItem::Spell {
                        caster: PlayerId::One,
                        card,
                        targets: vec![Position::Main],
                    },
                },
                GameEvent::Healed {
                    position: Position::Main,
                    amount: 15,
                },
            ]
        );
        assert_eq!(state.players.get(PlayerId::One).discard, vec![card]);
        assert_eq!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .expect("main")
                .damage,
            0
        );
    }

    #[test]
    fn drain_resolves_an_attack_spell_against_the_opponents_main() {
        let mut state = base_state();
        state.turn.window = None;
        let card = CardRef {
            instance: CardInstanceId(99),
            def: CardDefId("ember-lance"),
        };
        state.stack = vec![StackItem::Spell {
            caster: PlayerId::One,
            card,
            targets: vec![Position::Main],
        }];

        let (state, events) = drain(&state);

        assert_eq!(
            events,
            vec![
                GameEvent::StackItemResolved {
                    item: StackItem::Spell {
                        caster: PlayerId::One,
                        card,
                        targets: vec![Position::Main],
                    },
                },
                GameEvent::DamageApplied {
                    position: Position::Main,
                    before: 0,
                    after: 10,
                },
            ]
        );
        assert_eq!(state.players.get(PlayerId::One).discard, vec![card]);
        assert_eq!(
            state
                .players
                .get(PlayerId::Two)
                .main
                .as_ref()
                .expect("main")
                .damage,
            10
        );
    }

    #[test]
    fn drain_resolves_a_draw_spell_and_settles_the_loss_check_it_enqueues() {
        let mut state = base_state();
        state.turn.window = None;
        let card = CardRef {
            instance: CardInstanceId(99),
            def: CardDefId("scrying-glass"),
        };
        state.stack = vec![StackItem::Spell {
            caster: PlayerId::Two,
            card,
            targets: vec![],
        }];

        let (state, events) = drain(&state);

        assert_eq!(
            events,
            vec![
                GameEvent::StackItemResolved {
                    item: StackItem::Spell {
                        caster: PlayerId::Two,
                        card,
                        targets: vec![],
                    },
                },
                GameEvent::CardDrawn {
                    player: PlayerId::Two,
                    card: CardInstanceId(10),
                },
            ]
        );
        assert_eq!(state.players.get(PlayerId::Two).discard, vec![card]);
        assert!(
            state.work.is_empty(),
            "the LossCheck the draw enqueued was drained too"
        );
    }
}
