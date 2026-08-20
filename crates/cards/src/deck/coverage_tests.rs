#![allow(clippy::expect_used)]

use super::*;
use crate::parse_set;

const FOUNDATIONS: &[u8] = include_bytes!("../../data/foundations.toml");
const BODY_KEYS: [&str; 10] = [
    "foundations/warden-pathkeeper",
    "foundations/warden-of-set-paths",
    "foundations/quarry-scout",
    "foundations/quarry-warden-guard",
    "foundations/quarry-well-tender",
    "foundations/set-path-adept",
    "foundations/ember-lance",
    "foundations/scrying-glass",
    "foundations/second-wind",
    "foundations/standing-ward",
];

fn library() -> CardLibrary {
    CardLibrary::from_sets([parse_set(FOUNDATIONS).expect("Foundations parses")])
        .expect("Foundations forms one library")
}

fn requirement(set: &str, revision: u32) -> SetRequirementDto {
    SetRequirementDto {
        set: set.to_string(),
        revision,
    }
}

fn card(key: &str, quantity: u32) -> CardQuantityDto {
    CardQuantityDto {
        card: key.to_string(),
        quantity,
    }
}

fn valid_cards() -> Vec<CardQuantityDto> {
    BODY_KEYS.into_iter().map(|key| card(key, 2)).collect()
}

fn valid_dto() -> DeckDto {
    DeckDto {
        schema_version: 1,
        id: "set-paths".to_string(),
        name: "Set Paths".to_string(),
        starter: "foundations/warden-initiate".to_string(),
        requires: vec![requirement("foundations", 1)],
        cards: valid_cards(),
    }
}

#[test]
fn byte_and_schema_helpers_accept_the_valid_v1_shape() {
    let source = decode_utf8(b"schema_version = 1").expect("UTF-8 is valid");
    assert_eq!(read_version(source).expect("version exists"), 1);

    let decoded = decode_v1(
        r#"
schema_version = 1
id = "test"
name = "Test"
starter = "foundations/warden-initiate"
requires = [{ set = "foundations", revision = 1 }]
cards = []
"#,
    )
    .expect("private v1 shape is valid");
    assert_eq!(decoded.id, "test");
}

#[test]
fn resolve_builds_domain_data_and_checks_its_header() {
    let library = library();
    let deck = resolve(valid_dto(), &library).expect("valid DTO resolves");
    assert_eq!(
        deck.starter,
        library
            .card_id("foundations/warden-initiate")
            .expect("known")
    );
    assert_eq!(deck.body.len(), 20);

    let mut invalid_version = valid_dto();
    invalid_version.schema_version = 2;
    assert!(matches!(
        resolve(invalid_version, &library)
            .expect_err("version is exact")
            .cause,
        DeckLoadCause::UnsupportedSchemaVersion { found: 2 }
    ));

    let mut invalid_id = valid_dto();
    invalid_id.id = "Bad".to_string();
    assert!(matches!(
        resolve(invalid_id, &library)
            .expect_err("id is stable")
            .cause,
        DeckLoadCause::InvalidStableKey { .. }
    ));

    let mut invalid_name = valid_dto();
    invalid_name.name = " ".to_string();
    assert_eq!(
        resolve(invalid_name, &library)
            .expect_err("name is required")
            .cause,
        DeckLoadCause::NameMustNotBeEmpty
    );
}

#[test]
fn requirement_resolution_covers_success_and_each_rejection() {
    let library = library();
    let resolved = resolve_requirements(vec![requirement("foundations", 1)], &library)
        .expect("requirement matches");
    assert_eq!(resolved.get("foundations"), Some(&1));

    let invalid_key = resolve_requirements(vec![requirement("Foundations", 1)], &library)
        .expect_err("Set key is stable");
    assert!(matches!(
        invalid_key.cause,
        DeckLoadCause::InvalidStableKey { .. }
    ));

    let duplicate = resolve_requirements(
        vec![requirement("foundations", 1), requirement("foundations", 1)],
        &library,
    )
    .expect_err("requirements are unique");
    assert!(matches!(
        duplicate.cause,
        DeckLoadCause::DuplicateSetRequirement { .. }
    ));

    assert!(matches!(
        resolve_requirements(vec![requirement("other", 1)], &library)
            .expect_err("Set must exist")
            .cause,
        DeckLoadCause::RequiredSetUnavailable { .. }
    ));
    assert!(matches!(
        resolve_requirements(vec![requirement("foundations", 2)], &library)
            .expect_err("revision must match")
            .cause,
        DeckLoadCause::SetRevisionMismatch { .. }
    ));
}

