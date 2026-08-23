mod support;

use serde_json::{Value, json};
use summoners_match_log::{
    TranscriptStepResultV1, TranscriptV1,
    compare::{
        ComparisonOptions, TranscriptComparison, TranscriptComparisonError,
        compare_parsed_transcripts, compare_transcripts,
    },
    wire::{ErrorV1, EventV1, LossReasonV1, ManaBankV1, PlayerIdV1, PositionV1, SetRequirementV1},
};

fn parsed_fixture() -> TranscriptV1 {
    TranscriptV1::parse(support::valid_transcript_bytes().as_slice()).expect("valid fixture")
}

fn difference(
    comparison: TranscriptComparison,
) -> summoners_match_log::compare::TranscriptDifference {
    match comparison {
        TranscriptComparison::Equal => panic!("expected a transcript difference"),
        TranscriptComparison::Different(difference) => difference,
    }
}

fn parsed_difference(
    expected: &TranscriptV1,
    actual: &TranscriptV1,
) -> summoners_match_log::compare::TranscriptDifference {
    difference(
        compare_parsed_transcripts(expected, actual, ComparisonOptions::default())
            .expect("typed values serialize"),
    )
}

#[test]
fn input_comparison_ignores_json_layout_and_metadata_by_default() {
    let expected = support::valid_transcript_bytes();
    let mut records: Vec<Value> = String::from_utf8(expected.clone())
        .expect("fixture is UTF-8")
        .lines()
        .map(|line| serde_json::from_str(line).expect("fixture record is JSON"))
        .collect();
    records[0]["metadata"] = json!({"shell": "different", "nested": {"b": 2, "a": 1}});
    let actual = records
        .iter()
        .map(|record| {
            format!(
                "  {}  \n",
                serde_json::to_string(record).expect("record serializes")
            )
        })
        .collect::<String>();

    assert_eq!(
        compare_transcripts(
            expected.as_slice(),
            actual.as_bytes(),
            ComparisonOptions::default(),
        )
        .expect("both inputs parse"),
        TranscriptComparison::Equal
    );
}

#[test]
fn metadata_is_normative_only_when_requested() {
    let expected = parsed_fixture();
    let mut actual = expected.clone();
    actual
        .header
        .metadata
        .insert("shell".to_string(), json!("cli"));

    let difference = difference(
        compare_parsed_transcripts(
            &expected,
            &actual,
            ComparisonOptions {
                include_header_metadata: true,
            },
        )
        .expect("typed values serialize"),
    );
    assert_eq!(difference.sequence, 0);
    assert_eq!(difference.path.as_str(), "header.metadata.shell");
    assert_eq!(difference.expected, None);
    assert_eq!(difference.actual, Some(json!("cli")));
}

#[test]
fn parse_failures_identify_the_input_side() {
    let valid = support::valid_transcript_bytes();
    let expected_error = compare_transcripts(
        b"not json".as_slice(),
        valid.as_slice(),
        ComparisonOptions::default(),
    )
    .expect_err("expected input must fail");
    assert!(matches!(
        expected_error,
        TranscriptComparisonError::ExpectedParse(_)
    ));

    let actual_error = compare_transcripts(
        valid.as_slice(),
        b"not json".as_slice(),
        ComparisonOptions::default(),
    )
    .expect_err("actual input must fail");
    assert!(matches!(
        actual_error,
        TranscriptComparisonError::ActualParse(_)
    ));
}

#[test]
fn every_top_level_record_reports_its_first_changed_field() {
    let baseline = parsed_fixture();

    let mut actual = baseline.clone();
    actual.header.format = "other".to_string();
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "header.format"
    );

    let mut expected = baseline.clone();
    let mut actual = baseline.clone();
    expected.match_created.required_sets.push(SetRequirementV1 {
        set: "core".to_string(),
        revision: 1,
    });
    actual.match_created.required_sets.push(SetRequirementV1 {
        set: "core".to_string(),
        revision: 2,
    });
    assert_eq!(
        parsed_difference(&expected, &actual).path.as_str(),
        "match_created.required_sets[0].revision"
    );

    let mut actual = baseline.clone();
    if let summoners_match_log::ActionV1::ActivateSkill { player, .. } =
        &mut actual.steps[0].action.action
    {
        *player = PlayerIdV1::Two;
    } else {
        panic!("fixture starts with ActivateSkill");
    }
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "steps[0].action.action.player"
    );

    let mut actual = baseline.clone();
    let (event_sequence, event_step, event_index) = match &mut actual.steps[3].result {
        TranscriptStepResultV1::Accepted { events, .. } => {
            let event = events.first_mut().expect("terminal step has an event");
            event.event = EventV1::PrizesViewed {
                player: PlayerIdV1::One,
                prizes: vec![2, 1],
            };
            (event.sequence, event.step, event.index)
        }
        TranscriptStepResultV1::Rejected { .. } => panic!("terminal step is accepted"),
    };
    let mut expected = actual.clone();
    if let TranscriptStepResultV1::Accepted { events, .. } = &mut expected.steps[3].result {
        events[0].event = EventV1::PrizesViewed {
            player: PlayerIdV1::One,
            prizes: vec![1, 2],
        };
    }
    let event_difference = parsed_difference(&expected, &actual);
    assert_eq!(event_difference.sequence, event_sequence);
    assert_eq!(event_difference.step, Some(event_step));
    assert_eq!(event_difference.event_index, Some(event_index));
    assert_eq!(
        event_difference.path.as_str(),
        "steps[3].result.events[0].event.prizes[0]"
    );

    let mut actual = baseline.clone();
    if let TranscriptStepResultV1::Accepted { completion, .. } = &mut actual.steps[1].result {
        completion.state_digest.0 = format!("sha256:{}", "a".repeat(64));
    } else {
        panic!("second fixture step is accepted");
    }
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "steps[1].result.completion.state_digest"
    );

    let mut actual = baseline.clone();
    if let TranscriptStepResultV1::Rejected { rejection } = &mut actual.steps[0].result {
        rejection.error = ErrorV1::WrongPhase;
    } else {
        panic!("first fixture step is rejected");
    }
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "steps[0].result.rejection.error.kind"
    );

    let mut actual = baseline.clone();
    actual.final_state.final_state.coin = !actual.final_state.final_state.coin;
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "final_state.final_state.coin"
    );

    let mut actual = baseline.clone();
    actual.match_completed.step_count += 1;
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "match_completed.step_count"
    );
}

