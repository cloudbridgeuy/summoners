#![allow(clippy::expect_used, clippy::unwrap_used)]
use super::*;
use std::collections::HashSet;
use summoners_cards::built_in_catalog;
use summoners_core::domain::cards::{
    AccountingId, Component, DamageConstraints, DamageEffect, EffectCondition, EffectTarget,
    Entity, Form, Modifier, ResponseBlock, SpellTiming, Tags, TriggerEvent,
};
use summoners_core::domain::state::{LossReason, ManaSource};
use summoners_match_log::wire::EntityIdV1;

fn state_and_descriptions() -> (GameState, CardDescriptions) {
    let catalog = built_in_catalog().expect("catalog");
    let state = crate::setup::initial_state(
        catalog.library(),
        [catalog.set_paths(), catalog.barrow_herd()],
        4,
    )
    .expect("state");
    let descriptions = CardDescriptions::from_card_set(&state.cards);
    (state, descriptions)
}

fn probe_id(byte_pair: &str) -> EntityId {
    EntityId::parse(&byte_pair.repeat(16)).expect("valid probe id")
}

fn entity(id: EntityId, components: Vec<Component>) -> Entity {
    Entity { id, components }
}

fn unique_count<T: Eq + std::hash::Hash + Clone>(items: &[T]) -> usize {
    items.iter().cloned().collect::<HashSet<_>>().len()
}

const DEBUG_ARTIFACTS: [&str; 7] = [
    "Some(", "None", "Bench(", "First", "Second", "Third", "Matter)",
];

fn assert_no_debug_artifact(text: &str) {
    assert!(
        !text.chars().any(char::is_control),
        "control character in {text:?}"
    );
    for artifact in DEBUG_ARTIFACTS {
        assert!(!text.contains(artifact), "{text} contains {artifact}");
    }
}

fn full_effect_leaves() -> Vec<(EffectLeaf, &'static str)> {
    vec![
        (
            EffectLeaf::DealDamage(DamageEffect {
                base: 5,
                constraints: DamageConstraints::new(),
                additions: vec![],
            }),
            "deal 5 damage",
        ),
        (
            EffectLeaf::Heal {
                amount: 3,
                target: EffectTarget::Selected,
            },
            "heal 3",
        ),
        (EffectLeaf::MoveSummon, "move a Summon"),
        (EffectLeaf::SwapPositions, "swap positions"),
        (
            EffectLeaf::BlockResponses {
                condition: EffectCondition::SpellPlayedThisTurn,
                block: ResponseBlock::AttackSpells,
            },
            "block responses",
        ),
        (
            EffectLeaf::ReturnSpellFromDiscard,
            "return a Spell from discard",
        ),
        (EffectLeaf::LookAtPrizes, "look at Prizes"),
        (EffectLeaf::DrawCards { amount: 2 }, "draw 2 cards"),
        (
            EffectLeaf::ReturnSpellToDeckTop,
            "return a Spell to deck top",
        ),
        (
            EffectLeaf::ProduceMana {
                target: EffectTarget::Source,
            },
            "produce Mana",
        ),
        (
            EffectLeaf::CannotBeMovedByOpponent {
                target: EffectTarget::Selected,
            },
            "prevent opponent movement",
        ),
        (EffectLeaf::ReadySummon, "ready a Summon"),
        (EffectLeaf::SwapOpposingPositions, "swap opposing positions"),
    ]
}

fn full_ability_entity(id: EntityId, name: &str) -> Entity {
    entity(
        id,
        vec![
            Component::Name(Name(name.to_string())),
            Component::Cost(Cost {
                matter: 1,
                mind: 0,
                spirit: 0,
                generic: 1,
            }),
            Component::Effect(EffectLeaf::DrawCards { amount: 1 }),
            Component::Effect(EffectLeaf::Heal {
                amount: 2,
                target: EffectTarget::Source,
            }),
            Component::Event(TriggerEvent::YourUpkeep),
            Component::Timing(SpellTiming::Attack),
            Component::Respondable,
            Component::Persistent,
            Component::Passive(Modifier::IncomingAttackDamageReduction(1)),
        ],
    )
}

