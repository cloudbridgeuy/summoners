use super::*;

fn assert_decode_error(error: summoners_cards::SetLoadError, path: &str) {
    let summoners_cards::SetLoadError {
        phase,
        schema_version,
        path: actual_path,
        cause,
        ..
    } = error;
    assert_eq!(phase, LoadPhase::Decode);
    assert_eq!(schema_version, Some(1));
    assert_eq!(actual_path, path);
    assert!(matches!(cause, SetLoadCause::SchemaDecode { .. }));
}

fn sourced_card(kind: &str, persistence: &str) -> String {
    format!(
        r#"
schema_version = 1
id = "source-test"
revision = 1
name = "Source Test"

[[cards]]
code = "source-card"
name = "Source Card"
kind = "{kind}"
timing = "support"
persistence = "{persistence}"
cost = []

[[cards.effects]]
kind = "heal"
target = "source"
amount = 10
"#
    )
}

#[test]
fn public_parser_rejects_unknown_nested_fields_and_enum_variants() {
    let unknown_nested = summon_block("alpha", "Alpha", "Text", 10)
        .replace("base = 10", "base = 10\nunknown_nested = true");
    let error = parse_set(set_document(1, "Set", &[&unknown_nested]).as_bytes()).unwrap_err();
    assert_decode_error(error, "cards[0].abilities[0].effects[0]");

    let unknown_variant = summon_block("alpha", "Alpha", "Text", 10)
        .replace("kind = \"summon\"", "kind = \"artifact\"");
    let error = parse_set(set_document(1, "Set", &[&unknown_variant]).as_bytes()).unwrap_err();
    assert_decode_error(error, "cards[0].kind");
}

#[test]
fn public_parser_rejects_source_targets_for_spells_and_enchantments() {
    for (kind, persistence) in [("spell", "discard"), ("enchantment", "persistent")] {
        let error = parse_set(sourced_card(kind, persistence).as_bytes()).unwrap_err();
        assert_eq!(error.phase, LoadPhase::Semantics);
        assert_eq!(error.schema_version, Some(1));
        assert_eq!(error.path, "cards[0].effects[0].target");
        assert_eq!(
            error.cause,
            SetLoadCause::InvalidSemantics {
                rule: SemanticRule::EffectTarget,
            }
        );
    }
}
