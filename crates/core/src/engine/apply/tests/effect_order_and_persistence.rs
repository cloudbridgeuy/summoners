//! Two facts about reading printed cards straight off the container,
//! proven end to end through `apply`, off a private card set that never
//! touches the shared fixtures:
//!
//! 1. An ability printing four `Component::Effect`s resolves all four, in
//!    the order they were printed.
//! 2. Persistence is not a card family: a Spell that also prints
//!    `Component::Persistent` stays in play after resolving, added to its
//!    caster's `enchantments`, instead of moving to their discard pile —
//!    with `CardKind` left untouched.
//!
//! Split out of `tests.rs` to stay under the file length cap; shares that
//! module's fixtures through `super::*`, like `board_economy.rs` and its
//! siblings.

use super::*;
use crate::domain::cards::{
    CardKind, CardSet, Component, EffectLeaf, Entity, EntityId, Form, Life, ManaTypes, SpellTiming,
    Tags, TriggerEvent, family,
};
use crate::domain::state::WorkItem;
use std::sync::Arc;

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

#[test]
fn a_probe_spells_four_printed_effects_resolve_in_authored_order() {
    // A private probe Spell — never added to the shared fixtures — that
    // prints four `Component::Effect`s in a fixed order: Damage, Heal,
    // Ready, then a draw. Each leaf changes different, independently
    // observable state, so the events they emit prove all four ran, in
    // exactly the order they were printed.
    let probe = probe_id(0x10);
    let card = CardRef {
        instance: CardInstanceId(1),
        def: probe,
    };
    let entity = Entity {
        id: probe,
        components: vec![
            Component::Tags(Tags(vec!["spell".to_string()])),
            Component::Timing(SpellTiming::Support),
            Component::Effect(EffectLeaf::DealDamage(crate::domain::cards::DamageEffect {
                base: 5,
                constraints: crate::domain::cards::DamageConstraints::new(),
                additions: vec![],
            })),
            Component::Effect(EffectLeaf::Heal {
                amount: 3,
                target: crate::domain::cards::EffectTarget::Selected,
            }),
            Component::Effect(EffectLeaf::ReadySummon),
            Component::Effect(EffectLeaf::DrawCards { amount: 1 }),
        ],
    };

    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        damage: 5,
        ready: false,
        ..summon(PlayerId::One)
    });
    state.players.get_mut(PlayerId::One).hand = vec![card];
    state.players.get_mut(PlayerId::One).deck = vec![card_ref(50, "quarry-whelp")];

    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(1),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("the probe Spell prints no Cost, so it is free, and Support timing is legal in the caster's own resting Main");

    let defender_passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster holds Priority second");

    assert_eq!(
        compact_damage_events(&resolved.events),
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card,
                    targets: vec![Position::Main],
                },
            },
            legacy_damage(Position::Main, 0, 5),
            GameEvent::Healed {
                position: Position::Main,
                amount: 3,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: CardInstanceId(50),
            },
        ],
        "all four printed effects ran, each producing its own event, in the \
         order they were printed"
    );
}

#[test]
fn a_probe_spell_printing_persistent_stays_in_play_instead_of_discarding() {
    // A private probe Spell — tagged "spell", not "enchantment" — that also
    // prints `Component::Persistent`. Nothing in `CardKind` changes: this
    // card is `CardKind::Spell` the whole way through `cast_spell`'s timing
    // gate. Persistence is decided by the printed component alone.
    let probe = probe_id(0x20);
    let card = CardRef {
        instance: CardInstanceId(1),
        def: probe,
    };
    let entity = Entity {
        id: probe,
        components: vec![
            Component::Tags(Tags(vec!["spell".to_string()])),
            Component::Timing(SpellTiming::Support),
            Component::Persistent,
        ],
    };
    assert_eq!(
        family(&entity),
        CardKind::Spell,
        "tagging the probe \"spell\" keeps its family Spell — Persistent is \
         read as its own component, never folded into CardKind"
    );

    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::One).hand = vec![card];

    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(1),
            targets: vec![],
            mana_hint: None,
        },
    )
    .expect("the probe Spell prints no Cost, so it is free, and Support timing is legal in the caster's own resting Main");

    let defender_passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster holds Priority second");

    let caster = resolved.state.players.get(PlayerId::One);
    assert_eq!(
        caster.enchantments,
        vec![card],
        "printing Persistent moves the resolved card to enchantments, not discard"
    );
    assert!(
        caster.discard.is_empty(),
        "a persisting card never reaches the discard pile"
    );
}

