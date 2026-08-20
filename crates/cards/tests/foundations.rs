#![allow(clippy::expect_used)]

use summoners_cards::{LoadPhase, SemanticRule, SetLoadCause, StableKeyKind, parse_set};
use summoners_core::domain::{
    cards::{
        Attack, Cost, DamageConstraint, EffectCondition, EffectLeaf, Form, Life, ManaTypes,
        Modifier, Name, Persistent, Respondable, RetreatCost, Skill, SpellTiming, Tags, Trigger,
        TriggerEvent,
    },
    ids::ManaType,
};

const FOUNDATIONS: &[u8] = include_bytes!("../data/foundations.toml");

struct ExpectedSummon {
    code: &'static str,
    name: &'static str,
    form: Form,
    life: u32,
    types: &'static [ManaType],
    retreat: u32,
    attack_cost: Cost,
    attack_damage: u32,
}

#[test]
fn real_set_contains_the_twenty_approved_definitions() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    assert_eq!(loaded.code(), "foundations");
    assert_eq!(loaded.revision(), 1);
    assert_eq!(loaded.name(), "Foundations");
    assert_eq!(loaded.cards().entities().len(), 20);

    let summons = [
        ExpectedSummon {
            code: "warden-initiate",
            name: "Warden Initiate",
            form: Form::Base,
            life: 50,
            types: &[ManaType::Matter],
            retreat: 2,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "warden-pathkeeper",
            name: "Warden Pathkeeper",
            form: Form::Enhanced,
            life: 100,
            types: &[ManaType::Matter, ManaType::Mind],
            retreat: 2,
            attack_cost: Cost {
                generic: 1,
                ..Cost::default()
            },
            attack_damage: 20,
        },
        ExpectedSummon {
            code: "warden-of-set-paths",
            name: "Warden of Set Paths",
            form: Form::Elite,
            life: 150,
            types: &[ManaType::Matter, ManaType::Mind],
            retreat: 3,
            attack_cost: Cost {
                matter: 1,
                generic: 2,
                ..Cost::default()
            },
            attack_damage: 50,
        },
        ExpectedSummon {
            code: "quarry-scout",
            name: "Quarry Scout",
            form: Form::Base,
            life: 30,
            types: &[ManaType::Matter],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "quarry-warden-guard",
            name: "Quarry Warden-Guard",
            form: Form::Base,
            life: 40,
            types: &[ManaType::Matter],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "quarry-well-tender",
            name: "Quarry Well-Tender",
            form: Form::Base,
            life: 40,
            types: &[ManaType::Matter, ManaType::Mind],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "set-path-adept",
            name: "Set-Path Adept",
            form: Form::Base,
            life: 50,
            types: &[ManaType::Matter, ManaType::Mind],
            retreat: 2,
            attack_cost: Cost {
                mind: 1,
                ..Cost::default()
            },
            attack_damage: 20,
        },
        ExpectedSummon {
            code: "sow-piglet",
            name: "Sow Piglet",
            form: Form::Base,
            life: 50,
            types: &[ManaType::Spirit],
            retreat: 2,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "sow-matriarch",
            name: "Sow Matriarch",
            form: Form::Enhanced,
            life: 110,
            types: &[ManaType::Matter, ManaType::Spirit],
            retreat: 3,
            attack_cost: Cost {
                generic: 1,
                ..Cost::default()
            },
            attack_damage: 20,
        },
        ExpectedSummon {
            code: "old-sow-of-the-barrow",
            name: "Old Sow of the Barrow",
            form: Form::Elite,
            life: 170,
            types: &[ManaType::Matter, ManaType::Spirit],
            retreat: 4,
            attack_cost: Cost {
                matter: 1,
                spirit: 1,
                generic: 2,
                ..Cost::default()
            },
            attack_damage: 70,
        },
        ExpectedSummon {
            code: "hearth-warden",
            name: "Hearth Warden",
            form: Form::Base,
            life: 60,
            types: &[ManaType::Spirit],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "dawn-tender",
            name: "Dawn Tender",
            form: Form::Base,
            life: 40,
            types: &[ManaType::Spirit],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "barrow-grazer",
            name: "Barrow Grazer",
            form: Form::Base,
            life: 60,
            types: &[ManaType::Matter],
            retreat: 2,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "ash-shepherd",
            name: "Ash Shepherd",
            form: Form::Base,
            life: 40,
            types: &[ManaType::Spirit],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
        ExpectedSummon {
            code: "barrow-seer",
            name: "Barrow Seer",
            form: Form::Base,
            life: 40,
            types: &[ManaType::Spirit],
            retreat: 1,
            attack_cost: Cost::default(),
            attack_damage: 10,
        },
    ];

    for expected in summons {
        let card = card(&loaded, expected.code);
        assert_eq!(card.get::<Name>(), Some(&Name(expected.name.to_string())));
        assert_eq!(card.get::<Form>(), Some(&expected.form));
        assert_eq!(card.get::<Life>(), Some(&Life(expected.life)));
        assert_eq!(
            card.get::<ManaTypes>(),
            Some(&ManaTypes(expected.types.to_vec()))
        );
        assert_eq!(
            card.get::<RetreatCost>(),
            Some(&RetreatCost(expected.retreat))
        );
        let attacks = card.all::<Attack>();
        assert_eq!(attacks.len(), 1, "{} must have one attack", expected.code);
        assert_eq!(attacks[0].get::<Cost>(), Some(&expected.attack_cost));
        let EffectLeaf::DealDamage(damage) = attacks[0]
            .get::<EffectLeaf>()
            .expect("attack must deal Damage")
        else {
            panic!("{} has the wrong attack effect", expected.code);
        };
        assert_eq!(damage.base, expected.attack_damage);
    }
}

