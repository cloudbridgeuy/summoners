mod support;

use std::io::BufReader;

use serde_json::{Value, json};
use summoners_match_log::{
    ActionV1, ErrorV1, EventV1, LifecycleError, ParseError, ParseErrorKind, TranscriptStepResultV1,
    TranscriptV1,
};

fn values() -> Vec<Value> {
    std::str::from_utf8(&support::valid_transcript_bytes())
        .expect("the fixture is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("the fixture record is JSON"))
        .collect()
}

fn encode(values: &[Value]) -> Vec<u8> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut bytes = values
        .iter()
        .map(|value| serde_json::to_string(value).expect("the mutation is JSON"))
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes();
    bytes.push(b'\n');
    bytes
}

fn record_index(values: &[Value], name: &str) -> usize {
    values
        .iter()
        .position(|value| value["record"] == name)
        .unwrap_or_else(|| panic!("the fixture has a {name} record"))
}

fn record_indexes(values: &[Value], name: &str) -> Vec<usize> {
    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value["record"] == name).then_some(index))
        .collect()
}

fn resequence(values: &mut [Value]) {
    for (sequence, value) in values.iter_mut().enumerate() {
        value["sequence"] = json!(sequence);
    }
}

fn parse_error(values: &[Value]) -> ParseError {
    TranscriptV1::parse(encode(values).as_slice()).expect_err("the mutation is rejected")
}

fn assert_lifecycle(error: &ParseError, expected: &LifecycleError) {
    assert_eq!(error.kind(), &ParseErrorKind::Lifecycle(expected.clone()));
}

#[test]
fn valid_recorded_bytes_and_a_buffered_reader_preserve_all_typed_values() {
    let bytes = support::valid_transcript_bytes();
    let from_bytes = TranscriptV1::parse(bytes.as_slice()).expect("the V2 transcript is complete");
    let from_reader = TranscriptV1::parse(BufReader::new(bytes.as_slice()))
        .expect("the buffered input is complete");

    assert_eq!(from_bytes, from_reader);
    assert_eq!(from_bytes.header.format, "summoners_match");
    assert_eq!(from_bytes.header.format_version, 1);
    assert!(!from_bytes.steps.is_empty());
    assert!(matches!(
        &from_bytes.steps[0].action.action,
        ActionV1::ActivateSkill { ability, targets, mana_hint: Some(_), .. }
            if ability.0 == "01234567-89ab-cdef-0123-456789abcdef" && targets.len() == 1
    ));
    assert!(matches!(
        &from_bytes.steps[0].result,
        TranscriptStepResultV1::Rejected { rejection }
            if rejection.error == ErrorV1::EmptyPosition
    ));
    assert!(from_bytes.steps.iter().any(|step| matches!(
        &step.result,
        TranscriptStepResultV1::Accepted { events, .. }
            if events.iter().any(|event| matches!(event.event, EventV1::GameEnded { .. }))
    )));
    assert_eq!(
        from_bytes.match_completed.step_count,
        from_bytes.steps.len() as u64
    );
    assert_eq!(
        from_bytes.final_state.state_digest,
        from_bytes.match_completed.state_digest
    );
}

#[test]
fn strict_decoding_rejects_invalid_utf8_and_malformed_json() {
    let invalid_utf8 = [b'{', 0xff, b'}', b'\n'];
    let error = TranscriptV1::parse(invalid_utf8.as_slice()).expect_err("UTF-8 is strict");
    assert_eq!(error.kind(), &ParseErrorKind::InvalidUtf8);
    assert_eq!(error.context().line, Some(1));

    let error =
        TranscriptV1::parse(b"{\"record\":\n".as_slice()).expect_err("JSON syntax is strict");
    assert!(matches!(error.kind(), ParseErrorKind::MalformedJson { .. }));
    assert_eq!(error.context().line, Some(1));
}