#[test]
fn a_required_effect_draw_failure_stops_the_spells_later_effects() {
    let probe = probe_id(0x30);
    let card = CardRef {
        instance: CardInstanceId(1),
        def: probe,
    };
    let entity = Entity {
        id: probe,
        components: vec![
            Component::Tags(Tags(vec!["spell".to_string()])),
            Component::Timing(SpellTiming::Support),
            Component::Effect(EffectLeaf::DrawCards { amount: 2 }),
            Component::Effect(EffectLeaf::Heal {
                amount: 10,
                target: crate::domain::cards::EffectTarget::Selected,
            }),
        ],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::One).hand = vec![card];
    state.players.get_mut(PlayerId::One).deck = vec![card_ref(50, "quarry-whelp")];
    state
        .players
        .get_mut(PlayerId::One)
        .main
        .as_mut()
        .expect("main")
        .damage = 10;

    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: card.instance,
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("the probe Support Spell is legal");
    let passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender passes");
    let resolved = apply(
        &passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster passes and the Spell resolves");

    assert_eq!(
        resolved.state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::Two,
            reason: LossReason::EmptyDeckDraw,
        })
    );
    assert_eq!(
        resolved
            .state
            .players
            .get(PlayerId::One)
            .main
            .as_ref()
            .expect("main")
            .damage,
        10
    );
    assert!(matches!(
        resolved.events.as_slice(),
        [
            GameEvent::PriorityPassed {
                player: PlayerId::One
            },
            GameEvent::StackItemResolved { .. },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: CardInstanceId(50)
            },
            GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: LossReason::EmptyDeckDraw
            },
        ]
    ));
}

#[test]
fn a_non_active_players_trigger_produces_mana_for_its_controller() {
    let card_id = probe_id(0x40);
    let ability = probe_id(0x41);
    let entity = Entity {
        id: card_id,
        components: vec![
            Component::Life(Life(40)),
            Component::Form(Form::Base),
            Component::Produces(ManaTypes(vec![ManaType::Matter])),
            Component::Trigger(Entity {
                id: ability,
                components: vec![
                    Component::Event(TriggerEvent::AnySummonDestroyed),
                    Component::Effect(EffectLeaf::ProduceMana {
                        target: crate::domain::cards::EffectTarget::Source,
                    }),
                ],
            }),
        ],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(60),
                def: card_id,
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(61, "quarry-whelp")];
    state.work.push_back(WorkItem::FireTrigger(
        PlayerId::Two,
        Position::Main,
        TriggerEvent::AnySummonDestroyed,
        ability,
    ));

    let outcome = apply(
        &state,
        &GameAction::PlaySummon {
            player: PlayerId::One,
            card: CardInstanceId(61),
            slot: BenchSlot::First,
        },
    )
    .expect("the active player's public action drains the queued opposing trigger");

    assert_eq!(outcome.state.players.get(PlayerId::One).mana.matter, 0);
    assert_eq!(outcome.state.players.get(PlayerId::Two).mana.matter, 1);
    assert!(outcome.events.contains(&GameEvent::ManaProduced {
        player: PlayerId::Two,
        source: ManaSource::Summon(Position::Main),
        mana_type: ManaType::Matter,
    }));
}

#[test]
fn a_non_active_trigger_controller_answers_its_pending_mana_choice() {
    let card_id = probe_id(0x50);
    let ability = probe_id(0x51);
    let entity = Entity {
        id: card_id,
        components: vec![
            Component::Life(Life(40)),
            Component::Form(Form::Base),
            Component::Produces(ManaTypes(vec![ManaType::Matter, ManaType::Mind])),
            Component::Trigger(Entity {
                id: ability,
                components: vec![
                    Component::Event(TriggerEvent::AnySummonDestroyed),
                    Component::Effect(EffectLeaf::ProduceMana {
                        target: crate::domain::cards::EffectTarget::Source,
                    }),
                ],
            }),
        ],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(70),
                def: card_id,
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(71, "quarry-whelp")];
    state.work.push_back(WorkItem::FireTrigger(
        PlayerId::Two,
        Position::Main,
        TriggerEvent::AnySummonDestroyed,
        ability,
    ));

    let paused = apply(
        &state,
        &GameAction::PlaySummon {
            player: PlayerId::One,
            card: CardInstanceId(71),
            slot: BenchSlot::First,
        },
    )
    .expect("the public action drains the opposing trigger until its choice");
    assert_eq!(
        paused.state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Main),
        })
    );
    assert_eq!(
        apply(
            &paused.state,
            &GameAction::EndTurn {
                player: PlayerId::One,
            },
        ),
        Err(ActionError::NotYourDecision)
    );

    let answered = apply(
        &paused.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Mind,
        },
    )
    .expect("the trigger controller answers its own pending Mana choice");
    assert_eq!(answered.state.pending, None);
    assert_eq!(answered.state.players.get(PlayerId::Two).mana.mind, 1);
}