#[test]
fn describe_card_reads_every_scalar_component() {
    let card = entity(
        probe_id("a1"),
        vec![
            Component::Name(Name("Warden".to_string())),
            Component::Life(Life(30)),
            Component::RetreatCost(RetreatCost(2)),
            Component::Produces(ManaTypes(vec![
                ManaType::Matter,
                ManaType::Mind,
                ManaType::Spirit,
            ])),
            Component::Cost(Cost {
                matter: 1,
                mind: 2,
                spirit: 3,
                generic: 4,
            }),
            Component::Timing(SpellTiming::Support),
            Component::Persistent,
        ],
    );
    let description = describe_card(&card);
    assert_eq!(description.name, "Warden");
    assert_eq!(description.life, Some(30));
    assert_eq!(description.retreat_cost, Some(2));
    assert_eq!(description.mana_types, vec!["Matter", "Mind", "Spirit"]);
    assert_eq!(
        description.cost.as_deref(),
        Some("matter 1 mind 2 spirit 3 generic 4")
    );
    assert_eq!(description.timing.as_deref(), Some("support"));
    assert!(description.persistent);
    assert!(description.abilities.is_empty());
    assert!(description.effects.is_empty());
    assert!(description.modifiers.is_empty());
}

#[test]
fn describe_card_reads_attack_timing() {
    let card = entity(probe_id("a2"), vec![Component::Timing(SpellTiming::Attack)]);
    assert_eq!(describe_card(&card).timing.as_deref(), Some("attack"));
}

#[test]
fn describe_card_defaults_absent_scalars_to_none_and_empty() {
    let card = entity(probe_id("a3"), vec![]);
    let description = describe_card(&card);
    assert_eq!(description.name, "Unnamed card");
    assert_eq!(description.life, None);
    assert_eq!(description.retreat_cost, None);
    assert!(description.mana_types.is_empty());
    assert_eq!(description.cost, None);
    assert!(description.abilities.is_empty());
    assert!(description.effects.is_empty());
    assert!(description.modifiers.is_empty());
    assert_eq!(description.timing, None);
    assert!(!description.persistent);
}

#[test]
fn describe_card_lists_every_effect_leaf_variant_in_order() {
    let leaves = full_effect_leaves();
    let components = leaves
        .iter()
        .cloned()
        .map(|(leaf, _)| Component::Effect(leaf))
        .collect();
    let card = entity(probe_id("b1"), components);
    let expected: Vec<String> = leaves.iter().map(|(_, text)| (*text).to_string()).collect();
    assert_eq!(describe_card(&card).effects, expected);
    assert_eq!(expected.len(), 13);
}

#[test]
fn describe_card_lists_every_modifier_variant_in_order() {
    let card = entity(
        probe_id("b2"),
        vec![
            Component::Passive(Modifier::OpposingRetreatCostDelta(-2)),
            Component::Passive(Modifier::IncomingAttackDamageReduction(4)),
        ],
    );
    assert_eq!(
        describe_card(&card).modifiers,
        vec![
            "opposing retreat cost -2".to_string(),
            "reduce incoming attack damage by 4".to_string(),
        ]
    );
}

#[test]
fn describe_card_aggregates_abilities_in_skill_attack_trigger_order() {
    let card = entity(
        probe_id("c5"),
        vec![
            Component::Trigger(full_ability_entity(probe_id("c4"), "Ember Wake")),
            Component::Skill(full_ability_entity(probe_id("c2"), "Overcharge")),
            Component::Attack(full_ability_entity(probe_id("c3"), "Cleave")),
        ],
    );
    let description = describe_card(&card);
    let kinds: Vec<AbilityKind> = description
        .abilities
        .iter()
        .map(|ability| ability.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![
            AbilityKind::Skill,
            AbilityKind::Attack,
            AbilityKind::Trigger
        ]
    );
    let names: Vec<&str> = description
        .abilities
        .iter()
        .map(|ability| ability.name.as_str())
        .collect();
    assert_eq!(names, vec!["Overcharge", "Cleave", "Ember Wake"]);
}

