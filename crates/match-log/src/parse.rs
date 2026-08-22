//! Strict complete-transcript parsing.

use std::io::BufRead;

use serde::de::DeserializeOwned;
use serde_json::Value;
use summoners_core::domain::cards::EntityId;

use crate::{
    parse_error::{LifecycleError, ParseContext, ParseError, ParseErrorKind},
    state::StateDigestV1,
    wire::{
        ActionRecordV1, EventRecordV1, EventV1, FinalStateV1, GameOutcomeV1, GameStatusV1,
        HeaderV1, MatchCompletedV1, MatchCreatedV1, RecordV1, StepCompletedV1, StepRejectedV1,
    },
};

/// The result of one submitted action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptStepResultV1 {
    Accepted {
        events: Vec<EventRecordV1>,
        completion: StepCompletedV1,
    },
    Rejected {
        rejection: StepRejectedV1,
    },
}

/// One normalized action and its complete recorded result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptStepV1 {
    pub action: ActionRecordV1,
    pub result: TranscriptStepResultV1,
}

/// A strictly decoded, complete match transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptV1 {
    pub header: HeaderV1,
    pub match_created: MatchCreatedV1,
    pub steps: Vec<TranscriptStepV1>,
    pub final_state: FinalStateV1,
    pub match_completed: MatchCompletedV1,
}

impl TranscriptV1 {
    /// Parse strict NDJSON from caller-held bytes or any buffered reader.
    pub fn parse(reader: impl BufRead) -> Result<Self, ParseError> {
        parse_reader(reader)
    }
}

#[derive(Debug)]
struct DecodedRecord {
    line: usize,
    record: RecordV1,
}

fn parse_reader(mut reader: impl BufRead) -> Result<TranscriptV1, ParseError> {
    let mut records = Vec::new();
    let mut line_number = 1_usize;
    loop {
        let mut bytes = Vec::new();
        let read = reader.read_until(b'\n', &mut bytes).map_err(|error| {
            ParseError::new(
                ParseErrorKind::Read {
                    message: error.to_string(),
                },
                ParseContext {
                    line: Some(line_number),
                    ..ParseContext::default()
                },
            )
        })?;
        if read == 0 {
            break;
        }
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        records.push(decode_record(&bytes, line_number)?);
        line_number += 1;
    }
    fold_records(records)
}

fn decode_record(bytes: &[u8], line: usize) -> Result<DecodedRecord, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        ParseError::new(
            ParseErrorKind::InvalidUtf8,
            ParseContext {
                line: Some(line),
                ..ParseContext::default()
            },
        )
    })?;
    let value: Value = serde_json::from_str(text).map_err(|error| {
        ParseError::new(
            ParseErrorKind::MalformedJson {
                message: error.to_string(),
            },
            ParseContext {
                line: Some(line),
                ..ParseContext::default()
            },
        )
    })?;
    let base_context = context_from_value(line, &value);
    let tag = value.get("record").and_then(Value::as_str).ok_or_else(|| {
        invalid_record(&base_context, "record", "missing or invalid record field")
    })?;
    if tag != "header" {
        validate_canonical_values(&value, "", &base_context)?;
    }

    let record = match tag {
        "header" => {
            let header: HeaderV1 = decode_typed(text, &base_context)?;
            if header.format != "summoners_match" {
                return Err(ParseError::new(
                    ParseErrorKind::UnsupportedFormat {
                        found: header.format,
                    },
                    with_path(&base_context, "format"),
                ));
            }
            if header.format_version != 1 {
                return Err(ParseError::new(
                    ParseErrorKind::UnsupportedVersion {
                        found: header.format_version,
                    },
                    with_path(&base_context, "format_version"),
                ));
            }
            RecordV1::Header(header)
        }
        "match_created" => RecordV1::MatchCreated(Box::new(decode_typed(text, &base_context)?)),
        "action" => RecordV1::Action(decode_typed(text, &base_context)?),
        "event" => RecordV1::Event(decode_typed(text, &base_context)?),
        "step_completed" => RecordV1::StepCompleted(decode_typed(text, &base_context)?),
        "step_rejected" => RecordV1::StepRejected(decode_typed(text, &base_context)?),
        "final_state" => RecordV1::FinalState(Box::new(decode_typed(text, &base_context)?)),
        "match_completed" => RecordV1::MatchCompleted(decode_typed(text, &base_context)?),
        unknown => {
            return Err(invalid_record(
                &base_context,
                "record",
                &format!("unknown record variant `{unknown}`"),
            ));
        }
    };
    Ok(DecodedRecord { line, record })
}

