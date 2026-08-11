//! The resolution loop's full three-step contract: drain `state.work`
//! (step 1), then — once `work` is empty and a double pass has closed the
//! Priority window with something still unresolved on the current Stack
//! segment — resolve the top `StackItem` and let its consequences feed
//! back into `work` (step 2), and finally rest once both are settled
//! (step 3).

use crate::domain::cards::{EffectLeaf, Query, QueryResult, find_def};
use crate::domain::events::GameEvent;
use crate::domain::ids::{PlayerId, Position};
use crate::domain::state::{GameState, PlayerState, StackItem, SummonInstance, WorkItem};
use crate::engine::{loss, upkeep};

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

        // Destruction, promotion, movement triggers, ability triggers, and
        // the general loss check (§24, §28, §36–38) have no implementation
        // yet. Draining one of these items today does nothing and produces
        // no event: an honest, documented no-op rather than a panic, so the
        // loop can keep moving once later work starts enqueueing them for
        // real.
        WorkItem::DestructionCheck(_)
        | WorkItem::DiscardDestroyedChain(_)
        | WorkItem::RecordMainLoss(_)
        | WorkItem::RecoverPrize(_)
        | WorkItem::PromoteBenchSummon(_)
        | WorkItem::ResolveMovementConsequences(_)
        | WorkItem::MovementTrigger(_, _)
        | WorkItem::FireTrigger(_, _)
        | WorkItem::LossCheck(_) => (state.clone(), Vec::new()),
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
/// resolves top-first). `StackItem::Spell` has no resolution behavior yet,
/// so it is a documented no-op besides popping and reporting; a later
/// interpreter fills this in without touching the loop above.
fn resolve_top_stack_item(state: &GameState) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let Some(item) = state.stack.pop() else {
        return (state, Vec::new());
    };

    let mut events = vec![GameEvent::StackItemResolved { item: item.clone() }];

    if let StackItem::Attack { attacker, target } = item {
        let (next_state, damage_events, hit) = resolve_attack(&state, attacker, target);
        state = next_state;
        events.extend(damage_events);
        // Only a Summon that actually took Damage has anything to check for
        // destruction; an empty targeted position is a clean miss with
        // nothing left behind to examine (rules §23, §30).
        if hit {
            state.work.push_back(WorkItem::DestructionCheck(target));
        }
    }

    (state, events)
}

/// Apply an attack's Damage to whichever Summon occupies `target` right
/// now, not whichever one was there when the attack was declared (rules
/// §30). An empty target Position is a miss: nothing to damage, no event,
/// and the returned `bool` is `false`.
fn resolve_attack(
    state: &GameState,
    attacker: PlayerId,
    target: Position,
) -> (GameState, Vec<GameEvent>, bool) {
    let mut state = state.clone();
    let effects = attacker_effects(&state, attacker);

    let defender = attacker.opponent();
    let Some(summon) = summon_at_mut(state.players.get_mut(defender), target) else {
        return (state, Vec::new(), false);
    };

    let before = summon.damage;
    let after = effects.iter().fold(before, apply_effect_leaf);
    summon.damage = after;

    (
        state,
        vec![GameEvent::DamageApplied {
            position: target,
            before,
            after,
        }],
        true,
    )
}

/// The attacker's currently printed Attack effects, re-read at resolution
/// time rather than snapshotted at declare time, since `StackItem::Attack`
/// carries no effects field of its own.
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

/// The Summon at `position`, mutably, if any.
fn summon_at_mut(player: &mut PlayerState, position: Position) -> Option<&mut SummonInstance> {
    match position {
        Position::Main => player.main.as_mut(),
        Position::Bench(slot) => player.bench[slot.index()].as_mut(),
    }
}

/// Apply one `EffectLeaf` to a Summon's accumulated Damage. Isolated here
/// so a later general effect interpreter can replace it without touching
/// `resolve_attack`. Only `DealDamage` has behavior today; every other
/// leaf is a documented no-op that leaves Damage unchanged (rules §27
/// covers a `DealDamage` leaf's `immutable` flag once modifiers exist to
/// read it — none do yet, so it is accepted but not yet consulted).
fn apply_effect_leaf(current_damage: u32, leaf: &EffectLeaf) -> u32 {
    match leaf {
        EffectLeaf::DealDamage { amount, .. } => current_damage.saturating_add(*amount),
        EffectLeaf::Heal { .. }
        | EffectLeaf::MoveSummon
        | EffectLeaf::SwapPositions
        | EffectLeaf::ConditionalBonus { .. }
        | EffectLeaf::BlockResponses(_)
        | EffectLeaf::ReturnSpellFromDiscard
        | EffectLeaf::LookAtPrizes
        | EffectLeaf::DrawCards { .. }
        | EffectLeaf::ReturnSpellToDeckTop
        | EffectLeaf::ProduceMana
        | EffectLeaf::CannotBeMovedByOpponent => current_damage,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{CardInstanceId, PlayerId, Position};
    use crate::domain::state::{
        CardRef, ManaBank, ManaSource, PendingInput, PerPlayer, Phase, PlayerState, SummonInstance,
        TurnState, UpgradeChain,
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
    fn drain_treats_not_yet_built_work_items_as_silent_no_ops() {
        let mut state = base_state();
        state.work = VecDeque::from(vec![
            WorkItem::LossCheck(PlayerId::One),
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
    fn drain_resolves_a_spell_stack_item_as_a_documented_no_op() {
        let mut state = base_state();
        state.turn.window = None;
        state.stack = vec![StackItem::Spell {
            caster: PlayerId::One,
            card: CardInstanceId(99),
            targets: vec![Position::Main],
        }];

        let (state, events) = drain(&state);

        assert!(state.stack.is_empty());
        assert_eq!(
            events,
            vec![GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card: CardInstanceId(99),
                    targets: vec![Position::Main],
                },
            }]
        );
        assert!(state.work.is_empty());
    }
}