#[test]
fn action_arrays_and_event_order_remain_exact() {
    let baseline = parsed_fixture();
    let mut expected = baseline.clone();
    let mut actual = baseline.clone();
    if let summoners_match_log::ActionV1::ActivateSkill { targets, .. } =
        &mut expected.steps[0].action.action
    {
        *targets = vec![
            PositionV1::Main,
            PositionV1::Bench {
                slot: summoners_match_log::wire::BenchSlotV1::First,
            },
        ];
    }
    if let summoners_match_log::ActionV1::ActivateSkill { targets, .. } =
        &mut actual.steps[0].action.action
    {
        *targets = vec![
            PositionV1::Bench {
                slot: summoners_match_log::wire::BenchSlotV1::First,
            },
            PositionV1::Main,
        ];
    }
    assert_eq!(
        parsed_difference(&expected, &actual).path.as_str(),
        "steps[0].action.action.targets[0].kind"
    );

    let mut expected = baseline.clone();
    let mut actual = baseline.clone();
    let TranscriptStepResultV1::Accepted { events, .. } = &mut expected.steps[3].result else {
        panic!("terminal step is accepted");
    };
    let first = events.first_mut().expect("terminal event exists");
    first.event = EventV1::PriorityPassed {
        player: PlayerIdV1::One,
    };
    let TranscriptStepResultV1::Accepted { events, .. } = &mut actual.steps[3].result else {
        panic!("terminal step is accepted");
    };
    let first = events.first_mut().expect("terminal event exists");
    first.event = EventV1::TurnBegan {
        player: PlayerIdV1::One,
    };
    assert_eq!(
        parsed_difference(&expected, &actual).path.as_str(),
        "steps[3].result.events[0].event.kind"
    );
}

#[test]
fn typed_error_state_digest_and_completion_fields_have_exact_paths() {
    let baseline = parsed_fixture();

    let mut expected = baseline.clone();
    let mut actual = baseline.clone();
    let expected_rejection = match &mut expected.steps[0].result {
        TranscriptStepResultV1::Rejected { rejection } => rejection,
        TranscriptStepResultV1::Accepted { .. } => panic!("first fixture step is rejected"),
    };
    expected_rejection.error = ErrorV1::InsufficientMana {
        short: ManaBankV1 {
            matter: 1,
            mind: 2,
            spirit: 3,
        },
    };
    let actual_rejection = match &mut actual.steps[0].result {
        TranscriptStepResultV1::Rejected { rejection } => rejection,
        TranscriptStepResultV1::Accepted { .. } => panic!("first fixture step is rejected"),
    };
    actual_rejection.error = ErrorV1::InsufficientMana {
        short: ManaBankV1 {
            matter: 1,
            mind: 9,
            spirit: 3,
        },
    };
    assert_eq!(
        parsed_difference(&expected, &actual).path.as_str(),
        "steps[0].result.rejection.error.short.mind"
    );

    let mut actual = baseline.clone();
    actual.final_state.final_state.players.one.mana.mind += 1;
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "final_state.final_state.players.one.mana.mind"
    );

    let mut actual = baseline.clone();
    actual.final_state.state_digest.0 = format!("sha256:{}", "b".repeat(64));
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "final_state.state_digest"
    );

    let mut actual = baseline.clone();
    actual.match_completed.event_count += 1;
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "match_completed.event_count"
    );

    let mut actual = baseline.clone();
    actual.match_completed.state_digest.0 = format!("sha256:{}", "c".repeat(64));
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "match_completed.state_digest"
    );

    let mut actual = baseline.clone();
    actual.match_completed.winner = PlayerIdV1::Two;
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "match_completed.winner"
    );

    let mut actual = baseline.clone();
    actual.match_completed.reason = LossReasonV1::ThirdMainLoss;
    assert_eq!(
        parsed_difference(&baseline, &actual).path.as_str(),
        "match_completed.reason"
    );
}

#[test]
fn first_difference_follows_record_then_field_order() {
    let expected = parsed_fixture();
    let mut actual = expected.clone();
    actual.header.format = "other".to_string();
    actual.final_state.final_state.coin = !actual.final_state.final_state.coin;

    let difference = parsed_difference(&expected, &actual);
    assert_eq!(difference.sequence, 0);
    assert_eq!(difference.path.as_str(), "header.format");
    assert_eq!(difference.expected, Some(json!("summoners_match")));
    assert_eq!(difference.actual, Some(json!("other")));
}
