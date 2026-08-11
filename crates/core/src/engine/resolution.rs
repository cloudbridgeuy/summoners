//! The resolution loop's full three-step contract: drain `state.work`
//! (step 1), then — once `work` is empty and a double pass has closed the
//! Priority window with something still unresolved on the current Stack
//! segment — resolve the top `StackItem` and let its consequences feed
//! back into `work` (step 2), and finally rest once both are settled
//! (step 3).

use crate::domain::cards::{CardDefId, EffectLeaf, Query, QueryResult, find_def};
use crate::domain::events::GameEvent;
use crate::domain::ids::{PlayerId, Position};
use crate::domain::state::{CardRef, GameState, StackItem, WorkItem};
use crate::engine::{destruction, effects, loss, upkeep};

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

        if let Some(item) = state.work.pop_front() {
            let (next_state, item_events) = execute(&state, &item);
            state = next_state;
            events.extend(item_events);
            continue;
        }

        // Step 2: the work queue is settled. If the Priority window has
        // closed (rules §33) and the current Stack segment still holds
        // something, resolve its top item next (rules §35 — strictly
        // top-first); whatever it enqueues onto `work` is picked up by step
        // 1 on the next iteration.
        if state.turn.window.is_none() && stack_has_unresolved_items(&state) {
            let (next_state, item_events) = resolve_top_stack_item(&state);
            state = next_state;
            events.extend(item_events);
            continue;
        }

        // Step 3: both the work queue and the current Stack segment are
        // settled — rest here until the next action.
        break;
    }

    (state, events)
}

/// Whether the current Stack segment still holds an item to resolve.
/// `stack_segment_bases` is storage-only today (there is exactly one
/// implicit segment, based at 0), so this drains the whole Stack; once
/// nested segments exist, resolving down to the current segment's base
/// becomes a data change here, not a control-flow rewrite.
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

        // Movement and ability triggers (§28, §36–38) have no fixture that
        // reads one yet. Draining one of these items today does nothing and
        // produces no event: an honest, documented no-op rather than a
        // panic, so the loop can keep moving once later work starts
        // consuming them for real.
        WorkItem::MovementTrigger(_, _) | WorkItem::FireTrigger(_, _) => {
            (state.clone(), Vec::new())
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
/// its state and events forward.
fn apply_leaves(
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
    fn drain_treats_movement_and_ability_triggers_as_silent_no_ops() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![
            WorkItem::MovementTrigger(MovementStep::LeavingMain, Position::Main),
            WorkItem::FireTrigger(
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