#[test]
fn describe_card_preserves_authored_order_within_one_ability_kind() {
    let card = entity(
        probe_id("c6"),
        vec![
            Component::Skill(entity(
                probe_id("c7"),
                vec![Component::Name(Name("First Skill".into()))],
            )),
            Component::Skill(entity(
                probe_id("c8"),
                vec![Component::Name(Name("Second Skill".into()))],
            )),
        ],
    );
    let description = describe_card(&card);
    let names: Vec<&str> = description
        .abilities
        .iter()
        .map(|ability| ability.name.as_str())
        .collect();
    assert_eq!(names, vec!["First Skill", "Second Skill"]);
}

#[test]
fn describe_card_reports_unnamed_card_and_describe_ability_reports_unnamed_ability() {
    let card = entity(probe_id("d1"), vec![]);
    assert_eq!(describe_card(&card).name, "Unnamed card");
    let ability = entity(probe_id("d2"), vec![]);
    assert_eq!(
        describe_ability(AbilityKind::Skill, &ability).name,
        "Unnamed ability"
    );
}

#[test]
fn describe_ability_reads_every_field_for_a_skill() {
    let id = probe_id("c1");
    let ability = full_ability_entity(id, "Overcharge");
    let description = describe_ability(AbilityKind::Skill, &ability);
    assert_eq!(description.kind, AbilityKind::Skill);
    assert_eq!(description.id, EntityIdV1(id.to_string()));
    assert_eq!(description.name, "Overcharge");
    assert_eq!(
        description.cost.as_deref(),
        Some("matter 1 mind 0 spirit 0 generic 1")
    );
    assert_eq!(
        description.effects,
        vec!["draw 1 cards".to_string(), "heal 2".to_string()]
    );
    assert_eq!(description.trigger_event.as_deref(), Some("your upkeep"));
    assert_eq!(description.timing.as_deref(), Some("attack"));
    assert!(description.respondable);
    assert!(description.persistent);
    assert_eq!(
        description.modifiers,
        vec!["reduce incoming attack damage by 1".to_string()]
    );
}

#[test]
fn describe_ability_reads_every_field_for_an_attack_and_a_trigger() {
    for kind in [AbilityKind::Attack, AbilityKind::Trigger] {
        let id = probe_id("c9");
        let ability = full_ability_entity(id, "Cleave");
        let description = describe_ability(kind, &ability);
        assert_eq!(description.kind, kind);
        assert_eq!(description.id, EntityIdV1(id.to_string()));
        assert!(description.respondable);
        assert!(description.persistent);
        assert_eq!(description.timing.as_deref(), Some("attack"));
        assert_eq!(description.trigger_event.as_deref(), Some("your upkeep"));
    }
}

#[test]
fn describe_ability_trigger_event_maps_every_variant_to_a_distinct_string() {
    let events = [
        (TriggerEvent::YourUpkeep, "your upkeep"),
        (TriggerEvent::EntersMain, "enters Main"),
        (TriggerEvent::EntersBench, "enters Bench"),
        (TriggerEvent::LeavesMain, "leaves Main"),
        (TriggerEvent::LeavesBench, "leaves Bench"),
        (TriggerEvent::AnySummonDestroyed, "any Summon destroyed"),
    ];
    let mut seen = HashSet::new();
    for (event, expected) in events {
        let ability = entity(probe_id("e1"), vec![Component::Event(event)]);
        let description = describe_ability(AbilityKind::Trigger, &ability);
        assert_eq!(description.trigger_event.as_deref(), Some(expected));
        assert!(seen.insert(expected));
    }
    assert_eq!(seen.len(), 6);
}

#[test]
fn describe_ability_respondable_and_persistent_are_absent_by_default() {
    let ability = entity(probe_id("e2"), vec![]);
    let description = describe_ability(AbilityKind::Skill, &ability);
    assert!(!description.respondable);
    assert!(!description.persistent);
    assert!(description.cost.is_none());
    assert!(description.effects.is_empty());
    assert!(description.trigger_event.is_none());
    assert!(description.timing.is_none());
    assert!(description.modifiers.is_empty());
}