#[test]
fn real_set_preserves_the_approved_spell_and_enchantment_semantics() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");
    let ember = card(&loaded, "ember-lance");
    assert_eq!(ember.get::<SpellTiming>(), Some(&SpellTiming::Attack));
    assert_eq!(ember.get::<Cost>(), Some(&generic_cost(1)));
    assert_eq!(ember.get::<Persistent>(), None);
    assert!(
        matches!(ember.get::<EffectLeaf>(), Some(EffectLeaf::DealDamage(damage)) if damage.base == 10)
    );

    let scrying = card(&loaded, "scrying-glass");
    assert_eq!(scrying.get::<SpellTiming>(), Some(&SpellTiming::Support));
    assert!(matches!(
        scrying.get::<EffectLeaf>(),
        Some(EffectLeaf::DrawCards { amount: 1 })
    ));

    let wind = card(&loaded, "second-wind");
    assert!(matches!(
        wind.get::<EffectLeaf>(),
        Some(EffectLeaf::ReadySummon)
    ));

    let balm = card(&loaded, "renewing-balm");
    assert!(matches!(
        balm.get::<EffectLeaf>(),
        Some(EffectLeaf::Heal { amount: 20 })
    ));

    let ward = card(&loaded, "standing-ward");
    assert_eq!(ward.get::<SpellTiming>(), Some(&SpellTiming::Support));
    assert!(ward.get::<Persistent>().is_some());
    assert_eq!(
        ward.get::<Tags>(),
        Some(&Tags(vec!["enchantment".to_string()]))
    );
    assert!(matches!(
        ward.get::<EffectLeaf>(),
        Some(EffectLeaf::Heal { amount: 10 })
    ));
    assert_eq!(
        ward.get::<Modifier>(),
        Some(&Modifier::IncomingAttackDamageReduction(10))
    );
}

