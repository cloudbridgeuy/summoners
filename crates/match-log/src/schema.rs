//! Transcript schema boundary.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt, fs, io,
    path::{Path, PathBuf},
};

use schemars::{JsonSchema, SchemaGenerator};
use serde_json::{Map, Value, json};

use crate::wire::{
    ActionRecordV1, EventRecordV1, FinalStateV1, HeaderV1, MatchCompletedV1, MatchCreatedV1,
    StepCompletedV1, StepRejectedV1,
};

const ENTITY_ID_PATTERN: &str = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$";
const STATE_DIGEST_PATTERN: &str = "^sha256:[0-9a-f]{64}$";

const LIFECYCLE: &str = r#"# Summoners match log lifecycle, version 1

The JSON Schema files define each record. This document defines the rules that apply across records in one strict NDJSON transcript.

## Record order

The record order is `header`, `match_created`, then one or more action steps, `final_state`, and `match_completed`. A step contains `action`, zero or more `event` records for an accepted result, and exactly one `step_completed` or `step_rejected` result.

## Global sequence

`sequence` starts at 0 on `header` and increases by exactly 1 for every line. `match_created` has sequence 1. Missing, repeated, or reordered sequence values are invalid.

## Action steps

`step` starts at 1 on the first `action` and increases by exactly 1 after each step result. Each event and step result has the same step value as its action. A second action cannot start before the prior step result.

## Event indexes and counts

For each accepted step, event `index` starts at 0 and increases by exactly 1 in wire order. `step_completed.event_count` equals the number of event records in that step. An accepted step can have an event count of 0.

## Rejected steps

A rejected step has no event records. Its `state_digest` equals the digest before the rejected action, which proves that the authoritative state did not change.

## Terminal records

### Exactly one `game_ended` event

The transcript contains exactly one `game_ended` event. It is in the final accepted step. No action or other step result can follow that terminal step.

### Terminal outcome agreement

The winner and loss reason in the `game_ended` event, the ended status in `final_state`, and `match_completed` are equal. `final_state` must contain an ended state; a playing or broken state is invalid.

### Digest agreement

`match_created.state_digest` is the digest of `initial_state`. Each accepted step establishes its `step_completed.state_digest`. Each rejected step keeps the prior digest. The terminal step digest, the computed digest of `final_state`, `final_state.state_digest`, and `match_completed.state_digest` are equal.

### Total counts

`match_completed.step_count` equals the total number of accepted and rejected steps. `match_completed.event_count` equals the total number of event records in all accepted steps.

No record can follow `match_completed`. A transcript that stops before `match_completed`, has more than one terminal event, or omits any required terminal record is incomplete.
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContractArtifact {
    path: String,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct ContractGenerationError {
    path: String,
    source: serde_json::Error,
}

/// An error that prevents a V1 contract from being written.
#[derive(Debug)]
pub enum ContractWriteError {
    Generation {
        path: PathBuf,
        source: serde_json::Error,
    },
    CreateDirectory {
        path: PathBuf,
        source: io::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
}

impl ContractWriteError {
    /// Return the first path that could not be generated or written.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Generation { path, .. }
            | Self::CreateDirectory { path, .. }
            | Self::Write { path, .. } => path,
        }
    }
}

impl fmt::Display for ContractWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generation { path, source } => {
                write!(formatter, "cannot generate {}: {source}", path.display())
            }
            Self::CreateDirectory { path, source } => {
                write!(formatter, "cannot create {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
        }
    }
}

impl Error for ContractWriteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Generation { source, .. } => Some(source),
            Self::CreateDirectory { source, .. } | Self::Write { source, .. } => Some(source),
        }
    }
}

/// An error that identifies the first V1 contract artifact that differs.
#[derive(Debug)]
pub enum ContractVerifyError {
    Generation {
        path: PathBuf,
        source: serde_json::Error,
    },
    ReadDirectory {
        path: PathBuf,
        source: io::Error,
    },
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Missing {
        path: PathBuf,
    },
    Stale {
        path: PathBuf,
    },
    Unexpected {
        path: PathBuf,
    },
}