#[test]
fn card_text_helpers_map_every_component_variant_without_fallback() {
    let variants = vec![
        Component::Name(Name("x".into())),
        Component::AccountingId(AccountingId {
            prefix: "QRY".into(),
            number: 1,
        }),
        Component::Life(Life(1)),
        Component::RetreatCost(RetreatCost(1)),
        Component::Form(Form::Base),
        Component::Produces(ManaTypes(vec![])),
        Component::Tags(Tags(vec![])),
        Component::Cost(Cost::default()),
        Component::Skill(entity(probe_id("f1"), vec![])),
        Component::Attack(entity(probe_id("f2"), vec![])),
        Component::Trigger(entity(probe_id("f3"), vec![])),
        Component::Effect(EffectLeaf::MoveSummon),
        Component::Passive(Modifier::IncomingAttackDamageReduction(1)),
        Component::Timing(SpellTiming::Support),
        Component::Event(TriggerEvent::YourUpkeep),
        Component::Respondable,
        Component::Persistent,
    ];
    assert_eq!(variants.len(), 17);
    for component in &variants {
        let _ = crate::card_text::modifier(component);
        let _ = crate::card_text::timing(component);
        let _ = crate::card_text::event(component);
        let _ = crate::card_text::respondable(component);
        let _ = crate::card_text::persistent(component);
    }
    assert_eq!(
        crate::card_text::modifier(&Component::Passive(Modifier::OpposingRetreatCostDelta(1))),
        Some("opposing retreat cost +1".to_string())
    );
    assert_eq!(
        crate::card_text::modifier(&Component::Passive(
            Modifier::IncomingAttackDamageReduction(2)
        )),
        Some("reduce incoming attack damage by 2".to_string())
    );
    assert_eq!(
        crate::card_text::timing(&Component::Timing(SpellTiming::Support)),
        Some("support".to_string())
    );
    assert_eq!(
        crate::card_text::timing(&Component::Timing(SpellTiming::Attack)),
        Some("attack".to_string())
    );
    assert_eq!(
        crate::card_text::event(&Component::Event(TriggerEvent::EntersMain)),
        Some("enters Main".to_string())
    );
    assert!(crate::card_text::respondable(&Component::Respondable));
    assert!(!crate::card_text::respondable(&Component::Persistent));
    assert!(crate::card_text::persistent(&Component::Persistent));
    assert!(!crate::card_text::persistent(&Component::Respondable));
}

#[test]
fn text_helpers_map_every_variant_to_a_distinct_string() {
    let players: Vec<&str> = [PlayerId::One, PlayerId::Two]
        .iter()
        .map(|player| player_text(*player))
        .collect();
    assert_eq!(players, vec!["Player One", "Player Two"]);
    assert_eq!(players.len(), unique_count(&players));

    let benches: Vec<&str> = BenchSlot::ALL
        .iter()
        .map(|slot| bench_text(*slot))
        .collect();
    assert_eq!(benches, vec!["Bench 1", "Bench 2", "Bench 3"]);
    assert_eq!(benches.len(), unique_count(&benches));

    let manas: Vec<&str> = [ManaType::Matter, ManaType::Mind, ManaType::Spirit]
        .iter()
        .map(|mana| mana_text(*mana))
        .collect();
    assert_eq!(manas, vec!["Matter", "Mind", "Spirit"]);
    assert_eq!(manas.len(), unique_count(&manas));

    let positions: Vec<String> = [
        Position::Main,
        Position::Bench(BenchSlot::First),
        Position::Bench(BenchSlot::Second),
        Position::Bench(BenchSlot::Third),
    ]
    .iter()
    .map(|position| position_text(*position))
    .collect();
    assert_eq!(positions, vec!["Main", "Bench 1", "Bench 2", "Bench 3"]);
    assert_eq!(positions.len(), unique_count(&positions));

    let events: Vec<&str> = [
        TriggerEvent::YourUpkeep,
        TriggerEvent::EntersMain,
        TriggerEvent::EntersBench,
        TriggerEvent::LeavesMain,
        TriggerEvent::LeavesBench,
        TriggerEvent::AnySummonDestroyed,
    ]
    .iter()
    .map(|event| crate::card_text::trigger_event(*event))
    .collect();
    assert_eq!(events.len(), unique_count(&events));

    for text in players
        .iter()
        .chain(benches.iter())
        .chain(manas.iter())
        .chain(events.iter())
    {
        assert_no_debug_artifact(text);
    }
    for text in &positions {
        assert_no_debug_artifact(text);
    }
}