fn decode_typed<T: DeserializeOwned>(text: &str, context: &ParseContext) -> Result<T, ParseError> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let path = error.path().to_string();
        ParseError::new(
            ParseErrorKind::InvalidRecord {
                message: error.inner().to_string(),
            },
            if path == "." {
                context.clone()
            } else {
                with_path(context, &path)
            },
        )
    })
}

fn context_from_value(line: usize, value: &Value) -> ParseContext {
    ParseContext {
        line: Some(line),
        sequence: value.get("sequence").and_then(Value::as_u64),
        step: value.get("step").and_then(Value::as_u64),
        event_index: value.get("index").and_then(Value::as_u64),
        path: None,
    }
}

fn with_path(context: &ParseContext, path: &str) -> ParseContext {
    ParseContext {
        path: Some(path.trim_start_matches('.').to_string()),
        ..context.clone()
    }
}

fn invalid_record(context: &ParseContext, path: &str, message: &str) -> ParseError {
    ParseError::new(
        ParseErrorKind::InvalidRecord {
            message: message.to_string(),
        },
        with_path(context, path),
    )
}

fn validate_canonical_values(
    value: &Value,
    path: &str,
    context: &ParseContext,
) -> Result<(), ParseError> {
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                let child_path = join_path(path, name);
                if matches!(name.as_str(), "ability" | "definition" | "entity") {
                    if let Some(text) = child.as_str() {
                        validate_entity_id(text, &child_path, context)?;
                    }
                } else if name == "state_digest"
                    && let Some(text) = child.as_str()
                {
                    validate_digest(text, &child_path, context)?;
                }
                validate_canonical_values(child, &child_path, context)?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                validate_canonical_values(child, &format!("{path}[{index}]"), context)?;
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
    Ok(())
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}.{child}")
    }
}

fn validate_entity_id(value: &str, path: &str, context: &ParseContext) -> Result<(), ParseError> {
    let canonical = EntityId::parse(value)
        .map(|id| id.to_string())
        .is_ok_and(|canonical| canonical == value);
    if canonical {
        Ok(())
    } else {
        Err(ParseError::new(
            ParseErrorKind::InvalidEntityId {
                value: value.to_string(),
            },
            with_path(context, path),
        ))
    }
}

fn validate_digest(value: &str, path: &str, context: &ParseContext) -> Result<(), ParseError> {
    let valid = value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(ParseError::new(
            ParseErrorKind::InvalidStateDigest {
                value: value.to_string(),
            },
            with_path(context, path),
        ))
    }
}

#[derive(Debug)]
enum Phase {
    Header,
    MatchCreated,
    Ready,
    Pending(PendingStep),
    Terminal(GameOutcomeV1),
    Final(GameOutcomeV1),
    Complete,
}

#[derive(Debug)]
struct PendingStep {
    action: ActionRecordV1,
    events: Vec<EventRecordV1>,
    game_ended: Option<GameOutcomeV1>,
}

#[derive(Default)]
struct TranscriptBuilder {
    header: Option<HeaderV1>,
    match_created: Option<MatchCreatedV1>,
    steps: Vec<TranscriptStepV1>,
    final_state: Option<FinalStateV1>,
    match_completed: Option<MatchCompletedV1>,
    next_step: u64,
    event_count: u64,
    digest: Option<StateDigestV1>,
}

fn fold_records(records: Vec<DecodedRecord>) -> Result<TranscriptV1, ParseError> {
    let mut phase = Phase::Header;
    let mut builder = TranscriptBuilder {
        next_step: 1,
        ..TranscriptBuilder::default()
    };
    for (expected_sequence, decoded) in (0_u64..).zip(records) {
        let found_sequence = record_sequence(&decoded.record);
        if found_sequence != expected_sequence {
            return Err(lifecycle_error(
                &decoded,
                "sequence",
                LifecycleError::Sequence {
                    expected: expected_sequence,
                    found: found_sequence,
                },
            ));
        }
        phase = advance(phase, &decoded, &mut builder)?;
    }
    finish(&phase, builder)
}