impl ContractVerifyError {
    /// Return the first path that could not be checked or did not match.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Generation { path, .. }
            | Self::ReadDirectory { path, .. }
            | Self::Read { path, .. }
            | Self::Missing { path }
            | Self::Stale { path }
            | Self::Unexpected { path } => path,
        }
    }
}

impl fmt::Display for ContractVerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generation { path, source } => {
                write!(formatter, "cannot generate {}: {source}", path.display())
            }
            Self::ReadDirectory { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Missing { path } => {
                write!(formatter, "missing contract artifact {}", path.display())
            }
            Self::Stale { path } => write!(formatter, "stale contract artifact {}", path.display()),
            Self::Unexpected { path } => {
                write!(formatter, "unexpected contract artifact {}", path.display())
            }
        }
    }
}

impl Error for ContractVerifyError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Generation { source, .. } => Some(source),
            Self::ReadDirectory { source, .. } | Self::Read { source, .. } => Some(source),
            Self::Missing { .. } | Self::Stale { .. } | Self::Unexpected { .. } => None,
        }
    }
}

/// Write the deterministic V1 wire contract to an explicit directory.
pub fn write_v1_contract(target: impl AsRef<Path>) -> Result<(), ContractWriteError> {
    let target = target.as_ref();
    let artifacts = expected_v1_contract().map_err(|error| ContractWriteError::Generation {
        path: target.join(error.path),
        source: error.source,
    })?;
    fs::create_dir_all(target).map_err(|source| ContractWriteError::CreateDirectory {
        path: target.to_path_buf(),
        source,
    })?;
    for artifact in artifacts {
        let path = target.join(artifact.path);
        fs::write(&path, artifact.bytes)
            .map_err(|source| ContractWriteError::Write { path, source })?;
    }
    Ok(())
}

/// Compare the deterministic V1 wire contract with an explicit directory.
///
/// This function does not write to the target directory.
pub fn verify_v1_contract(target: impl AsRef<Path>) -> Result<(), ContractVerifyError> {
    let target = target.as_ref();
    let artifacts = expected_v1_contract().map_err(|error| ContractVerifyError::Generation {
        path: target.join(error.path),
        source: error.source,
    })?;
    let expected = artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact.bytes.as_slice()))
        .collect::<BTreeMap<_, _>>();
    let actual = actual_paths(target, expected.keys().next().copied())?;
    let paths = expected
        .keys()
        .map(|path| (*path).to_string())
        .chain(actual.keys().cloned())
        .collect::<BTreeSet<_>>();

    for relative in paths {
        let path = target.join(&relative);
        match (expected.get(relative.as_str()), actual.get(&relative)) {
            (Some(_), None) => return Err(ContractVerifyError::Missing { path }),
            (None, Some(_)) => return Err(ContractVerifyError::Unexpected { path }),
            (Some(expected_bytes), Some(actual_path)) if actual_path.is_file() => {
                let actual_bytes =
                    fs::read(actual_path).map_err(|source| ContractVerifyError::Read {
                        path: actual_path.clone(),
                        source,
                    })?;
                if actual_bytes != *expected_bytes {
                    return Err(ContractVerifyError::Stale { path });
                }
            }
            (Some(_), Some(_)) => return Err(ContractVerifyError::Unexpected { path }),
            (None, None) => {}
        }
    }
    Ok(())
}

