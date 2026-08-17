//! Split out of `mod.rs` to stay under the file length cap; shares that
//! module's items through `super::*`.
#![allow(clippy::expect_used)]

use super::*;
use crate::domain::cards::{CardSet, Component, fixtures};
use crate::domain::ids::{BenchSlot, CardInstanceId, Position};
use crate::domain::state::{
    CardRef, GameStatus, ManaBank, PerPlayer, Phase, PlayerState, StackWindow, TurnState,
    UpgradeChain,
};
use std::collections::VecDeque;
use std::sync::Arc;

fn card(def: &'static str) -> CardRef {
    CardRef {
        instance: CardInstanceId(1),
        def: fixtures::id(def),
    }
}

fn summon_of(def: &'static str, owner: PlayerId) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(card(def), vec![]),
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

/// A Summon whose printed card is `def` rather than a named fixture — the
/// shape the probe-card tests below build against.
fn summon_with_def(def: EntityId, owner: PlayerId) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(1),
                def,
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
        main: Some(summon_of("quarry-whelp", owner)),
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

/// An id no fixture card in `fixtures::entities()` uses.
fn probe_id(byte: u8) -> EntityId {
    EntityId::parse(&format!("{byte:02x}").repeat(16)).expect("valid probe id")
}

/// Every fixture entity plus `extra`, wrapped in a fresh `CardSet`/`Arc` of
/// its own — the shared fixture `Arc` (`fixtures::card_set()`) is never
/// touched.
fn cards_with(extra: Vec<Entity>) -> Arc<CardSet> {
    let mut entities = fixtures::entities();
    entities.extend(extra);
    Arc::new(CardSet::new(entities))
}

/// A private probe Trigger ability entity: one `Component::Event`, an
/// optional `Component::Respondable`, and its effects, in printed order —
/// the same shape `fixtures::trigger` builds, but free to carry its own id
/// so several can sit on the same card without colliding.
fn trigger_entity(
    id: EntityId,
    event: TriggerEvent,
    respondable: bool,
    effects: Vec<EffectLeaf>,
) -> Entity {
    let mut components = vec![Component::Event(event)];
    if respondable {
        components.push(Component::Respondable);
    }
    components.extend(effects.into_iter().map(Component::Effect));
    Entity { id, components }
}

#[test]
fn movement_event_maps_every_step_to_its_fixed_event() {
    assert_eq!(
        movement_event(MovementStep::LeavingMain),
        TriggerEvent::LeavesMain
    );
    assert_eq!(
        movement_event(MovementStep::EnteringBench),
        TriggerEvent::EntersBench
    );
    assert_eq!(
        movement_event(MovementStep::LeavingBench),
        TriggerEvent::LeavesBench
    );
    assert_eq!(
        movement_event(MovementStep::EnteringMain),
        TriggerEvent::EntersMain
    );
}

#[test]
fn movement_trigger_queues_an_immediate_heal_ability_that_drain_then_fires_on_entering_main() {
    // `movement_trigger` itself never fires an ability directly: it only
    // marks the arrival and queues one `FireTrigger` per matching ability.
    // The events this Summon's Heal produces only show up once the queue
    // is drained.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 20,
        ..summon_of("hearth-warden", PlayerId::One)
    });

    let (queued_state, immediate_events) = movement_trigger(
        &state,
        MovementStep::EnteringMain,
        PlayerId::One,
        Position::Main,
    );

    assert!(
        immediate_events.is_empty(),
        "movement_trigger queues work, it never fires an ability inline"
    );
    assert_eq!(
        queued_state.work,
        VecDeque::from(vec![WorkItem::FireTrigger(
            PlayerId::One,
            Position::Main,
            TriggerEvent::EntersMain,
            fixtures::trigger_id("hearth-warden"),
        )])
    );
    assert!(
        queued_state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .entered_main_this_turn
    );

    let (state, drained_events) = resolution::drain(&queued_state);

    assert_eq!(
        drained_events,
        vec![
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::EntersMain,
                ability: fixtures::trigger_id("hearth-warden"),
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ]
    );
    let healed = state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("main");
    assert_eq!(healed.damage, 5);
}