#[test]
fn real_set_preserves_complex_ability_semantics_and_response_modes() {
    let loaded = parse_set(FOUNDATIONS).expect("Foundations must load");

    let warden = card(&loaded, "warden-of-set-paths");
    assert!(matches!(
        warden.all::<Skill>()[0].get::<EffectLeaf>(),
        Some(EffectLeaf::SwapOpposingPositions)
    ));
    assert_eq!(
        warden.get::<Modifier>(),
        Some(&Modifier::OpposingRetreatCostDelta(1))
    );
    let warden_attack = warden.all::<Attack>()[0];
    let EffectLeaf::DealDamage(damage) = &warden_attack.all::<EffectLeaf>()[0] else {
        panic!("Warden attack must deal Damage");
    };
    assert_eq!(damage.additions.len(), 1);
    assert_eq!(damage.additions[0].amount, 30);
    assert_eq!(
        damage.additions[0].condition,
        EffectCondition::DefenderEnteredMainThisTurn
    );

    let adept = card(&loaded, "set-path-adept").all::<Attack>()[0];
    let adept_effects = adept.all::<EffectLeaf>();
    assert!(
        matches!(&adept_effects[0], EffectLeaf::DealDamage(damage) if damage.additions[0].condition == EffectCondition::SpellPlayedThisTurn)
    );
    assert!(matches!(
        &adept_effects[1],
        EffectLeaf::BlockResponses {
            condition: EffectCondition::SpellPlayedThisTurn,
            ..
        }
    ));

    let old_sow = card(&loaded, "old-sow-of-the-barrow");
    let skill_effects = old_sow.all::<Skill>()[0].all::<EffectLeaf>();
    assert!(matches!(
        skill_effects.as_slice(),
        [
            EffectLeaf::Heal { amount: 30 },
            EffectLeaf::CannotBeMovedByOpponent
        ]
    ));
    let EffectLeaf::DealDamage(damage) = old_sow.all::<Attack>()[0]
        .get::<EffectLeaf>()
        .expect("Old Sow Damage")
    else {
        panic!("Old Sow attack must deal Damage");
    };
    assert!(damage.constraints.contains(DamageConstraint::Unpreventable));
    assert!(damage.constraints.contains(DamageConstraint::Unincreasable));

    assert_trigger(
        &loaded,
        "quarry-warden-guard",
        TriggerEvent::LeavesMain,
        false,
        &EffectLeaf::Heal { amount: 10 },
    );
    assert_trigger(
        &loaded,
        "quarry-well-tender",
        TriggerEvent::LeavesBench,
        false,
        &EffectLeaf::ProduceMana,
    );
    assert_trigger(
        &loaded,
        "hearth-warden",
        TriggerEvent::EntersMain,
        false,
        &EffectLeaf::Heal { amount: 15 },
    );
    assert_trigger(
        &loaded,
        "dawn-tender",
        TriggerEvent::YourUpkeep,
        false,
        &EffectLeaf::Heal { amount: 10 },
    );
    assert_trigger(
        &loaded,
        "barrow-grazer",
        TriggerEvent::EntersBench,
        false,
        &EffectLeaf::Heal { amount: 10 },
    );
    assert_trigger(
        &loaded,
        "ash-shepherd",
        TriggerEvent::AnySummonDestroyed,
        true,
        &EffectLeaf::ReturnSpellFromDiscard,
    );

    let seer = card(&loaded, "barrow-seer").all::<Skill>()[0];
    assert_eq!(
        seer.get::<Cost>(),
        Some(&Cost {
            spirit: 1,
            generic: 1,
            ..Cost::default()
        })
    );
    assert_eq!(
        seer.all::<EffectLeaf>(),
        vec![
            &EffectLeaf::LookAtPrizes,
            &EffectLeaf::DrawCards { amount: 1 },
            &EffectLeaf::ReturnSpellToDeckTop,
        ]
    );
}

#[test]
fn card_identity_ignores_revision_display_text_and_document_order() {
    let alpha = summon_block("alpha", "Alpha", "First text", 10);
    let beta = summon_block("beta", "Beta", "Second text", 20);
    let original = parse_set(set_document(1, "Display Set", &[&alpha, &beta]).as_bytes())
        .expect("original Set loads");
    let changed = parse_set(
        set_document(
            9,
            "Renamed Set",
            &[
                &beta
                    .replace("Beta", "Renamed Beta")
                    .replace("Second text", "New text"),
                &alpha
                    .replace("Alpha", "Renamed Alpha")
                    .replace("First text", "Other text")
                    .replace("Printed Strike", "Renamed Strike")
                    .replace("Printed ability text", "Other ability text"),
            ],
        )
        .as_bytes(),
    )
    .expect("changed Set loads");
    assert_eq!(original.id(), changed.id());
    assert_eq!(original.card_id("alpha"), changed.card_id("alpha"));
    assert_eq!(original.card_id("beta"), changed.card_id("beta"));
    assert_eq!(
        card(&original, "alpha").all::<Attack>()[0].id,
        card(&changed, "alpha").all::<Attack>()[0].id
    );
}