fn advance(
    phase: Phase,
    decoded: &DecodedRecord,
    builder: &mut TranscriptBuilder,
) -> Result<Phase, ParseError> {
    match (phase, &decoded.record) {
        (Phase::Header, RecordV1::Header(header)) => {
            builder.header = Some(header.clone());
            Ok(Phase::MatchCreated)
        }
        (Phase::Header, record) => Err(unexpected(decoded, "header", record)),
        (Phase::MatchCreated, RecordV1::MatchCreated(created)) => {
            validate_initial_state(decoded, created)?;
            builder.digest = Some(created.state_digest.clone());
            builder.match_created = Some((**created).clone());
            Ok(Phase::Ready)
        }
        (Phase::MatchCreated, record) => Err(unexpected(decoded, "match_created", record)),
        (Phase::Ready, RecordV1::Action(action)) => {
            validate_step(decoded, builder.next_step, action.step)?;
            Ok(Phase::Pending(PendingStep {
                action: action.clone(),
                events: Vec::new(),
                game_ended: None,
            }))
        }
        (Phase::Ready, RecordV1::Event(_)) => Err(lifecycle_error(
            decoded,
            "record",
            LifecycleError::EventWithoutAction,
        )),
        (Phase::Ready, RecordV1::FinalState(_)) => Err(lifecycle_error(
            decoded,
            "record",
            LifecycleError::MissingGameEnded,
        )),
        (Phase::Ready, record) => Err(unexpected(decoded, "action", record)),
        (Phase::Pending(mut pending), RecordV1::Event(event)) => {
            validate_step(decoded, pending.action.step, event.step)?;
            let expected_index = pending.events.len() as u64;
            if event.index != expected_index {
                return Err(lifecycle_error(
                    decoded,
                    "index",
                    LifecycleError::EventIndex {
                        expected: expected_index,
                        found: event.index,
                    },
                ));
            }
            if let EventV1::GameEnded { winner, reason } = event.event {
                if pending.game_ended.is_some() {
                    return Err(lifecycle_error(
                        decoded,
                        "event.kind",
                        LifecycleError::MultipleGameEnded,
                    ));
                }
                pending.game_ended = Some(GameOutcomeV1 { winner, reason });
            }
            pending.events.push(event.clone());
            Ok(Phase::Pending(pending))
        }
        (Phase::Pending(pending), RecordV1::StepCompleted(completion)) => {
            validate_step(decoded, pending.action.step, completion.step)?;
            let expected_count = pending.events.len() as u64;
            if completion.event_count != expected_count {
                return Err(lifecycle_error(
                    decoded,
                    "event_count",
                    LifecycleError::EventCount {
                        expected: expected_count,
                        found: completion.event_count,
                    },
                ));
            }
            builder.event_count += expected_count;
            builder.digest = Some(completion.state_digest.clone());
            let outcome = pending.game_ended;
            builder.steps.push(TranscriptStepV1 {
                action: pending.action,
                result: TranscriptStepResultV1::Accepted {
                    events: pending.events,
                    completion: completion.clone(),
                },
            });
            builder.next_step += 1;
            Ok(outcome.map_or(Phase::Ready, Phase::Terminal))
        }
        (Phase::Pending(pending), RecordV1::StepRejected(rejection)) => {
            validate_step(decoded, pending.action.step, rejection.step)?;
            if !pending.events.is_empty() {
                return Err(lifecycle_error(
                    decoded,
                    "record",
                    LifecycleError::RejectedStepHasEvents,
                ));
            }
            if builder.digest.as_ref() != Some(&rejection.state_digest) {
                return Err(lifecycle_error(
                    decoded,
                    "state_digest",
                    LifecycleError::RejectedDigestMismatch,
                ));
            }
            builder.steps.push(TranscriptStepV1 {
                action: pending.action,
                result: TranscriptStepResultV1::Rejected {
                    rejection: rejection.clone(),
                },
            });
            builder.next_step += 1;
            Ok(Phase::Ready)
        }
        (Phase::Pending(_), RecordV1::Action(_)) => Err(lifecycle_error(
            decoded,
            "record",
            LifecycleError::ActionBeforeStepResult,
        )),
        (Phase::Pending(_), record) => Err(unexpected(decoded, "event or step result", record)),
        (Phase::Terminal(outcome), RecordV1::FinalState(final_state)) => {
            validate_final_state(decoded, final_state, outcome, builder.digest.as_ref())?;
            builder.final_state = Some((**final_state).clone());
            Ok(Phase::Final(outcome))
        }
        (Phase::Terminal(_), _) => Err(lifecycle_error(
            decoded,
            "record",
            LifecycleError::RecordAfterTerminalStep,
        )),
        (Phase::Final(outcome), RecordV1::MatchCompleted(completed)) => {
            validate_completion(decoded, completed, outcome, builder)?;
            builder.match_completed = Some(completed.clone());
            Ok(Phase::Complete)
        }
        (Phase::Final(_), record) => Err(unexpected(decoded, "match_completed", record)),
        (Phase::Complete, _) => Err(lifecycle_error(
            decoded,
            "record",
            LifecycleError::RecordAfterCompletion,
        )),
    }
}