#[test]
fn effect_text_maps_every_variant_to_a_distinct_string() {
    let leaves = full_effect_leaves();
    let texts: Vec<String> = leaves.iter().map(|(leaf, _)| effect_text(leaf)).collect();
    assert_eq!(texts.len(), unique_count(&texts));
    for text in &texts {
        assert_no_debug_artifact(text);
    }
}

#[test]
fn cost_text_reports_every_field() {
    let cost = Cost {
        matter: 1,
        mind: 2,
        spirit: 3,
        generic: 4,
    };
    assert_eq!(cost_text(&cost), "matter 1 mind 2 spirit 3 generic 4");
}

#[test]
fn phase_view_maps_every_phase_variant() {
    assert_eq!(phase_view(Phase::Upkeep), PhaseView::Upkeep);
    assert_eq!(phase_view(Phase::Main), PhaseView::Main);
    assert_eq!(phase_view(Phase::Combat), PhaseView::Combat);
}

#[test]
fn readiness_view_maps_every_variant() {
    assert_eq!(readiness_view(Readiness::Ready), ReadinessView::Ready);
    assert_eq!(
        readiness_view(Readiness::Exhausted),
        ReadinessView::Exhausted
    );
}

#[test]
fn pending_view_maps_every_pending_input_variant() {
    assert_eq!(
        pending_view(PendingInput::ManaProduction {
            player: PlayerId::One,
            source: ManaSource::Player,
        }),
        PendingView {
            kind: PendingKindView::ManaProduction,
            owner: Seat::One,
        }
    );
    assert_eq!(
        pending_view(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Bench(BenchSlot::Second)),
        }),
        PendingView {
            kind: PendingKindView::ManaProduction,
            owner: Seat::Two,
        }
    );
    assert_eq!(
        pending_view(PendingInput::Promotion {
            player: PlayerId::One
        }),
        PendingView {
            kind: PendingKindView::Promotion,
            owner: Seat::One,
        }
    );
    assert_eq!(
        pending_view(PendingInput::PrizePick {
            chooser: PlayerId::Two
        }),
        PendingView {
            kind: PendingKindView::PrizePick,
            owner: Seat::Two,
        }
    );
}

#[test]
fn outcome_view_maps_every_loss_reason_and_preserves_winner() {
    for (reason, expected) in [
        (LossReason::ThirdMainLoss, OutcomeReasonView::ThirdMainLoss),
        (
            LossReason::NoPromotionAvailable,
            OutcomeReasonView::NoPromotionAvailable,
        ),
        (LossReason::EmptyDeckDraw, OutcomeReasonView::EmptyDeckDraw),
        (LossReason::Resignation, OutcomeReasonView::Resignation),
    ] {
        let view = outcome_view(GameOutcome {
            winner: PlayerId::Two,
            reason,
        });
        assert_eq!(
            view,
            OutcomeView {
                winner: Seat::Two,
                reason: expected,
            }
        );
    }
}

#[test]
fn stack_view_reports_full_attack_fields() {
    let (_, descriptions) = state_and_descriptions();
    let view = stack_view(
        &StackItem::Attack {
            attacker: PlayerId::Two,
            target: Position::Bench(BenchSlot::Second),
        },
        &descriptions,
    );
    assert_eq!(
        view,
        StackView::Attack {
            attacker: Seat::Two,
            target: "Bench 2".to_string(),
        }
    );
}

#[test]
fn stack_view_reports_full_spell_fields() {
    let (state, descriptions) = state_and_descriptions();
    let card = state.players.one.hand[0];
    let view = stack_view(
        &StackItem::Spell {
            caster: PlayerId::One,
            card,
            targets: vec![Position::Main, Position::Bench(BenchSlot::First)],
        },
        &descriptions,
    );
    match view {
        StackView::Spell {
            caster,
            card: description,
            targets,
        } => {
            assert_eq!(caster, Seat::One);
            assert_eq!(description, descriptions.get(card.def));
            assert_eq!(targets, vec!["Main".to_string(), "Bench 1".to_string()]);
        }
        other => panic!("expected StackView::Spell, got {other:?}"),
    }
}