#[test]
fn malformed_documents_return_typed_paths_and_causes() {
    let utf8 = parse_set(&[0xff]).expect_err("invalid UTF-8 must fail");
    assert_eq!(utf8.phase, LoadPhase::Utf8);
    assert!(matches!(utf8.cause, SetLoadCause::InvalidUtf8 { .. }));

    let missing = parse_set(b"id = \"x\"").expect_err("missing version must fail");
    assert_eq!(missing.path, "schema_version");
    assert_eq!(missing.cause, SetLoadCause::MissingSchemaVersion);

    let unsupported = parse_set(b"schema_version = 9\nunknown = true")
        .expect_err("unsupported version must fail before strict decode");
    assert_eq!(unsupported.phase, LoadPhase::Version);
    assert_eq!(unsupported.schema_version, Some(9));

    let unknown = parse_set(
        b"schema_version = 1\nid = \"x\"\nrevision = 1\nname = \"X\"\ncards = []\nunknown = true",
    )
    .expect_err("unknown field must fail");
    assert_eq!(unknown.phase, LoadPhase::Decode);
    assert!(matches!(unknown.cause, SetLoadCause::SchemaDecode { .. }));

    let invalid = summon_block("Invalid_Key", "Invalid", "Text", 10);
    let invalid = parse_set(set_document(1, "Set", &[&invalid]).as_bytes())
        .expect_err("invalid stable key must fail");
    assert_eq!(invalid.path, "cards[0].code");
    assert!(matches!(
        invalid.cause,
        SetLoadCause::InvalidStableKey {
            kind: StableKeyKind::Card,
            ..
        }
    ));

    let duplicate = summon_block("same", "Same", "Text", 10);
    let duplicate = parse_set(set_document(1, "Set", &[&duplicate, &duplicate]).as_bytes())
        .expect_err("duplicate stable key must fail");
    assert_eq!(duplicate.path, "cards[1].code");
    assert!(matches!(
        duplicate.cause,
        SetLoadCause::DuplicateStableKey { .. }
    ));

    let missing_life = summon_block("alpha", "Alpha", "Text", 10).replace("life = 10\n", "");
    let missing_life = parse_set(set_document(1, "Set", &[&missing_life]).as_bytes())
        .expect_err("invalid Summon structure must fail");
    assert_eq!(missing_life.path, "cards[0].life");
    assert_eq!(
        missing_life.cause,
        SetLoadCause::MissingRequiredField { field: "life" }
    );

    let wrong_selector = summon_block("alpha", "Alpha", "Text", 10)
        .replace("defending-main", "selected-opposing-position");
    let wrong_selector = parse_set(set_document(1, "Set", &[&wrong_selector]).as_bytes())
        .expect_err("invalid selector must fail");
    assert_eq!(
        wrong_selector.path,
        "cards[0].abilities[0].effects[0].target"
    );
    assert_eq!(
        wrong_selector.cause,
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::EffectTarget
        }
    );

    let duplicate_ability = format!(
        "{}\n{}",
        summon_block("alpha", "Alpha", "Text", 10),
        attack_block("strike", 20)
    );
    let duplicate_ability = parse_set(set_document(1, "Set", &[&duplicate_ability]).as_bytes())
        .expect_err("duplicate ability code must fail");
    assert_eq!(duplicate_ability.path, "cards[0].abilities[1].code");
    assert!(matches!(
        duplicate_ability.cause,
        SetLoadCause::DuplicateStableKey {
            kind: StableKeyKind::Ability,
            ..
        }
    ));
}

fn card<'a>(
    loaded: &'a summoners_cards::LoadedSet,
    code: &str,
) -> &'a summoners_core::domain::cards::Entity {
    let id = loaded.card_id(code).expect("approved card code exists");
    loaded.cards().get(id).expect("approved card id exists")
}

fn generic_cost(amount: u32) -> Cost {
    Cost {
        generic: amount,
        ..Cost::default()
    }
}

fn assert_trigger(
    loaded: &summoners_cards::LoadedSet,
    code: &str,
    event: TriggerEvent,
    respondable: bool,
    effect: &EffectLeaf,
) {
    let trigger = card(loaded, code).all::<Trigger>()[0];
    assert_eq!(trigger.get::<TriggerEvent>(), Some(&event));
    assert_eq!(trigger.get::<Respondable>().is_some(), respondable);
    assert_eq!(trigger.get::<EffectLeaf>(), Some(effect));
}

fn set_document(revision: u32, name: &str, cards: &[&str]) -> String {
    format!(
        "schema_version = 1\nid = \"identity-set\"\nrevision = {revision}\nname = \"{name}\"\n\n{}",
        cards.join("\n")
    )
}

fn summon_block(code: &str, name: &str, text: &str, damage: u32) -> String {
    format!(
        r#"
[[cards]]
code = "{code}"
name = "{name}"
text = "{text}"
kind = "summon"
form = "base"
life = 10
types = ["matter"]
retreat = 1

[[cards.abilities]]
code = "strike"
name = "Printed Strike"
text = "Printed ability text"
kind = "attack"
cost = []

[[cards.abilities.effects]]
kind = "damage"
target = "defending-main"
base = {damage}
"#
    )
}

fn attack_block(code: &str, damage: u32) -> String {
    format!(
        r#"
[[cards.abilities]]
code = "{code}"
kind = "attack"
cost = []

[[cards.abilities.effects]]
kind = "damage"
target = "defending-main"
base = {damage}
"#
    )
}