fn validate_initial_state(
    decoded: &DecodedRecord,
    created: &MatchCreatedV1,
) -> Result<(), ParseError> {
    if !matches!(created.initial_state.status, GameStatusV1::Playing) {
        return Err(lifecycle_error(
            decoded,
            "initial_state.status",
            LifecycleError::InvalidInitialStatus,
        ));
    }
    let computed = StateDigestV1::compute(&created.initial_state).map_err(|error| {
        invalid_record(
            &record_context(decoded, None),
            "initial_state",
            &error.to_string(),
        )
    })?;
    if computed != created.state_digest {
        return Err(lifecycle_error(
            decoded,
            "state_digest",
            LifecycleError::InitialStateDigestMismatch,
        ));
    }
    Ok(())
}

fn validate_final_state(
    decoded: &DecodedRecord,
    final_state: &FinalStateV1,
    terminal_outcome: GameOutcomeV1,
    authoritative_digest: Option<&StateDigestV1>,
) -> Result<(), ParseError> {
    let final_outcome = match final_state.final_state.status {
        GameStatusV1::Ended { outcome } => outcome,
        GameStatusV1::Playing | GameStatusV1::Broken { .. } => {
            return Err(lifecycle_error(
                decoded,
                "final_state.status",
                LifecycleError::FinalStateNotEnded,
            ));
        }
    };
    if final_outcome != terminal_outcome {
        return Err(lifecycle_error(
            decoded,
            "final_state.status.outcome",
            LifecycleError::OutcomeMismatch,
        ));
    }
    let computed = StateDigestV1::compute(&final_state.final_state).map_err(|error| {
        invalid_record(
            &record_context(decoded, None),
            "final_state",
            &error.to_string(),
        )
    })?;
    if authoritative_digest != Some(&final_state.state_digest)
        || computed != final_state.state_digest
    {
        return Err(lifecycle_error(
            decoded,
            "state_digest",
            LifecycleError::FinalStateDigestMismatch,
        ));
    }
    Ok(())
}

fn validate_completion(
    decoded: &DecodedRecord,
    completed: &MatchCompletedV1,
    terminal_outcome: GameOutcomeV1,
    builder: &TranscriptBuilder,
) -> Result<(), ParseError> {
    let expected_steps = builder.steps.len() as u64;
    if completed.step_count != expected_steps {
        return Err(lifecycle_error(
            decoded,
            "step_count",
            LifecycleError::CompletionStepCount {
                expected: expected_steps,
                found: completed.step_count,
            },
        ));
    }
    if completed.event_count != builder.event_count {
        return Err(lifecycle_error(
            decoded,
            "event_count",
            LifecycleError::CompletionEventCount {
                expected: builder.event_count,
                found: completed.event_count,
            },
        ));
    }
    if builder.digest.as_ref() != Some(&completed.state_digest) {
        return Err(lifecycle_error(
            decoded,
            "state_digest",
            LifecycleError::CompletionDigestMismatch,
        ));
    }
    if completed.winner != terminal_outcome.winner || completed.reason != terminal_outcome.reason {
        return Err(lifecycle_error(
            decoded,
            "winner",
            LifecycleError::OutcomeMismatch,
        ));
    }
    Ok(())
}

fn validate_step(decoded: &DecodedRecord, expected: u64, found: u64) -> Result<(), ParseError> {
    if found == expected {
        Ok(())
    } else {
        Err(lifecycle_error(
            decoded,
            "step",
            LifecycleError::Step { expected, found },
        ))
    }
}