#[test]
fn movement_trigger_is_a_no_op_with_no_matching_trigger_ability() {
    let state = base_state();

    let (next_state, events) = movement_trigger(
        &state,
        MovementStep::LeavingMain,
        PlayerId::One,
        Position::Main,
    );

    assert_eq!(next_state, state);
    assert!(events.is_empty());
}

#[test]
fn movement_trigger_still_sets_entered_main_with_no_matching_trigger_ability() {
    let state = base_state();

    let (state, events) = movement_trigger(
        &state,
        MovementStep::EnteringMain,
        PlayerId::One,
        Position::Main,
    );

    assert!(events.is_empty());
    assert!(
        state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .entered_main_this_turn
    );
}

#[test]
fn movement_trigger_queues_several_matching_abilities_in_authored_order() {
    // Pins the `.rev()` in `movement_trigger`'s push-to-front loop: every
    // other test of this function uses a card printing exactly one
    // matching Trigger, where `.rev()` is a no-op. A card printing two
    // Trigger abilities on the same movement event proves the queue — and
    // the order the Heal effects fire in — stays authored, not reversed.
    let probe = probe_id(0x60);
    let first = trigger_entity(
        probe_id(0x61),
        TriggerEvent::EntersMain,
        false,
        vec![EffectLeaf::Heal { amount: 1 }],
    );
    let second = trigger_entity(
        probe_id(0x62),
        TriggerEvent::EntersMain,
        false,
        vec![EffectLeaf::Heal { amount: 2 }],
    );
    let card = Entity {
        id: probe,
        components: vec![
            Component::Trigger(first.clone()),
            Component::Trigger(second.clone()),
        ],
    };

    let mut state = base_state();
    state.cards = cards_with(vec![card]);
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 10,
        ..summon_with_def(probe, PlayerId::One)
    });

    let (queued_state, immediate_events) = movement_trigger(
        &state,
        MovementStep::EnteringMain,
        PlayerId::One,
        Position::Main,
    );

    assert!(immediate_events.is_empty());
    assert_eq!(
        queued_state.work,
        VecDeque::from(vec![
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                TriggerEvent::EntersMain,
                first.id,
            ),
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                TriggerEvent::EntersMain,
                second.id,
            ),
        ]),
        "several Trigger abilities matching one event queue in authored order"
    );

    let (state, events) = resolution::drain(&queued_state);

    assert_eq!(
        events,
        vec![
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::EntersMain,
                ability: first.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 1,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::EntersMain,
                ability: second.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 2,
            },
        ],
        "authored order carries through to firing, not reversed"
    );
    let main = state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("main");
    assert_eq!(
        main.damage, 7,
        "10 damage, healed 1 then 2, in authored order"
    );
}

#[test]
fn fire_queued_opens_a_respondable_window_for_the_opponent_of_the_controller() {
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(summon_of("spite-thorn", PlayerId::Two));

    let (state, events) = fire_queued(
        &state,
        PlayerId::Two,
        Position::Main,
        TriggerEvent::AnySummonDestroyed,
        fixtures::trigger_id("spite-thorn"),
    );

    assert_eq!(
        events,
        vec![GameEvent::TriggerFired {
            controller: PlayerId::Two,
            position: Position::Main,
            event: TriggerEvent::AnySummonDestroyed,
            ability: fixtures::trigger_id("spite-thorn"),
        }]
    );
    assert_eq!(state.stack_segment_bases, vec![0]);
    assert_eq!(
        state.stack,
        vec![StackItem::Trigger {
            controller: PlayerId::Two,
            source: Position::Main,
            event: TriggerEvent::AnySummonDestroyed,
            targets: vec![Position::Main],
            effects: vec![EffectLeaf::DealDamage {
                amount: 15,
                immutable: false,
            }],
        }]
    );
    assert_eq!(
        state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        })
    );
}

#[test]
fn fire_queued_is_a_silent_no_op_when_the_named_ability_is_no_longer_printed() {
    // A missing ability is not a break (see this module's own doc comment):
    // a queued `FireTrigger` naming an id the card no longer prints fires
    // nothing and leaves the state untouched.
    let mut state = base_state();
    state.players.get_mut(PlayerId::Two).main = Some(summon_of("spite-thorn", PlayerId::Two));

    let (next_state, events) = fire_queued(
        &state,
        PlayerId::Two,
        Position::Main,
        TriggerEvent::AnySummonDestroyed,
        probe_id(0xff),
    );

    assert_eq!(next_state, state);
    assert!(events.is_empty());
}