#[test]
fn stack_view_reports_full_trigger_fields() {
    let (state, descriptions) = state_and_descriptions();
    let card = state.players.one.hand[0];
    let view = stack_view(
        &StackItem::Trigger {
            controller: PlayerId::Two,
            source: Position::Main,
            ability: card.def,
            event: TriggerEvent::LeavesBench,
            targets: vec![Position::Bench(BenchSlot::Third)],
            effects: vec![EffectLeaf::DrawCards { amount: 3 }],
        },
        &descriptions,
    );
    assert_eq!(
        view,
        StackView::Trigger {
            controller: Seat::Two,
            source: "Main".to_string(),
            event: "leaves Bench".to_string(),
            targets: vec!["Bench 3".to_string()],
            effects: vec!["draw 3 cards".to_string()],
        }
    );
}

#[test]
fn adversarial_control_characters_are_escaped_and_other_text_passes_through() {
    let input = "Full Card\n\r\t\u{1b}\u{7}\u{7f}{:?}{}\"'\u{f1} Some(Bench(First".to_string();
    let expected =
        "Full Card\\n\\r\\t\\x1b\\u{0007}\\u{007f}{:?}{}\"'\u{f1} Some(Bench(First".to_string();

    let card = entity(probe_id("1a"), vec![Component::Name(Name(input.clone()))]);
    let description = describe_card(&card);
    assert_eq!(description.name, expected);
    assert!(!description.name.chars().any(char::is_control));

    let ability = entity(probe_id("1b"), vec![Component::Name(Name(input))]);
    let ability_description = describe_ability(AbilityKind::Skill, &ability);
    assert_eq!(ability_description.name, expected);
    assert!(!ability_description.name.chars().any(char::is_control));
}

const CATALOG_DEBUG_ARTIFACTS: [&str; 5] = ["Some(", "Bench(", "Matter)", "Mind)", "Spirit)"];

fn assert_catalog_text_is_clean(text: &str) {
    assert!(
        !text.chars().any(char::is_control),
        "control character in {text:?}"
    );
    for artifact in CATALOG_DEBUG_ARTIFACTS {
        assert!(!text.contains(artifact), "{text} contains {artifact}");
    }
}

fn assert_description_is_clean(description: &CardDescription) {
    assert_catalog_text_is_clean(&description.name);
    if let Some(cost) = &description.cost {
        assert_catalog_text_is_clean(cost);
    }
    for mana in &description.mana_types {
        assert_catalog_text_is_clean(mana);
    }
    for effect in &description.effects {
        assert_catalog_text_is_clean(effect);
    }
    for modifier in &description.modifiers {
        assert_catalog_text_is_clean(modifier);
    }
    if let Some(timing) = &description.timing {
        assert_catalog_text_is_clean(timing);
    }
    for ability in &description.abilities {
        assert_catalog_text_is_clean(&ability.name);
        if let Some(cost) = &ability.cost {
            assert_catalog_text_is_clean(cost);
        }
        for effect in &ability.effects {
            assert_catalog_text_is_clean(effect);
        }
        if let Some(event) = &ability.trigger_event {
            assert_catalog_text_is_clean(event);
        }
        if let Some(timing) = &ability.timing {
            assert_catalog_text_is_clean(timing);
        }
        for modifier in &ability.modifiers {
            assert_catalog_text_is_clean(modifier);
        }
    }
}

#[test]
fn every_catalog_card_description_is_free_of_control_characters_and_debug_artifacts() {
    let catalog = built_in_catalog().expect("catalog");
    let cards = catalog.library().core_cards();
    assert!(!cards.entities().is_empty());
    for card in cards.entities() {
        assert_description_is_clean(&describe_card(card));
    }
}

#[test]
fn protocol_and_card_text_source_never_uses_debug_formatting_tokens() {
    let protocol_source = include_str!("../protocol.rs");
    let card_text_source = include_str!("../card_text.rs");
    assert!(!protocol_source.contains(":?}"));
    assert!(!card_text_source.contains(":?}"));
}