#[test]
fn strict_decoding_rejects_unknown_missing_and_non_integer_fields() {
    let mut unknown = values();
    unknown[0]["extra"] = json!(true);
    let error = parse_error(&unknown);
    assert!(matches!(error.kind(), ParseErrorKind::InvalidRecord { .. }));
    assert_eq!(error.context().line, Some(1));

    let mut missing = values();
    missing[0]
        .as_object_mut()
        .expect("header is an object")
        .remove("format");
    let error = parse_error(&missing);
    assert!(matches!(error.kind(), ParseErrorKind::InvalidRecord { .. }));

    let mut floating = values();
    let action = record_index(&floating, "action");
    floating[action]["step"] = json!(1.0);
    let error = parse_error(&floating);
    assert!(matches!(error.kind(), ParseErrorKind::InvalidRecord { .. }));
    assert_eq!(error.context().path.as_deref(), Some("step"));
}

#[test]
fn strict_decoding_rejects_bad_ids_variants_digests_formats_and_versions() {
    let mut bad_id = values();
    let action = record_index(&bad_id, "action");
    bad_id[action]["action"]["ability"] = json!("NOT-A-UUID");
    let error = parse_error(&bad_id);
    assert!(matches!(
        error.kind(),
        ParseErrorKind::InvalidEntityId { .. }
    ));
    assert_eq!(error.context().path.as_deref(), Some("action.ability"));

    let mut bare_id = values();
    bare_id[action]["action"]["ability"] = json!("0123456789abcdef0123456789abcdef");
    assert!(matches!(
        parse_error(&bare_id).kind(),
        ParseErrorKind::InvalidEntityId { .. }
    ));

    let mut unknown_variant = values();
    unknown_variant[action]["action"]["kind"] = json!("teleport");
    let error = parse_error(&unknown_variant);
    assert!(matches!(error.kind(), ParseErrorKind::InvalidRecord { .. }));
    assert_eq!(error.context().path.as_deref(), Some("action.kind"));

    let mut bad_digest = values();
    bad_digest[1]["state_digest"] = json!("sha256:ABC");
    let error = parse_error(&bad_digest);
    assert!(matches!(
        error.kind(),
        ParseErrorKind::InvalidStateDigest { .. }
    ));
    assert_eq!(error.context().path.as_deref(), Some("state_digest"));

    let mut format = values();
    format[0]["format"] = json!("other_match");
    assert_eq!(
        parse_error(&format).kind(),
        &ParseErrorKind::UnsupportedFormat {
            found: "other_match".to_string()
        }
    );

    let mut version = values();
    version[0]["format_version"] = json!(2);
    assert_eq!(
        parse_error(&version).kind(),
        &ParseErrorKind::UnsupportedVersion { found: 2 }
    );
}

#[test]
fn header_metadata_remains_open_and_is_not_normatively_validated() {
    let mut input = values();
    input[0]["metadata"] = json!({
        "entity": "diagnostic text",
        "nested": { "anything": [1, 2.5, false] }
    });

    let transcript = TranscriptV1::parse(encode(&input).as_slice()).expect("metadata is open data");
    assert_eq!(transcript.header.metadata["entity"], "diagnostic text");
}

#[test]
fn sequence_gaps_and_repeats_report_the_exact_line_and_sequence() {
    for found in [4, 2] {
        let mut input = values();
        input[3]["sequence"] = json!(found);
        let error = parse_error(&input);
        assert_lifecycle(&error, &LifecycleError::Sequence { expected: 3, found });
        assert_eq!(error.context().line, Some(4));
        assert_eq!(error.context().sequence, Some(found));
        assert_eq!(error.context().path.as_deref(), Some("sequence"));
    }
}

#[test]
fn wrong_step_numbers_report_the_record_step_and_path() {
    let mut input = values();
    let action = record_index(&input, "action");
    input[action]["step"] = json!(9);
    let error = parse_error(&input);
    assert_lifecycle(
        &error,
        &LifecycleError::Step {
            expected: 1,
            found: 9,
        },
    );
    assert_eq!(error.context().step, Some(9));
    assert_eq!(error.context().path.as_deref(), Some("step"));
}

