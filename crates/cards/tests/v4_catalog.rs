#![allow(clippy::expect_used)]

use std::{collections::BTreeMap, sync::Arc};

use summoners_cards::{
    CardLibrary, DeckLoadCause, DocumentKind, LibraryError, LoadPhase, built_in_catalog,
    parse_deck, parse_set,
};

const FOUNDATIONS: &[u8] = include_bytes!("../data/foundations.toml");
const SET_PATHS: &[u8] = include_bytes!("../data/set-paths.toml");

fn library() -> CardLibrary {
    let set = parse_set(FOUNDATIONS).expect("Foundations must parse");
    CardLibrary::from_sets([set]).expect("Foundations must form a library")
}

fn cards(lines: &[(&str, u32)]) -> String {
    lines
        .iter()
        .map(|(card, quantity)| format!("[[cards]]\ncard = \"{card}\"\nquantity = {quantity}\n"))
        .collect()
}

fn deck_document(id: &str, name: &str, starter: &str, requires: &str, body: &str) -> Vec<u8> {
    format!(
        "schema_version = 1\nid = \"{id}\"\nname = \"{name}\"\nstarter = \"{starter}\"\nrequires = {requires}\n\n{body}"
    )
    .into_bytes()
}

fn valid_body() -> String {
    cards(&[
        ("foundations/warden-pathkeeper", 2),
        ("foundations/warden-of-set-paths", 2),
        ("foundations/quarry-scout", 2),
        ("foundations/quarry-warden-guard", 2),
        ("foundations/quarry-well-tender", 2),
        ("foundations/set-path-adept", 2),
        ("foundations/ember-lance", 2),
        ("foundations/scrying-glass", 2),
        ("foundations/second-wind", 2),
        ("foundations/standing-ward", 2),
    ])
}

fn custom_deck(starter: &str, requires: &str, body: &str) -> Vec<u8> {
    deck_document("caller-deck", "Caller Deck", starter, requires, body)
}

#[test]
fn library_resolves_qualified_keys_and_reuses_one_core_allocation() {
    let library = library();
    let first = library.core_cards();
    let second = library.core_cards();

    assert_eq!(library.set_revision("foundations"), Some(1));
    assert!(library.card_id("foundations/ember-lance").is_some());
    assert!(library.card_id("other/ember-lance").is_none());
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(first.entities().len(), 20);
}

#[test]
fn library_rejects_duplicate_qualified_keys() {
    let set = parse_set(FOUNDATIONS).expect("Foundations must parse");
    let error = CardLibrary::from_sets([set.clone(), set]).expect_err("keys must be unique");
    assert!(matches!(error, LibraryError::DuplicateQualifiedKey { .. }));
}

#[test]
fn public_parser_accepts_caller_owned_bytes() {
    let source = SET_PATHS.to_vec();
    let deck = parse_deck(&source, &library()).expect("caller bytes must parse");
    assert_eq!(deck.id(), "set-paths");
    assert_eq!(deck.name(), "Set Paths");
    assert_eq!(deck.body().len(), 20);
}

#[test]
fn parser_reports_utf8_version_syntax_and_schema_failures() {
    let library = library();
    let utf8 = parse_deck(&[b'a', 0xff], &library).expect_err("UTF-8 must be valid");
    assert_eq!(utf8.document, DocumentKind::Deck);
    assert_eq!(utf8.phase, LoadPhase::Utf8);
    assert!(matches!(utf8.cause, DeckLoadCause::InvalidUtf8 { .. }));

    let missing = parse_deck(b"id = \"x\"", &library).expect_err("version is required");
    assert_eq!(missing.phase, LoadPhase::Version);
    assert_eq!(missing.cause, DeckLoadCause::MissingSchemaVersion);

    let invalid =
        parse_deck(b"schema_version = \"one\"", &library).expect_err("version must be an integer");
    assert!(matches!(
        invalid.cause,
        DeckLoadCause::InvalidSchemaVersion { .. }
    ));

    let unsupported = parse_deck(b"schema_version = 2\nunknown = true", &library)
        .expect_err("unsupported versions stop before strict decode");
    assert_eq!(unsupported.schema_version, Some(2));
    assert_eq!(unsupported.phase, LoadPhase::Version);
    assert_eq!(
        unsupported.cause,
        DeckLoadCause::UnsupportedSchemaVersion { found: 2 }
    );

    let syntax =
        parse_deck(b"schema_version = [", &library).expect_err("TOML syntax must be valid");
    assert!(matches!(syntax.cause, DeckLoadCause::TomlSyntax { .. }));

    let unknown = parse_deck(
        b"schema_version = 1\nid = \"x\"\nname = \"X\"\nstarter = \"foundations/warden-initiate\"\nrequires = [{ set = \"foundations\", revision = 1, extra = true }]\ncards = []",
        &library,
    )
    .expect_err("nested unknown fields must fail");
    assert_eq!(unknown.phase, LoadPhase::Decode);
    assert!(matches!(unknown.cause, DeckLoadCause::SchemaDecode { .. }));
}

