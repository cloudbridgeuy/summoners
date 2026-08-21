use summoners_cards::{DocumentKind, LoadPhase, SetLoadCause, parse_set};

#[test]
fn empty_input_reports_a_typed_set_error() {
    let error = parse_set(b"").expect_err("empty input cannot contain a Set version");
    assert_eq!(error.document, DocumentKind::Set);
    assert_eq!(error.phase, LoadPhase::Version);
    assert_eq!(error.path, "schema_version");
    assert!(matches!(error.cause, SetLoadCause::MissingSchemaVersion));
}
