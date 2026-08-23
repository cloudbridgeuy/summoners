//! Semantic transcript comparison.

use std::{error::Error, fmt, io::BufRead};

use serde::{Deserialize, Serialize, de::MapAccess, de::SeqAccess, de::Visitor};
use serde_json::{Map, Number, Value};

use crate::{
    ParseError, TranscriptStepResultV1, TranscriptV1,
    wire::{
        ActionRecordV1, EventRecordV1, FinalStateV1, HeaderV1, MatchCompletedV1, MatchCreatedV1,
        StepCompletedV1, StepRejectedV1,
    },
};

/// Selects the one optional value set that semantic comparison can include.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComparisonOptions {
    /// Compare the open header metadata object when this value is true.
    pub include_header_metadata: bool,
}

/// A stable path through the versioned transcript wire values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptPath(String);

impl TranscriptPath {
    /// Get the wire-oriented path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TranscriptPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The first semantic difference between two valid transcripts.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptDifference {
    pub sequence: u64,
    pub step: Option<u64>,
    pub event_index: Option<u64>,
    pub path: TranscriptPath,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
}

/// The result of semantic comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptComparison {
    Equal,
    Different(TranscriptDifference),
}

/// A boundary or representation error that prevented comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TranscriptComparisonError {
    ExpectedParse(ParseError),
    ActualParse(ParseError),
    ValueEncoding { message: String },
}

impl fmt::Display for TranscriptComparisonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExpectedParse(error) => {
                write!(formatter, "the expected transcript is invalid: {error}")
            }
            Self::ActualParse(error) => {
                write!(formatter, "the actual transcript is invalid: {error}")
            }
            Self::ValueEncoding { message } => {
                write!(
                    formatter,
                    "a typed transcript value could not be compared: {message}"
                )
            }
        }
    }
}

impl Error for TranscriptComparisonError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ExpectedParse(error) | Self::ActualParse(error) => Some(error),
            Self::ValueEncoding { .. } => None,
        }
    }
}

/// Parse and semantically compare two complete transcript inputs.
pub fn compare_transcripts(
    expected: impl BufRead,
    actual: impl BufRead,
    options: ComparisonOptions,
) -> Result<TranscriptComparison, TranscriptComparisonError> {
    let expected =
        TranscriptV1::parse(expected).map_err(TranscriptComparisonError::ExpectedParse)?;
    let actual = TranscriptV1::parse(actual).map_err(TranscriptComparisonError::ActualParse)?;
    compare_parsed_transcripts(&expected, &actual, options)
}

/// Compare two parsed transcripts without doing input or output work.
pub fn compare_parsed_transcripts(
    expected: &TranscriptV1,
    actual: &TranscriptV1,
    options: ComparisonOptions,
) -> Result<TranscriptComparison, TranscriptComparisonError> {
    let expected_records = transcript_records(expected);
    let actual_records = transcript_records(actual);

    for (expected_record, actual_record) in expected_records.iter().zip(&actual_records) {
        let expected_value = expected_record.value(options)?;
        let actual_value = actual_record.value(options)?;
        if let Some(value_difference) = first_value_difference(
            Some(&expected_value),
            Some(&actual_value),
            &expected_record.path,
        ) {
            return Ok(TranscriptComparison::Different(record_difference(
                expected_record,
                actual_record,
                value_difference,
            )));
        }
    }

    match (
        expected_records.get(actual_records.len()),
        actual_records.get(expected_records.len()),
    ) {
        (None, None) => Ok(TranscriptComparison::Equal),
        (expected_record, actual_record) => {
            let context = expected_record.or(actual_record).ok_or_else(|| {
                TranscriptComparisonError::ValueEncoding {
                    message: "a transcript length difference had no record context".to_string(),
                }
            })?;
            let expected_value = expected_record
                .map(|record| record.value(options))
                .transpose()?;
            let actual_value = actual_record
                .map(|record| record.value(options))
                .transpose()?;
            let value_difference = ValueDifference {
                path: context.path.clone(),
                expected: expected_value.map(OrderedValue::into_json),
                actual: actual_value.map(OrderedValue::into_json),
            };
            Ok(TranscriptComparison::Different(record_difference(
                expected_record.unwrap_or(context),
                actual_record.unwrap_or(context),
                value_difference,
            )))
        }
    }
}

fn record_difference(
    expected: &RecordView<'_>,
    actual: &RecordView<'_>,
    difference: ValueDifference,
) -> TranscriptDifference {
    TranscriptDifference {
        sequence: expected.sequence.min(actual.sequence),
        step: expected.step.or(actual.step),
        event_index: expected.event_index.or(actual.event_index),
        path: TranscriptPath(difference.path),
        expected: difference.expected,
        actual: difference.actual,
    }
}