fn finish(phase: &Phase, builder: TranscriptBuilder) -> Result<TranscriptV1, ParseError> {
    if !matches!(phase, Phase::Complete) {
        return Err(ParseError::new(
            ParseErrorKind::Lifecycle(LifecycleError::Incomplete {
                expected: expected_after(phase),
            }),
            ParseContext::default(),
        ));
    }
    let TranscriptBuilder {
        header,
        match_created,
        steps,
        final_state,
        match_completed,
        ..
    } = builder;
    match (header, match_created, final_state, match_completed) {
        (Some(header), Some(match_created), Some(final_state), Some(match_completed)) => {
            Ok(TranscriptV1 {
                header,
                match_created,
                steps,
                final_state,
                match_completed,
            })
        }
        _ => Err(ParseError::new(
            ParseErrorKind::Lifecycle(LifecycleError::Incomplete {
                expected: "all required records",
            }),
            ParseContext::default(),
        )),
    }
}

fn expected_after(phase: &Phase) -> &'static str {
    match phase {
        Phase::Header => "header",
        Phase::MatchCreated => "match_created",
        Phase::Ready => "the next action and terminal step",
        Phase::Pending(pending) if pending.events.is_empty() => "the step result",
        Phase::Pending(_) => "the remaining events or step result",
        Phase::Terminal(_) => "final_state",
        Phase::Final(_) => "match_completed",
        Phase::Complete => "no more records",
    }
}

fn unexpected(decoded: &DecodedRecord, expected: &'static str, found: &RecordV1) -> ParseError {
    lifecycle_error(
        decoded,
        "record",
        LifecycleError::UnexpectedRecord {
            expected,
            found: record_name(found),
        },
    )
}

fn lifecycle_error(decoded: &DecodedRecord, path: &str, error: LifecycleError) -> ParseError {
    ParseError::new(
        ParseErrorKind::Lifecycle(error),
        record_context(decoded, Some(path)),
    )
}

fn record_context(decoded: &DecodedRecord, path: Option<&str>) -> ParseContext {
    ParseContext {
        line: Some(decoded.line),
        sequence: Some(record_sequence(&decoded.record)),
        step: record_step(&decoded.record),
        event_index: match &decoded.record {
            RecordV1::Event(event) => Some(event.index),
            RecordV1::Header(_)
            | RecordV1::MatchCreated(_)
            | RecordV1::Action(_)
            | RecordV1::StepCompleted(_)
            | RecordV1::StepRejected(_)
            | RecordV1::FinalState(_)
            | RecordV1::MatchCompleted(_) => None,
        },
        path: path.map(str::to_string),
    }
}

fn record_sequence(record: &RecordV1) -> u64 {
    match record {
        RecordV1::Header(value) => value.sequence,
        RecordV1::MatchCreated(value) => value.sequence,
        RecordV1::Action(value) => value.sequence,
        RecordV1::Event(value) => value.sequence,
        RecordV1::StepCompleted(value) => value.sequence,
        RecordV1::StepRejected(value) => value.sequence,
        RecordV1::FinalState(value) => value.sequence,
        RecordV1::MatchCompleted(value) => value.sequence,
    }
}

fn record_step(record: &RecordV1) -> Option<u64> {
    match record {
        RecordV1::Action(value) => Some(value.step),
        RecordV1::Event(value) => Some(value.step),
        RecordV1::StepCompleted(value) => Some(value.step),
        RecordV1::StepRejected(value) => Some(value.step),
        RecordV1::Header(_)
        | RecordV1::MatchCreated(_)
        | RecordV1::FinalState(_)
        | RecordV1::MatchCompleted(_) => None,
    }
}

fn record_name(record: &RecordV1) -> &'static str {
    match record {
        RecordV1::Header(_) => "header",
        RecordV1::MatchCreated(_) => "match_created",
        RecordV1::Action(_) => "action",
        RecordV1::Event(_) => "event",
        RecordV1::StepCompleted(_) => "step_completed",
        RecordV1::StepRejected(_) => "step_rejected",
        RecordV1::FinalState(_) => "final_state",
        RecordV1::MatchCompleted(_) => "match_completed",
    }
}

#[cfg(test)]
mod tests;