#[test]
fn parser_reports_deck_identity_and_name_failures() {
    let library = library();
    let invalid_id = deck_document(
        "Bad_ID",
        "Deck",
        "foundations/warden-initiate",
        "[{ set = \"foundations\", revision = 1 }]",
        &valid_body(),
    );
    let error = parse_deck(&invalid_id, &library).expect_err("Deck id must be stable");
    assert!(matches!(
        error.cause,
        DeckLoadCause::InvalidStableKey { .. }
    ));

    let empty_name = deck_document(
        "valid-id",
        "  ",
        "foundations/warden-initiate",
        "[{ set = \"foundations\", revision = 1 }]",
        &valid_body(),
    );
    assert_eq!(
        parse_deck(&empty_name, &library)
            .expect_err("name must not be empty")
            .cause,
        DeckLoadCause::NameMustNotBeEmpty
    );
}

#[test]
fn parser_reports_each_set_requirement_failure() {
    let library = library();
    let duplicate = custom_deck(
        "foundations/warden-initiate",
        "[{ set = \"foundations\", revision = 1 }, { set = \"foundations\", revision = 1 }]",
        &valid_body(),
    );
    assert!(matches!(
        parse_deck(&duplicate, &library)
            .expect_err("requirements must be unique")
            .cause,
        DeckLoadCause::DuplicateSetRequirement { .. }
    ));

    let unavailable = custom_deck(
        "other/starter",
        "[{ set = \"other\", revision = 1 }]",
        &valid_body(),
    );
    assert_eq!(
        parse_deck(&unavailable, &library)
            .expect_err("required Set must be loaded")
            .cause,
        DeckLoadCause::RequiredSetUnavailable {
            set: "other".to_string(),
        }
    );

    let wrong = custom_deck(
        "foundations/warden-initiate",
        "[{ set = \"foundations\", revision = 2 }]",
        &valid_body(),
    );
    assert_eq!(
        parse_deck(&wrong, &library)
            .expect_err("revision must match")
            .cause,
        DeckLoadCause::SetRevisionMismatch {
            set: "foundations".to_string(),
            required: 2,
            available: 1,
        }
    );

    let unbound = custom_deck("foundations/warden-initiate", "[]", &valid_body());
    assert_eq!(
        parse_deck(&unbound, &library)
            .expect_err("each reference needs a requirement")
            .cause,
        DeckLoadCause::MissingSetRequirement {
            set: "foundations".to_string(),
        }
    );
}

#[test]
fn parser_reports_reference_and_starter_failures() {
    let library = library();
    let invalid_ref = custom_deck(
        "foundations",
        "[{ set = \"foundations\", revision = 1 }]",
        &valid_body(),
    );
    assert!(matches!(
        parse_deck(&invalid_ref, &library)
            .expect_err("reference must be qualified")
            .cause,
        DeckLoadCause::InvalidStableKey { .. }
    ));

    let unresolved = custom_deck(
        "foundations/missing",
        "[{ set = \"foundations\", revision = 1 }]",
        &valid_body(),
    );
    assert_eq!(
        parse_deck(&unresolved, &library)
            .expect_err("Starter must resolve")
            .cause,
        DeckLoadCause::UnresolvedCard {
            key: "foundations/missing".to_string(),
        }
    );

    let non_base = custom_deck(
        "foundations/warden-pathkeeper",
        "[{ set = \"foundations\", revision = 1 }]",
        &valid_body(),
    );
    assert_eq!(
        parse_deck(&non_base, &library)
            .expect_err("Starter must be Base")
            .cause,
        DeckLoadCause::StarterMustBeBase {
            key: "foundations/warden-pathkeeper".to_string(),
        }
    );
}