#[test]
fn discover_back_orders_opponent_of_active_first_then_main_then_bench() {
    let mut state = base_state();
    state.turn.active_player = PlayerId::One;
    state.players.get_mut(PlayerId::One).main = Some(summon_of("spite-thorn", PlayerId::One));
    state.players.get_mut(PlayerId::Two).main = Some(summon_of("spite-thorn", PlayerId::Two));
    state.players.get_mut(PlayerId::Two).bench[0] = Some(summon_of("spite-thorn", PlayerId::Two));

    let state = discover_back(
        &state,
        &destruction::ordered_players(&state),
        TriggerEvent::AnySummonDestroyed,
    );

    let ability = fixtures::trigger_id("spite-thorn");
    let queued: Vec<_> = state.work.into_iter().collect();
    assert_eq!(
        queued,
        vec![
            WorkItem::FireTrigger(
                PlayerId::Two,
                Position::Main,
                TriggerEvent::AnySummonDestroyed,
                ability,
            ),
            WorkItem::FireTrigger(
                PlayerId::Two,
                Position::Bench(BenchSlot::First),
                TriggerEvent::AnySummonDestroyed,
                ability,
            ),
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                TriggerEvent::AnySummonDestroyed,
                ability,
            ),
        ]
    );
}

#[test]
fn discover_front_pushes_ahead_of_work_already_queued() {
    // Two candidates (Main and Bench) pin the `.rev()` in this function's
    // push-to-front loop: with a single candidate, `.rev()` is a no-op, so
    // deleting it would still leave this test green.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(summon_of("spite-thorn", PlayerId::One));
    state.players.get_mut(PlayerId::One).bench[0] = Some(summon_of("spite-thorn", PlayerId::One));
    state.work = VecDeque::from(vec![WorkItem::LossCheck(PlayerId::Two)]);

    let state = discover_front(&state, &[PlayerId::One], TriggerEvent::AnySummonDestroyed);

    let ability = fixtures::trigger_id("spite-thorn");
    let queued: Vec<_> = state.work.into_iter().collect();
    assert_eq!(
        queued,
        vec![
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                TriggerEvent::AnySummonDestroyed,
                ability,
            ),
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Bench(BenchSlot::First),
                TriggerEvent::AnySummonDestroyed,
                ability,
            ),
            WorkItem::LossCheck(PlayerId::Two),
        ]
    );
}

#[test]
fn implicit_targets_heal_the_trigger_source_and_damage_the_opposing_main() {
    assert_eq!(
        implicit_targets(
            Position::Bench(BenchSlot::First),
            &[EffectLeaf::Heal { amount: 5 }]
        ),
        vec![Position::Bench(BenchSlot::First)]
    );
    assert_eq!(
        implicit_targets(
            Position::Bench(BenchSlot::First),
            &[EffectLeaf::DealDamage {
                amount: 5,
                immutable: false
            }]
        ),
        vec![Position::Main]
    );
}

// --- reading every match, not just the first -------------------------

#[test]
fn matching_abilities_reads_every_match_in_authored_order_and_nothing_for_an_unmatched_event() {
    let probe = probe_id(0x30);
    let first_match = trigger_entity(
        probe_id(0x31),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![],
    );
    let other_event = trigger_entity(probe_id(0x32), TriggerEvent::YourUpkeep, false, vec![]);
    let second_match = trigger_entity(
        probe_id(0x33),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![],
    );
    let card = Entity {
        id: probe,
        components: vec![
            Component::Trigger(first_match.clone()),
            Component::Trigger(other_event),
            Component::Trigger(second_match.clone()),
        ],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![card]);
    state.players.get_mut(PlayerId::One).main = Some(summon_with_def(probe, PlayerId::One));

    assert_eq!(
        matching_abilities(
            &state,
            PlayerId::One,
            Position::Main,
            TriggerEvent::AnySummonDestroyed
        ),
        vec![first_match.id, second_match.id]
    );
    assert_eq!(
        matching_abilities(
            &state,
            PlayerId::One,
            Position::Main,
            TriggerEvent::LeavesMain
        ),
        Vec::new()
    );
}