#[derive(Clone, Copy)]
enum RecordValue<'a> {
    Header(&'a HeaderV1),
    MatchCreated(&'a MatchCreatedV1),
    Action(&'a ActionRecordV1),
    Event(&'a EventRecordV1),
    StepCompleted(&'a StepCompletedV1),
    StepRejected(&'a StepRejectedV1),
    FinalState(&'a FinalStateV1),
    MatchCompleted(&'a MatchCompletedV1),
}

struct RecordView<'a> {
    sequence: u64,
    step: Option<u64>,
    event_index: Option<u64>,
    path: String,
    value: RecordValue<'a>,
}

impl RecordView<'_> {
    fn value(&self, options: ComparisonOptions) -> Result<OrderedValue, TranscriptComparisonError> {
        let mut value = match self.value {
            RecordValue::Header(record) => ordered_value(record),
            RecordValue::MatchCreated(record) => ordered_value(record),
            RecordValue::Action(record) => ordered_value(record),
            RecordValue::Event(record) => ordered_value(record),
            RecordValue::StepCompleted(record) => ordered_value(record),
            RecordValue::StepRejected(record) => ordered_value(record),
            RecordValue::FinalState(record) => ordered_value(record),
            RecordValue::MatchCompleted(record) => ordered_value(record),
        }?;
        if !options.include_header_metadata && matches!(self.value, RecordValue::Header(_)) {
            value.remove_mapping_field("metadata");
        }
        Ok(value)
    }
}

fn transcript_records(transcript: &TranscriptV1) -> Vec<RecordView<'_>> {
    let mut records = Vec::new();
    records.push(RecordView {
        sequence: transcript.header.sequence,
        step: None,
        event_index: None,
        path: "header".to_string(),
        value: RecordValue::Header(&transcript.header),
    });
    records.push(RecordView {
        sequence: transcript.match_created.sequence,
        step: None,
        event_index: None,
        path: "match_created".to_string(),
        value: RecordValue::MatchCreated(&transcript.match_created),
    });

    for (step_index, step) in transcript.steps.iter().enumerate() {
        records.push(RecordView {
            sequence: step.action.sequence,
            step: Some(step.action.step),
            event_index: None,
            path: format!("steps[{step_index}].action"),
            value: RecordValue::Action(&step.action),
        });
        match &step.result {
            TranscriptStepResultV1::Accepted { events, completion } => {
                for (event_position, event) in events.iter().enumerate() {
                    records.push(RecordView {
                        sequence: event.sequence,
                        step: Some(event.step),
                        event_index: Some(event.index),
                        path: format!("steps[{step_index}].result.events[{event_position}]"),
                        value: RecordValue::Event(event),
                    });
                }
                records.push(RecordView {
                    sequence: completion.sequence,
                    step: Some(completion.step),
                    event_index: None,
                    path: format!("steps[{step_index}].result.completion"),
                    value: RecordValue::StepCompleted(completion),
                });
            }
            TranscriptStepResultV1::Rejected { rejection } => records.push(RecordView {
                sequence: rejection.sequence,
                step: Some(rejection.step),
                event_index: None,
                path: format!("steps[{step_index}].result.rejection"),
                value: RecordValue::StepRejected(rejection),
            }),
        }
    }

    records.push(RecordView {
        sequence: transcript.final_state.sequence,
        step: None,
        event_index: None,
        path: "final_state".to_string(),
        value: RecordValue::FinalState(&transcript.final_state),
    });
    records.push(RecordView {
        sequence: transcript.match_completed.sequence,
        step: None,
        event_index: None,
        path: "match_completed".to_string(),
        value: RecordValue::MatchCompleted(&transcript.match_completed),
    });
    records
}

#[derive(Debug, Clone, PartialEq)]
enum OrderedValue {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Sequence(Vec<Self>),
    Mapping(Vec<(String, Self)>),
}

impl OrderedValue {
    fn remove_mapping_field(&mut self, field: &str) {
        if let Self::Mapping(fields) = self {
            fields.retain(|(name, _)| name != field);
        }
    }

    fn into_json(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(value),
            Self::Number(value) => Value::Number(value),
            Self::String(value) => Value::String(value),
            Self::Sequence(values) => {
                Value::Array(values.into_iter().map(Self::into_json).collect())
            }
            Self::Mapping(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(name, value)| (name, value.into_json()))
                    .collect::<Map<_, _>>(),
            ),
        }
    }
}

impl<'de> Deserialize<'de> for OrderedValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(OrderedValueVisitor)
    }
}

struct OrderedValueVisitor;