#[test]
fn wrong_event_indexes_and_counts_report_full_available_context() {
    let mut bad_index = values();
    let event = record_index(&bad_index, "event");
    bad_index[event]["index"] = json!(7);
    let error = parse_error(&bad_index);
    assert_lifecycle(
        &error,
        &LifecycleError::EventIndex {
            expected: 0,
            found: 7,
        },
    );
    assert_eq!(error.context().line, Some(event + 1));
    assert_eq!(error.context().sequence, Some(event as u64));
    assert_eq!(error.context().step, bad_index[event]["step"].as_u64());
    assert_eq!(error.context().event_index, Some(7));
    assert_eq!(error.context().path.as_deref(), Some("index"));

    let mut bad_count = values();
    let completion = record_indexes(&bad_count, "step_completed")[1];
    let found = bad_count[completion]["event_count"]
        .as_u64()
        .expect("event count")
        + 1;
    bad_count[completion]["event_count"] = json!(found);
    let error = parse_error(&bad_count);
    assert!(matches!(
        error.kind(),
        ParseErrorKind::Lifecycle(LifecycleError::EventCount { found: actual, .. }) if *actual == found
    ));
    assert_eq!(error.context().path.as_deref(), Some("event_count"));
}

#[test]
fn an_event_without_an_action_and_an_action_without_a_result_are_rejected() {
    let mut no_action = values();
    let event = record_index(&no_action, "event");
    let step = no_action[event]["step"].as_u64().expect("event step");
    let action = no_action[..event]
        .iter()
        .rposition(|value| value["record"] == "action" && value["step"] == step)
        .expect("event action");
    no_action.remove(action);
    resequence(&mut no_action);
    let error = parse_error(&no_action);
    assert_lifecycle(&error, &LifecycleError::EventWithoutAction);

    let mut no_result = values();
    let actions = record_indexes(&no_result, "action");
    let second_action = actions[1];
    no_result.remove(second_action - 1);
    resequence(&mut no_result);
    let error = parse_error(&no_result);
    assert_lifecycle(&error, &LifecycleError::ActionBeforeStepResult);
}