#[test]
fn body_resolution_covers_success_and_each_construction_rejection() {
    let library = library();
    let requirements = BTreeMap::from([("foundations".to_string(), 1)]);
    let starter = "foundations/warden-initiate";
    assert_eq!(
        resolve_body(valid_cards(), &library, &requirements, starter)
            .expect("body is legal")
            .len(),
        20
    );

    let cases = [
        (
            vec![card("other/card", 2)],
            DeckLoadCause::MissingSetRequirement {
                set: "other".to_string(),
            },
        ),
        (
            vec![card("foundations/ember-lance", 0)],
            DeckLoadCause::QuantityMustBePositive,
        ),
        (
            vec![card(starter, 2)],
            DeckLoadCause::StarterInBody {
                key: starter.to_string(),
            },
        ),
        (
            vec![card("foundations/missing", 2)],
            DeckLoadCause::UnresolvedCard {
                key: "foundations/missing".to_string(),
            },
        ),
        (
            vec![card("foundations/ember-lance", 3)],
            DeckLoadCause::CopyLimitExceeded {
                key: "foundations/ember-lance".to_string(),
                copies: 3,
                limit: 2,
            },
        ),
        (
            vec![card("foundations/ember-lance", 2)],
            DeckLoadCause::BodyCardCount {
                expected: 20,
                found: 2,
            },
        ),
    ];
    for (body, expected) in cases {
        assert_eq!(
            resolve_body(body, &library, &requirements, starter)
                .expect_err("case must fail")
                .cause,
            expected
        );
    }
}

#[test]
fn reference_helpers_cover_success_and_failure_values_directly() {
    let library = library();
    let requirements = BTreeMap::from([("foundations".to_string(), 1)]);
    let known = parse_qualified("foundations/warden-initiate", "starter").expect("qualified key");

    require_bound(&known, &requirements, "starter").expect("Set is required");
    let unbound = QualifiedCard {
        key: "other/card".to_string(),
        set: "other".to_string(),
    };
    assert!(matches!(
        require_bound(&unbound, &requirements, "card")
            .expect_err("Set is not required")
            .cause,
        DeckLoadCause::MissingSetRequirement { .. }
    ));

    let known_id = resolve_card(&library, &known, "starter").expect("card exists");
    assert!(matches!(
        resolve_card(
            &library,
            &QualifiedCard {
                key: "foundations/missing".to_string(),
                set: "foundations".to_string(),
            },
            "card",
        )
        .expect_err("card is absent")
        .cause,
        DeckLoadCause::UnresolvedCard { .. }
    ));

    require_base_starter(&library, known_id, &known.key).expect("Starter is Base");
    let enhanced = library
        .card_id("foundations/warden-pathkeeper")
        .expect("Enhanced exists");
    assert!(matches!(
        require_base_starter(&library, enhanced, "foundations/warden-pathkeeper")
            .expect_err("Starter is not Base")
            .cause,
        DeckLoadCause::StarterMustBeBase { .. }
    ));
}

#[test]
fn error_helpers_preserve_kind_path_phase_and_version() {
    require_stable_key("valid-key", DeckStableKeyKind::Card, "card").expect("key is valid");
    let stable =
        require_stable_key("Bad", DeckStableKeyKind::Card, "card").expect_err("key is invalid");
    assert!(matches!(
        stable.cause,
        DeckLoadCause::InvalidStableKey {
            kind: DeckStableKeyKind::Card,
            ..
        }
    ));

    let qualified = invalid_qualified("bad", "starter");
    assert_eq!(qualified.path, "starter");
    assert!(matches!(
        qualified.cause,
        DeckLoadCause::InvalidStableKey {
            kind: DeckStableKeyKind::QualifiedCard,
            ..
        }
    ));

    let semantic = semantic_error("cards", DeckLoadCause::QuantityMustBePositive);
    assert_eq!(semantic.document, DocumentKind::Deck);
    assert_eq!(semantic.phase, LoadPhase::Semantics);
    assert_eq!(semantic.schema_version, Some(1));
    assert_eq!(semantic.path, "cards");
}