#[test]
fn parser_reports_each_body_construction_failure() {
    let library = library();
    let requirements = "[{ set = \"foundations\", revision = 1 }]";

    let unresolved_body = custom_deck(
        "foundations/warden-initiate",
        requirements,
        &cards(&[("foundations/missing", 2)]),
    );
    assert!(matches!(
        parse_deck(&unresolved_body, &library)
            .expect_err("body card must resolve")
            .cause,
        DeckLoadCause::UnresolvedCard { .. }
    ));

    let starter_body = custom_deck(
        "foundations/warden-initiate",
        requirements,
        &cards(&[("foundations/warden-initiate", 2)]),
    );
    assert!(matches!(
        parse_deck(&starter_body, &library)
            .expect_err("Starter must stay separate")
            .cause,
        DeckLoadCause::StarterInBody { .. }
    ));

    let zero = custom_deck(
        "foundations/warden-initiate",
        requirements,
        &cards(&[("foundations/ember-lance", 0)]),
    );
    assert_eq!(
        parse_deck(&zero, &library)
            .expect_err("quantity must be positive")
            .cause,
        DeckLoadCause::QuantityMustBePositive
    );

    let too_many = custom_deck(
        "foundations/warden-initiate",
        requirements,
        &cards(&[("foundations/ember-lance", 3)]),
    );
    assert!(matches!(
        parse_deck(&too_many, &library)
            .expect_err("copy limit must hold")
            .cause,
        DeckLoadCause::CopyLimitExceeded { .. }
    ));

    let duplicate_lines = custom_deck(
        "foundations/warden-initiate",
        requirements,
        &cards(&[
            ("foundations/ember-lance", 2),
            ("foundations/ember-lance", 1),
        ]),
    );
    assert!(matches!(
        parse_deck(&duplicate_lines, &library)
            .expect_err("copy limit spans duplicate entries")
            .cause,
        DeckLoadCause::CopyLimitExceeded { copies: 3, .. }
    ));

    let wrong_count = custom_deck(
        "foundations/warden-initiate",
        requirements,
        &cards(&[("foundations/ember-lance", 2)]),
    );
    assert_eq!(
        parse_deck(&wrong_count, &library)
            .expect_err("body must have exactly 20 cards")
            .cause,
        DeckLoadCause::BodyCardCount {
            expected: 20,
            found: 2,
        }
    );
}

#[test]
fn built_in_catalog_has_exact_recipes_and_shared_allocations() {
    let first = built_in_catalog().expect("built-in catalog must load");
    let second = built_in_catalog().expect("cached catalog must load");
    assert!(Arc::ptr_eq(&first, &second));

    let core_a = first.library().core_cards();
    let core_b = first.library().core_cards();
    assert!(Arc::ptr_eq(&core_a, &core_b));

    let expected = [
        (
            first.set_paths(),
            "foundations/warden-initiate",
            [
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
            ],
        ),
        (
            first.barrow_herd(),
            "foundations/sow-piglet",
            [
                "foundations/sow-matriarch",
                "foundations/old-sow-of-the-barrow",
                "foundations/hearth-warden",
                "foundations/dawn-tender",
                "foundations/barrow-grazer",
                "foundations/ash-shepherd",
                "foundations/barrow-seer",
                "foundations/renewing-balm",
                "foundations/ember-lance",
                "foundations/standing-ward",
            ],
        ),
    ];

    for (deck, starter, keys) in expected {
        assert_eq!(
            deck.starter(),
            first.library().card_id(starter).expect("Starter exists")
        );
        let counts = deck
            .body()
            .iter()
            .copied()
            .fold(BTreeMap::new(), |mut counts, id| {
                *counts.entry(format!("{id:?}")).or_insert(0) += 1;
                counts
            });
        assert_eq!(deck.body().len(), 20);
        assert_eq!(counts.len(), 10);
        assert!(counts.values().all(|count| *count == 2));
        for key in keys {
            let id = first.library().card_id(key).expect("recipe card exists");
            assert_eq!(
                deck.body()
                    .iter()
                    .filter(|candidate| **candidate == id)
                    .count(),
                2
            );
        }
    }
}