fn actual_paths(
    target: &Path,
    first_expected: Option<&str>,
) -> Result<BTreeMap<String, PathBuf>, ContractVerifyError> {
    let entries = match fs::read_dir(target) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            return first_expected.map_or_else(
                || Ok(BTreeMap::new()),
                |relative| {
                    Err(ContractVerifyError::Missing {
                        path: target.join(relative),
                    })
                },
            );
        }
        Err(source) => {
            return Err(ContractVerifyError::ReadDirectory {
                path: target.to_path_buf(),
                source,
            });
        }
    };

    let mut paths = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|source| ContractVerifyError::ReadDirectory {
            path: target.to_path_buf(),
            source,
        })?;
        let name =
            entry
                .file_name()
                .into_string()
                .map_err(|name| ContractVerifyError::Unexpected {
                    path: target.join(name),
                })?;
        paths.insert(name, entry.path());
    }
    Ok(paths)
}

fn expected_v1_contract() -> Result<Vec<ContractArtifact>, ContractGenerationError> {
    let mut artifacts = vec![
        schema_artifact::<ActionRecordV1>("action", "action record"),
        schema_artifact::<EventRecordV1>("event", "event record"),
        schema_artifact::<FinalStateV1>("final_state", "final state record"),
        schema_artifact::<HeaderV1>("header", "header record"),
        schema_artifact::<MatchCompletedV1>("match_completed", "match completion record"),
        schema_artifact::<MatchCreatedV1>("match_created", "match creation record"),
        schema_artifact::<StepCompletedV1>("step_completed", "accepted step result record"),
        schema_artifact::<StepRejectedV1>("step_rejected", "rejected step result record"),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    artifacts.push(ContractArtifact {
        path: "lifecycle.md".to_string(),
        bytes: LIFECYCLE.as_bytes().to_vec(),
    });
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(artifacts)
}

fn schema_artifact<T: JsonSchema>(
    record_name: &str,
    description: &str,
) -> Result<ContractArtifact, ContractGenerationError> {
    let schema = SchemaGenerator::default().into_root_schema_for::<T>();
    let mut value = Value::from(schema);
    normalize_schema(&mut value, record_name, description);
    let path = format!("{record_name}.json");
    let mut bytes =
        serde_json::to_vec_pretty(&value).map_err(|source| ContractGenerationError {
            path: path.clone(),
            source,
        })?;
    bytes.push(b'\n');
    Ok(ContractArtifact { path, bytes })
}

fn normalize_schema(schema: &mut Value, record_name: &str, description: &str) {
    strip_generated_prose(schema);
    rename_definitions(schema);
    replace_single_enums(schema);
    constrain_integer_bounds(schema);
    if let Some(root) = schema.as_object_mut() {
        root.insert(
            "$id".to_string(),
            Value::String(format!("urn:summoners:match-log:v1:{record_name}")),
        );
        root.insert(
            "title".to_string(),
            Value::String(format!("Summoners match log {description}, version 1")),
        );
        root.insert(
            "description".to_string(),
            Value::String(format!("Wire contract for one {description}.")),
        );
    }
    constrain_root(schema, record_name);
    constrain_named_properties(schema);
    constrain_definition(schema, "entity_id", ENTITY_ID_PATTERN);
    constrain_definition(schema, "state_digest", STATE_DIGEST_PATTERN);
}

fn strip_generated_prose(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(strip_generated_prose),
        Value::Object(fields) => {
            fields.remove("title");
            fields.remove("description");
            fields.values_mut().for_each(strip_generated_prose);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn rename_definitions(schema: &mut Value) {
    let Some(definitions) = schema.get_mut("$defs").and_then(Value::as_object_mut) else {
        return;
    };
    let renamed = std::mem::take(definitions)
        .into_iter()
        .map(|(name, value)| (wire_definition_name(&name), value))
        .collect::<Map<_, _>>();
    *definitions = renamed;
    rewrite_references(schema);
}

fn rewrite_references(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(rewrite_references),
        Value::Object(fields) => {
            let rewritten = fields
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| reference.strip_prefix("#/$defs/"))
                .map(wire_definition_name)
                .map(|name| Value::String(format!("#/$defs/{name}")));
            if let Some(reference) = rewritten {
                fields.insert("$ref".to_string(), reference);
            }
            fields.values_mut().for_each(rewrite_references);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn wire_definition_name(rust_name: &str) -> String {
    let name = rust_name.strip_suffix("V1").unwrap_or(rust_name);
    let characters = name.chars().collect::<Vec<_>>();
    let mut wire_name = String::new();
    for (index, character) in characters.iter().copied().enumerate() {
        let previous = index
            .checked_sub(1)
            .and_then(|prior| characters.get(prior))
            .copied();
        let next = characters.get(index + 1).copied();
        let word_boundary = character.is_ascii_uppercase()
            && index > 0
            && (previous.is_some_and(|value| value.is_ascii_lowercase() || value.is_ascii_digit())
                || next.is_some_and(|value| value.is_ascii_lowercase())
                    && previous.is_some_and(|value| value.is_ascii_uppercase()));
        if word_boundary {
            wire_name.push('_');
        }
        wire_name.push(character.to_ascii_lowercase());
    }
    wire_name
}

fn replace_single_enums(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(replace_single_enums),
        Value::Object(fields) => {
            if let Some(single) = fields
                .get("enum")
                .and_then(Value::as_array)
                .filter(|values| values.len() == 1)
                .and_then(|values| values.first())
                .cloned()
            {
                fields.remove("enum");
                fields.insert("const".to_string(), single);
            }
            fields.values_mut().for_each(replace_single_enums);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn constrain_integer_bounds(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(constrain_integer_bounds),
        Value::Object(fields) => {
            let maximum = fields
                .get("format")
                .and_then(Value::as_str)
                .and_then(|format| match format {
                    "uint8" => Some(Value::from(u8::MAX)),
                    "uint16" => Some(Value::from(u16::MAX)),
                    "uint32" => Some(Value::from(u32::MAX)),
                    "uint64" => Some(Value::from(u64::MAX)),
                    _ => None,
                });
            if let Some(maximum) = maximum {
                fields.insert("maximum".to_string(), maximum);
            }
            fields.values_mut().for_each(constrain_integer_bounds);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn constrain_root(schema: &mut Value, record_name: &str) {
    let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) else {
        return;
    };
    if record_name == "header" {
        properties.insert("format".to_string(), json!({ "const": "summoners_match" }));
        properties.insert("format_version".to_string(), json!({ "const": 1 }));
        properties.insert(
            "metadata".to_string(),
            json!({ "type": "object", "additionalProperties": true }),
        );
    }
}

fn constrain_named_properties(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(constrain_named_properties),
        Value::Object(fields) => {
            if let Some(properties) = fields.get_mut("properties").and_then(Value::as_object_mut) {
                for (name, property_schema) in properties {
                    let pattern = match name.as_str() {
                        "ability" | "definition" | "entity" => Some(ENTITY_ID_PATTERN),
                        "state_digest" => Some(STATE_DIGEST_PATTERN),
                        _ => None,
                    };
                    if let (Some(pattern), Some(property_schema)) =
                        (pattern, property_schema.as_object_mut())
                    {
                        property_schema
                            .insert("pattern".to_string(), Value::String(pattern.to_string()));
                    }
                }
            }
            fields.values_mut().for_each(constrain_named_properties);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn constrain_definition(schema: &mut Value, name: &str, pattern: &str) {
    if let Some(definition) = schema
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .and_then(|definitions| definitions.get_mut(name))
    {
        *definition = json!({ "type": "string", "pattern": pattern });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use schemars::SchemaGenerator;
    use serde_json::Value;

    use super::*;
    use crate::wire::RecordV1;

    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let number = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "summoners-match-log-schema-{}-{number}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("the test directory is created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn pure_contract_generation_is_stable_and_complete() {
        let first = expected_v1_contract().expect("the contract is generated");
        let second = expected_v1_contract().expect("the contract is generated again");

        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|artifact| artifact.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "action.json",
                "event.json",
                "final_state.json",
                "header.json",
                "lifecycle.md",
                "match_completed.json",
                "match_created.json",
                "step_completed.json",
                "step_rejected.json",
            ]
        );
        assert_eq!(
            first
                .iter()
                .filter(|artifact| artifact.path.ends_with(".json"))
                .count(),
            8
        );
    }

    #[test]
    fn every_record_variant_has_one_contract_artifact() {
        let artifacts = expected_v1_contract().expect("the contract is generated");
        let artifact_names = artifacts
            .iter()
            .filter_map(|artifact| artifact.path.strip_suffix(".json"))
            .collect::<BTreeSet<_>>();

        let schema = SchemaGenerator::default().into_root_schema_for::<RecordV1>();
        let mut value = Value::from(schema);
        rename_definitions(&mut value);
        replace_single_enums(&mut value);
        let record_names = value["$defs"]
            .as_object()
            .expect("the record union has definitions")
            .iter()
            .filter(|(name, _)| name.ends_with("_record_kind"))
            .filter_map(|(_, schema)| schema.get("const").and_then(Value::as_str))
            .collect::<BTreeSet<_>>();

        assert_eq!(artifact_names, record_names);
    }

    #[test]
    fn generated_schemas_use_wire_names_and_parser_constraints() {
        let artifacts = expected_v1_contract().expect("the contract is generated");
        for artifact in artifacts
            .iter()
            .filter(|artifact| artifact.path.ends_with(".json"))
        {
            let text = std::str::from_utf8(&artifact.bytes).expect("schema text is UTF-8");
            assert!(
                !text.contains("V1"),
                "{} exposes a Rust version suffix",
                artifact.path
            );
            assert!(
                !text.contains("::"),
                "{} exposes a Rust module path",
                artifact.path
            );

            let schema: Value = serde_json::from_slice(&artifact.bytes).expect("valid JSON Schema");
            assert_eq!(
                schema["additionalProperties"], false,
                "{} has a closed root",
                artifact.path
            );
            assert_eq!(schema["properties"]["sequence"]["type"], "integer");
            assert_eq!(schema["properties"]["sequence"]["minimum"], 0);
            assert_eq!(
                schema["properties"]["sequence"]["maximum"],
                u64::MAX,
                "{} limits sequence to the parser's integer type",
                artifact.path
            );
            assert_integer_formats_have_bounds(&schema);
        }

        let header = schema_artifact(&artifacts, "header.json");
        assert_eq!(header["properties"]["format"]["const"], "summoners_match");
        assert_eq!(header["properties"]["format_version"]["const"], 1);
        assert_ne!(
            header["properties"]["metadata"]["additionalProperties"], false,
            "header metadata stays open"
        );

        let created = schema_artifact(&artifacts, "match_created.json");
        assert!(contains_string(
            &created,
            "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
        ));
        assert!(contains_string(&created, "^sha256:[0-9a-f]{64}$"));

        let action = schema_artifact(&artifacts, "action.json");
        assert!(contains_key_value(&action, "const", "action"));
    }

    #[test]
    fn lifecycle_text_names_every_cross_record_rule() {
        let artifacts = expected_v1_contract().expect("the contract is generated");
        let lifecycle = artifacts
            .iter()
            .find(|artifact| artifact.path == "lifecycle.md")
            .expect("the lifecycle artifact exists");
        let text = std::str::from_utf8(&lifecycle.bytes).expect("lifecycle text is UTF-8");

        for required in [
            "Global sequence",
            "Action steps",
            "Event indexes and counts",
            "Rejected steps",
            "Exactly one `game_ended` event",
            "Terminal outcome agreement",
            "Digest agreement",
            "Total counts",
            "No record can follow `match_completed`",
        ] {
            assert!(
                text.contains(required),
                "missing lifecycle rule: {required}"
            );
        }
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn write_is_stable_and_verify_reports_the_first_stale_path() {
        let target = TestDirectory::new();
        write_v1_contract(&target.0).expect("the first contract write succeeds");
        let first = read_artifact_bytes(&target.0);
        write_v1_contract(&target.0).expect("the second contract write succeeds");
        assert_eq!(first, read_artifact_bytes(&target.0));
        verify_v1_contract(&target.0).expect("the generated contract verifies");

        let stale_path = target.0.join("action.json");
        fs::write(&stale_path, b"stale\n").expect("the test makes one artifact stale");
        let error =
            verify_v1_contract(&target.0).expect_err("the stale artifact fails verification");

        assert_eq!(error.path(), stale_path.as_path());
        assert!(matches!(error, ContractVerifyError::Stale { .. }));
    }

    #[test]
    fn verification_orders_missing_stale_and_unexpected_paths() {
        let target = TestDirectory::new();
        write_v1_contract(&target.0).expect("the contract write succeeds");
        let unexpected_path = target.0.join("000-unexpected");
        fs::write(&unexpected_path, b"extra\n").expect("the unexpected file is written");
        fs::write(target.0.join("action.json"), b"stale\n").expect("one schema is stale");

        let error = verify_v1_contract(&target.0).expect_err("the first path fails verification");
        assert_eq!(error.path(), unexpected_path.as_path());
        assert!(matches!(error, ContractVerifyError::Unexpected { .. }));
    }

    #[test]
    fn verification_reports_the_first_missing_path_without_writing() {
        let target = TestDirectory::new();
        write_v1_contract(&target.0).expect("the contract write succeeds");
        let missing_path = target.0.join("action.json");
        fs::remove_file(&missing_path).expect("the first schema is removed");

        let error =
            verify_v1_contract(&target.0).expect_err("the missing schema fails verification");

        assert_eq!(error.path(), missing_path.as_path());
        assert!(matches!(error, ContractVerifyError::Missing { .. }));
        assert!(
            !missing_path.exists(),
            "verification does not restore the file"
        );
    }

    #[test]
    fn checked_in_contract_is_current() {
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/v1");
        verify_v1_contract(target).expect("the checked-in contract is current");
    }

    fn schema_artifact(artifacts: &[ContractArtifact], path: &str) -> Value {
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact.path == path)
            .expect("the schema artifact exists");
        serde_json::from_slice(&artifact.bytes).expect("valid JSON Schema")
    }

    fn contains_string(value: &Value, expected: &str) -> bool {
        match value {
            Value::String(text) => text == expected,
            Value::Array(values) => values.iter().any(|value| contains_string(value, expected)),
            Value::Object(fields) => fields
                .values()
                .any(|value| contains_string(value, expected)),
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }

    fn contains_key_value(value: &Value, key: &str, expected: &str) -> bool {
        match value {
            Value::Array(values) => values
                .iter()
                .any(|value| contains_key_value(value, key, expected)),
            Value::Object(fields) => {
                fields.get(key).and_then(Value::as_str) == Some(expected)
                    || fields
                        .values()
                        .any(|value| contains_key_value(value, key, expected))
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
        }
    }

    fn assert_integer_formats_have_bounds(value: &Value) {
        match value {
            Value::Array(values) => values.iter().for_each(assert_integer_formats_have_bounds),
            Value::Object(fields) => {
                if fields
                    .get("format")
                    .and_then(Value::as_str)
                    .is_some_and(|format| format.starts_with("uint"))
                {
                    assert!(fields.contains_key("minimum"));
                    assert!(fields.contains_key("maximum"));
                    assert_eq!(fields.get("type").and_then(Value::as_str), Some("integer"));
                }
                fields.values().for_each(assert_integer_formats_have_bounds);
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    fn read_artifact_bytes(target: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut paths = fs::read_dir(target)
            .expect("the contract directory is readable")
            .map(|entry| entry.expect("the contract entry is readable").path())
            .collect::<Vec<_>>();
        paths.sort();
        paths
            .into_iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .expect("the artifact has a name")
                    .to_string_lossy()
                    .into_owned();
                let bytes = fs::read(path).expect("the artifact is readable");
                (name, bytes)
            })
            .collect()
    }
}