#[test]
fn rejected_steps_require_no_events_and_the_prior_digest() {
    let mut has_event = values();
    let rejected = record_index(&has_event, "step_rejected");
    let mut event = has_event[record_index(&has_event, "event")].clone();
    event["step"] = has_event[rejected]["step"].clone();
    event["index"] = json!(0);
    has_event.insert(rejected, event);
    resequence(&mut has_event);
    let error = parse_error(&has_event);
    assert_lifecycle(&error, &LifecycleError::RejectedStepHasEvents);

    let mut digest = values();
    let rejected = record_index(&digest, "step_rejected");
    digest[rejected]["state_digest"] =
        json!("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let error = parse_error(&digest);
    assert_lifecycle(&error, &LifecycleError::RejectedDigestMismatch);
    assert_eq!(error.context().path.as_deref(), Some("state_digest"));
}

#[test]
fn two_terminal_events_and_records_after_the_terminal_result_are_rejected() {
    let mut duplicate = values();
    let game_ended = duplicate
        .iter()
        .position(|value| value["event"]["kind"] == "game_ended")
        .expect("terminal event");
    let completion = duplicate[game_ended..]
        .iter()
        .position(|value| value["record"] == "step_completed")
        .map(|offset| offset + game_ended)
        .expect("terminal completion");
    let mut copy = duplicate[game_ended].clone();
    copy["index"] = json!(
        duplicate[completion]["event_count"]
            .as_u64()
            .expect("terminal event count")
    );
    duplicate.insert(completion, copy);
    duplicate[completion + 1]["event_count"] = json!(
        duplicate[completion + 1]["event_count"]
            .as_u64()
            .expect("terminal event count")
            + 1
    );
    let completed = record_index(&duplicate, "match_completed");
    duplicate[completed]["event_count"] = json!(
        duplicate[completed]["event_count"]
            .as_u64()
            .expect("total event count")
            + 1
    );
    resequence(&mut duplicate);
    let error = parse_error(&duplicate);
    assert_lifecycle(&error, &LifecycleError::MultipleGameEnded);

    let mut after_terminal = values();
    let final_state = record_index(&after_terminal, "final_state");
    let mut action = after_terminal[record_index(&after_terminal, "action")].clone();
    action["step"] = json!(99);
    after_terminal.insert(final_state, action);
    resequence(&mut after_terminal);
    let error = parse_error(&after_terminal);
    assert_lifecycle(&error, &LifecycleError::RecordAfterTerminalStep);
}

#[test]
fn records_after_completion_and_missing_game_ended_are_rejected() {
    let mut after_completion = values();
    let mut extra = after_completion[0].clone();
    extra["sequence"] = json!(after_completion.len());
    after_completion.push(extra);
    let error = parse_error(&after_completion);
    assert_lifecycle(&error, &LifecycleError::RecordAfterCompletion);

    let mut missing = values();
    let game_ended = missing
        .iter()
        .position(|value| value["event"]["kind"] == "game_ended")
        .expect("terminal event");
    let terminal_step = missing[game_ended]["step"].as_u64().expect("terminal step");
    missing.remove(game_ended);
    for value in &mut missing {
        if value["step"] == terminal_step && value["record"] == "step_completed" {
            value["event_count"] =
                json!(value["event_count"].as_u64().expect("terminal count") - 1);
        }
        if value["record"] == "match_completed" {
            value["event_count"] = json!(value["event_count"].as_u64().expect("total count") - 1);
        }
    }
    resequence(&mut missing);
    let error = parse_error(&missing);
    assert_lifecycle(&error, &LifecycleError::MissingGameEnded);
}

#[test]
fn final_and_completion_outcome_digest_and_count_inconsistencies_are_rejected() {
    let mut outcome = values();
    let final_state = record_index(&outcome, "final_state");
    outcome[final_state]["final_state"]["status"]["outcome"]["winner"] = json!("two");
    let error = parse_error(&outcome);
    assert_lifecycle(&error, &LifecycleError::OutcomeMismatch);

    let mut completion_reason = values();
    let completed = record_index(&completion_reason, "match_completed");
    completion_reason[completed]["reason"] = json!("third_main_loss");
    let error = parse_error(&completion_reason);
    assert_lifecycle(&error, &LifecycleError::OutcomeMismatch);
    assert_eq!(error.context().path.as_deref(), Some("reason"));

    let mut steps = values();
    let completed = record_index(&steps, "match_completed");
    let expected = steps[completed]["step_count"].as_u64().expect("step count");
    steps[completed]["step_count"] = json!(expected + 1);
    assert_lifecycle(
        &parse_error(&steps),
        &LifecycleError::CompletionStepCount {
            expected,
            found: expected + 1,
        },
    );

    let mut events = values();
    let expected = events[completed]["event_count"]
        .as_u64()
        .expect("event count");
    events[completed]["event_count"] = json!(expected + 1);
    assert_lifecycle(
        &parse_error(&events),
        &LifecycleError::CompletionEventCount {
            expected,
            found: expected + 1,
        },
    );

    let mut digest = values();
    digest[completed]["state_digest"] =
        json!("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert_lifecycle(
        &parse_error(&digest),
        &LifecycleError::CompletionDigestMismatch,
    );
}

#[test]
fn every_valid_record_boundary_is_an_incomplete_transcript() {
    let input = values();
    for boundary in 0..input.len() {
        let error = parse_error(&input[..boundary]);
        assert!(
            matches!(
                error.kind(),
                ParseErrorKind::Lifecycle(LifecycleError::Incomplete { .. })
            ),
            "boundary {boundary}: {error}"
        );
        assert_eq!(error.context().line, None);
    }
}

#[test]
fn removing_or_moving_one_line_reports_the_exact_lifecycle_fault() {
    let mut removed = values();
    removed.remove(2);
    let error = parse_error(&removed);
    assert_lifecycle(
        &error,
        &LifecycleError::Sequence {
            expected: 2,
            found: 3,
        },
    );
    assert_eq!(error.context().line, Some(3));

    let mut moved = values();
    moved.swap(2, 3);
    let error = parse_error(&moved);
    assert_lifecycle(
        &error,
        &LifecycleError::Sequence {
            expected: 2,
            found: 3,
        },
    );
    assert_eq!(error.context().line, Some(3));
}
