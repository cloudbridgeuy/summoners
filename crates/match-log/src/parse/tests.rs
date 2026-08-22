#![allow(clippy::expect_used)]

use super::*;

#[test]
fn canonical_entity_ids_and_digests_accept_only_wire_text() {
    let context = ParseContext::default();
    assert!(
        validate_entity_id("01234567-89ab-cdef-0123-456789abcdef", "ability", &context).is_ok()
    );
    assert!(validate_entity_id("0123456789abcdef0123456789abcdef", "ability", &context).is_err());
    assert!(
        validate_entity_id("01234567-89AB-CDEF-0123-456789ABCDEF", "ability", &context).is_err()
    );

    let valid = format!("sha256:{}", "a".repeat(64));
    assert!(validate_digest(&valid, "state_digest", &context).is_ok());
    assert!(validate_digest("sha256:ABC", "state_digest", &context).is_err());
}

#[test]
fn pure_decoder_keeps_available_record_context() {
    let error = decode_record(
        br#"{"sequence":4,"record":"action","step":2,"action":{"kind":"end_turn","player":"three"}}"#,
        9,
    )
    .expect_err("the player variant is closed");

    assert_eq!(error.context().line, Some(9));
    assert_eq!(error.context().sequence, Some(4));
    assert_eq!(error.context().step, Some(2));
    assert_eq!(error.context().path.as_deref(), Some("action"));
}

#[test]
fn pure_fold_rejects_empty_input_without_invented_source_context() {
    let error = fold_records(vec![]).expect_err("a complete transcript is required");

    assert_eq!(
        error.kind(),
        &ParseErrorKind::Lifecycle(LifecycleError::Incomplete { expected: "header" })
    );
    assert_eq!(error.context(), &ParseContext::default());
}

#[test]
fn error_display_orders_all_known_context_before_the_fault() {
    let error = ParseError::new(
        ParseErrorKind::Lifecycle(LifecycleError::EventIndex {
            expected: 0,
            found: 2,
        }),
        ParseContext {
            line: Some(7),
            sequence: Some(6),
            step: Some(3),
            event_index: Some(2),
            path: Some("index".to_string()),
        },
    );

    assert_eq!(
        error.to_string(),
        "line 7, sequence 6, step 3, event index 2, path index: expected event index 0, found 2"
    );
}