impl<'de> Visitor<'de> for OrderedValueVisitor {
    type Value = OrderedValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON semantic value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(OrderedValue::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(OrderedValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(OrderedValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(OrderedValue::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(OrderedValue::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Number::from_f64(value)
            .map(OrderedValue::Number)
            .ok_or_else(|| E::custom("a non-finite number is not a JSON value"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(OrderedValue::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(OrderedValue::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(OrderedValue::Sequence(values))
    }

    fn visit_map<A>(self, mut mapping: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut fields = Vec::new();
        while let Some((name, value)) = mapping.next_entry()? {
            fields.push((name, value));
        }
        Ok(OrderedValue::Mapping(fields))
    }
}

fn ordered_value(value: &impl Serialize) -> Result<OrderedValue, TranscriptComparisonError> {
    let bytes = serde_json::to_vec(value).map_err(|error| value_encoding_error(&error))?;
    serde_json::from_slice(&bytes).map_err(|error| value_encoding_error(&error))
}

fn value_encoding_error(error: &serde_json::Error) -> TranscriptComparisonError {
    TranscriptComparisonError::ValueEncoding {
        message: error.to_string(),
    }
}

struct ValueDifference {
    path: String,
    expected: Option<Value>,
    actual: Option<Value>,
}

fn first_value_difference(
    expected: Option<&OrderedValue>,
    actual: Option<&OrderedValue>,
    path: &str,
) -> Option<ValueDifference> {
    match (expected, actual) {
        (Some(OrderedValue::Sequence(expected)), Some(OrderedValue::Sequence(actual))) => {
            for index in 0..expected.len().max(actual.len()) {
                let element_path = format!("{path}[{index}]");
                if let Some(difference) =
                    first_value_difference(expected.get(index), actual.get(index), &element_path)
                {
                    return Some(difference);
                }
            }
            None
        }
        (Some(OrderedValue::Mapping(expected)), Some(OrderedValue::Mapping(actual))) => {
            for (name, expected_value) in expected {
                let field_path = join_path(path, name);
                let actual_value = actual
                    .iter()
                    .find_map(|(actual_name, value)| (actual_name == name).then_some(value));
                if let Some(difference) =
                    first_value_difference(Some(expected_value), actual_value, &field_path)
                {
                    return Some(difference);
                }
            }
            actual.iter().find_map(|(name, actual_value)| {
                expected
                    .iter()
                    .all(|(expected_name, _)| expected_name != name)
                    .then(|| ValueDifference {
                        path: join_path(path, name),
                        expected: None,
                        actual: Some(actual_value.clone().into_json()),
                    })
            })
        }
        (Some(expected), Some(actual)) if expected == actual => None,
        (expected, actual) => Some(ValueDifference {
            path: path.to_string(),
            expected: expected.cloned().map(OrderedValue::into_json),
            actual: actual.cloned().map(OrderedValue::into_json),
        }),
    }
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}.{child}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn mapping(fields: &[(&str, OrderedValue)]) -> OrderedValue {
        OrderedValue::Mapping(
            fields
                .iter()
                .map(|(name, value)| ((*name).to_string(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn pure_traversal_uses_field_then_array_order() {
        let expected = mapping(&[
            ("first", OrderedValue::Bool(true)),
            (
                "second",
                OrderedValue::Sequence(vec![
                    OrderedValue::Number(1_u64.into()),
                    OrderedValue::Number(2_u64.into()),
                ]),
            ),
        ]);
        let actual = mapping(&[
            ("first", OrderedValue::Bool(false)),
            (
                "second",
                OrderedValue::Sequence(vec![
                    OrderedValue::Number(9_u64.into()),
                    OrderedValue::Number(2_u64.into()),
                ]),
            ),
        ]);

        let difference = first_value_difference(Some(&expected), Some(&actual), "record")
            .expect("values differ");
        assert_eq!(difference.path, "record.first");
        assert_eq!(difference.expected, Some(Value::Bool(true)));
        assert_eq!(difference.actual, Some(Value::Bool(false)));
    }

    #[test]
    fn pure_traversal_reports_missing_array_values() {
        let expected = OrderedValue::Sequence(vec![OrderedValue::String("one".to_string())]);
        let actual = OrderedValue::Sequence(Vec::new());

        let difference =
            first_value_difference(Some(&expected), Some(&actual), "items").expect("values differ");
        assert_eq!(difference.path, "items[0]");
        assert_eq!(difference.expected, Some(Value::String("one".to_string())));
        assert_eq!(difference.actual, None);
    }

    #[test]
    fn pure_traversal_matches_mapping_keys_not_insertion_order() {
        let expected = mapping(&[
            ("one", OrderedValue::Bool(true)),
            ("two", OrderedValue::Bool(false)),
        ]);
        let actual = mapping(&[
            ("two", OrderedValue::Bool(false)),
            ("one", OrderedValue::Bool(true)),
        ]);

        assert!(first_value_difference(Some(&expected), Some(&actual), "record").is_none());
    }
}