#[test]
fn ability_at_reads_the_named_abilitys_respondability_and_effects_and_answers_none_for_a_missing_one()
 {
    let probe = probe_id(0x34);
    let printed = trigger_entity(
        probe_id(0x35),
        TriggerEvent::AnySummonDestroyed,
        true,
        vec![EffectLeaf::Heal { amount: 7 }],
    );
    let card = Entity {
        id: probe,
        components: vec![Component::Trigger(printed.clone())],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![card]);
    state.players.get_mut(PlayerId::One).main = Some(summon_with_def(probe, PlayerId::One));

    assert_eq!(
        ability_at(&state, PlayerId::One, Position::Main, printed.id),
        Some((true, vec![EffectLeaf::Heal { amount: 7 }]))
    );
    assert_eq!(
        ability_at(&state, PlayerId::One, Position::Main, probe_id(0xff)),
        None,
        "an ability id this card does not print answers None, not a break"
    );
}

#[test]
fn discover_back_queues_nothing_for_a_card_with_no_matching_trigger() {
    // Quarry Whelp — both players' default main here — prints no Trigger
    // at all, so discovery finds no candidates and fires nothing.
    let state = base_state();

    let discovered = discover_back(
        &state,
        &[PlayerId::One, PlayerId::Two],
        TriggerEvent::AnySummonDestroyed,
    );

    assert!(discovered.work.is_empty());
    let (final_state, events) = resolution::drain(&discovered);
    assert!(events.is_empty());
    assert_eq!(final_state, state);
}

// --- the fixture fact this used to prove through `shim::find_def`'s
// `Query::Trigger` projection, re-expressed as a direct container read
// (`Query::Trigger` is retired; the fact is not) ------------------------

#[test]
fn spite_thorn_and_dawn_tenders_triggers_read_their_printed_event_respondability_and_effects() {
    let cards = fixtures::card_set();

    let spite_thorn = cards
        .get(fixtures::id("spite-thorn"))
        .expect("spite-thorn is a fixture");
    let its_trigger = spite_thorn
        .get::<Trigger>()
        .expect("spite-thorn prints a Trigger");
    assert_eq!(
        its_trigger.get::<TriggerEvent>(),
        Some(&TriggerEvent::AnySummonDestroyed)
    );
    assert!(its_trigger.get::<Respondable>().is_some());
    assert_eq!(
        its_trigger.all::<EffectLeaf>(),
        vec![&EffectLeaf::DealDamage {
            amount: 15,
            immutable: false,
        }]
    );

    let dawn_tender = cards
        .get(fixtures::id("dawn-tender"))
        .expect("dawn-tender is a fixture");
    let its_trigger = dawn_tender
        .get::<Trigger>()
        .expect("dawn-tender prints a Trigger");
    assert_eq!(
        its_trigger.get::<TriggerEvent>(),
        Some(&TriggerEvent::YourUpkeep)
    );
    assert!(its_trigger.get::<Respondable>().is_none());
    assert_eq!(
        its_trigger.all::<EffectLeaf>(),
        vec![&EffectLeaf::Heal { amount: 10 }]
    );
}

// --- the Demo: three Triggers on one card, one event, authored order ---

#[test]
fn a_card_printing_three_triggers_on_one_event_fires_all_three_in_authored_order() {
    // The capability this reading style adds over a single-match projection:
    // reading every match, not just the first `Trigger` a card printed. One
    // probe card prints three, all matching the same event, each with its
    // own id and its own Heal amount so both the `ability` field on
    // `TriggerFired` and the resulting `Healed` events independently prove
    // the order they fire in is the order they were printed.
    let probe = probe_id(0x40);
    let first = trigger_entity(
        probe_id(0x41),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![EffectLeaf::Heal { amount: 1 }],
    );
    let second = trigger_entity(
        probe_id(0x42),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![EffectLeaf::Heal { amount: 2 }],
    );
    let third = trigger_entity(
        probe_id(0x43),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![EffectLeaf::Heal { amount: 3 }],
    );
    let card = Entity {
        id: probe,
        components: vec![
            Component::Trigger(first.clone()),
            Component::Trigger(second.clone()),
            Component::Trigger(third.clone()),
        ],
    };

    let mut state = base_state();
    state.cards = cards_with(vec![card]);
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 10,
        ..summon_with_def(probe, PlayerId::One)
    });

    let discovered = discover_back(&state, &[PlayerId::One], TriggerEvent::AnySummonDestroyed);
    let (state, events) = resolution::drain(&discovered);

    assert_eq!(
        events,
        vec![
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                ability: first.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 1,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                ability: second.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 2,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                ability: third.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 3,
            },
        ],
        "all three Trigger abilities this one card prints fire, in the \
         order they were printed"
    );
    assert!(state.work.is_empty());
    let main = state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("main");
    assert_eq!(main.damage, 4, "1 + 2 + 3 Damage healed off the printed 10");
}

