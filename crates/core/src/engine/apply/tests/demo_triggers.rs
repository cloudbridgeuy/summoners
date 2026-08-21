//! Integration-style demo tests for movement triggers and respondable
//! destruction triggers, driven through `apply` end to end. Split out of
//! `demo.rs` to stay under the file length cap; shares `tests.rs`'s
//! fixtures through `super::*`.

use super::*;

// -- Demo: movement triggers fire in fixed §28 order through Retreat, and
// a respondable destruction trigger opens and resumes a mid-drain Priority
// window (rules §28, §36-41). ------------------------------------------

#[test]
fn demo_retreat_fires_the_four_movement_triggers_and_heals_the_entering_main_summon_through_apply()
{
    // Rules §26, §28, §36: retreating swaps Main with a Bench slot and
    // fires all four movement-trigger steps in fixed order — LeavingMain,
    // EnteringBench, LeavingBench, EnteringMain. Neither Quarry Whelp nor
    // Hearth Warden carries a Trigger for the first three, so only the
    // last one, Hearth Warden's immediate Heal on entering Main, produces
    // any event.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).bench[0] = Some(SummonInstance {
        chain: UpgradeChain::new(card_ref(9, "hearth-warden"), vec![]),
        damage: 20,
        ..summon(PlayerId::One)
    });
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 0,
    };

    let outcome = apply(
        &state,
        &GameAction::Retreat {
            player: PlayerId::One,
            slot: BenchSlot::First,
            mana_hint: None,
        },
    )
    .expect("Quarry Whelp's printed Retreat Cost of 1 is affordable");

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
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: crate::domain::cards::TriggerEvent::EntersMain,
                ability: fixtures::trigger_id("hearth-warden"),
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ],
        "EnteringMain fires last, in fixed §28 order, after the three \
         silent steps ahead of it"
    );
    let healed = outcome
        .state
        .players
        .get(PlayerId::One)
        .main
        .as_ref()
        .expect("Hearth Warden landed on Main");
    assert_eq!(healed.damage, 5);
    assert!(healed.turn.main_entry.is_some());
}

#[test]
fn demo_a_respondable_destruction_trigger_opens_a_mid_drain_window_and_resumes_the_chain_through_apply()
 {
    // Rules §23-25, §37-38, §40-41: a lethal attack destroys Two's Main.
    // Discovering Spite Thorn's respondable trigger on One's own Bench
    // interrupts Two's destruction chain mid-drain and opens a window for
    // the opponent of the trigger's controller (One) — Two. Once both
    // players pass, the segment resolves — Spite Thorn's Damage misses
    // Two's still-empty Main, since Promotion has not run yet — and only
    // then does the destruction chain it interrupted resume underneath
    // it: an empty Prize pool is a silent no-op, and Two's lone Bench
    // Summon promotes automatically.
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).bench[0] = Some(SummonInstance {
        chain: UpgradeChain::new(card_ref(9, "spite-thorn"), vec![]),
        ..summon(PlayerId::One)
    });
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        damage: 30, // ten more finishes Quarry Whelp's printed Life of 40.
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).bench[0] = Some(summon(PlayerId::Two));

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("Quarry Whelp's Attack is free and Main is a legal target");
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority first");

    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the attacker passes second, closing the window and draining the Attack");

    assert_eq!(
        compact_damage_events(&resolved.events),
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            legacy_damage(Position::Main, 30, 40),
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
                ability: fixtures::trigger_id("spite-thorn"),
            },
        ],
        "discovering Spite Thorn's respondable trigger interrupts the rest \
         of Two's destruction chain and opens a window before any of it \
         runs"
    );
    assert_eq!(
        resolved.state.stack,
        vec![StackItem::Trigger {
            controller: PlayerId::One,
            source: Position::Bench(BenchSlot::First),
            ability: fixtures::trigger_id("spite-thorn"),
            event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
            targets: vec![Position::Main],
            effects: vec![crate::domain::cards::EffectLeaf::DealDamage(
                crate::domain::cards::DamageEffect {
                    base: 15,
                    constraints: crate::domain::cards::DamageConstraints::new(),
                    additions: vec![]
                }
            )],
        }],
        "the respondable trigger's Stack index becomes a new segment base"
    );
    assert_eq!(resolved.state.stack_segment_bases, vec![0]);
    assert_eq!(
        resolved.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        }),
        "the window opens for the opponent of the trigger's controller (rules §38)"
    );

    let responder_passed = apply(
        &resolved.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Two holds Priority first");
    let resumed = apply(
        &responder_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("One passes second, closing the window and resuming the interrupted chain");

    assert_eq!(
        resumed.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Trigger {
                    controller: PlayerId::One,
                    source: Position::Bench(BenchSlot::First),
                    ability: fixtures::trigger_id("spite-thorn"),
                    event: crate::domain::cards::TriggerEvent::AnySummonDestroyed,
                    targets: vec![Position::Main],
                    effects: vec![crate::domain::cards::EffectLeaf::DealDamage(
                        crate::domain::cards::DamageEffect {
                            base: 15,
                            constraints: crate::domain::cards::DamageConstraints::new(),
                            additions: vec![]
                        }
                    )],
                },
            },
            GameEvent::SummonPromoted {
                player: PlayerId::Two,
                from: BenchSlot::First,
            },
        ],
        "the segment's own item resolved first — missing, since Two's Main \
         is still empty — then the destruction chain it interrupted \
         resumed below it"
    );
    assert!(
        resumed.state.stack.is_empty(),
        "the segment's item was the only thing on the Stack"
    );
    assert!(
        resumed.state.stack_segment_bases.is_empty(),
        "the segment's base came off once its item resolved"
    );
    assert!(
        resumed.state.work.is_empty(),
        "every remaining destruction-chain step, and the movement triggers \
         Promotion queued, ran once the segment settled"
    );
    assert_eq!(resumed.state.turn.window, None);
    assert_eq!(resumed.state.pending, None);
    assert_eq!(resumed.state.status, GameStatus::Playing);
    let two = resumed.state.players.get(PlayerId::Two);
    assert!(two.main.is_some(), "Promotion filled the empty Main");
    assert_eq!(two.bench, [None, None, None]);
}