#[test]
fn when_the_second_of_three_matching_triggers_is_respondable_the_third_still_fires_once_the_window_closes()
 {
    // Three identical `FireTrigger` items would be indistinguishable
    // without an ability id, and the respondable one in the middle must
    // pause the queue — not lose or skip what is queued behind it (rules
    // §38, and this module's own doc comment).
    let probe = probe_id(0x50);
    let first = trigger_entity(
        probe_id(0x51),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![EffectLeaf::Heal { amount: 1 }],
    );
    let second = trigger_entity(
        probe_id(0x52),
        TriggerEvent::AnySummonDestroyed,
        true,
        vec![EffectLeaf::Heal { amount: 2 }],
    );
    let third = trigger_entity(
        probe_id(0x53),
        TriggerEvent::AnySummonDestroyed,
        false,
        vec![EffectLeaf::Heal { amount: 3 }],
    );
    let card = Entity {
        id: probe,
        components: vec![
            Component::Trigger(first.clone()),
            Component::Trigger(second.clone()),
            Component::Trigger(third.clone()),
        ],
    };

    let mut state = base_state();
    state.cards = cards_with(vec![card]);
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 10,
        ..summon_with_def(probe, PlayerId::One)
    });

    let discovered = discover_back(&state, &[PlayerId::One], TriggerEvent::AnySummonDestroyed);
    let (state, events) = resolution::drain(&discovered);

    assert_eq!(
        events,
        vec![
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                ability: first.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 1,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                ability: second.id,
            },
        ],
        "the first fires immediately, then the second — respondable — opens \
         its own window and pauses the loop before the third ever runs"
    );
    assert_eq!(
        state.work,
        VecDeque::from(vec![WorkItem::FireTrigger(
            PlayerId::One,
            Position::Main,
            TriggerEvent::AnySummonDestroyed,
            third.id,
        )]),
        "the third stays queued behind the open window instead of being \
         lost or skipped"
    );
    assert_eq!(state.stack_segment_bases, vec![0]);
    assert_eq!(
        state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        }),
        "the window opens for the opponent of the trigger's controller"
    );

    // Both players pass in order to close the window, the same way
    // `apply` would drive it.
    let opponent_passed = stack::pass(&state, PlayerId::Two).expect("Two holds Priority first");
    let controller_passed = stack::pass(&opponent_passed.state, PlayerId::One)
        .expect("One's second consecutive pass closes the window");
    let (final_state, resumed_events) = resolution::drain(&controller_passed.state);

    assert_eq!(
        resumed_events,
        vec![
            GameEvent::StackItemResolved {
                item: StackItem::Trigger {
                    controller: PlayerId::One,
                    source: Position::Main,
                    event: TriggerEvent::AnySummonDestroyed,
                    targets: vec![Position::Main],
                    effects: vec![EffectLeaf::Heal { amount: 2 }],
                },
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 2,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                ability: third.id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 3,
            },
        ],
        "closing the window resolves the second trigger's own effect, then \
         the third — queued behind it the whole time — finally fires"
    );
    assert!(final_state.work.is_empty());
    assert!(final_state.stack.is_empty());
    assert!(final_state.stack_segment_bases.is_empty());
    assert_eq!(final_state.turn.window, None);
    let main = final_state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("main");
    assert_eq!(main.damage, 4, "1 + 2 + 3 Damage healed off the printed 10");
}
