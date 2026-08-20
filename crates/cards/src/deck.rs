use std::{collections::BTreeMap, error::Error, fmt};

use serde::Deserialize;
use summoners_core::domain::cards::{EntityId, Form};

use crate::{CardLibrary, DocumentKind, LoadPhase};

const SCHEMA_VERSION: i64 = 1;
const BODY_SIZE: u32 = 20;
const COPY_LIMIT: u32 = 2;

/// Stable authoring-key families used by Deck documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeckStableKeyKind {
    Deck,
    Set,
    Card,
    QualifiedCard,
}

/// The typed reason a Deck load failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckLoadCause {
    InvalidUtf8 {
        valid_up_to: usize,
        error_len: Option<usize>,
    },
    TomlSyntax {
        message: String,
    },
    MissingSchemaVersion,
    InvalidSchemaVersion {
        found: String,
    },
    UnsupportedSchemaVersion {
        found: i64,
    },
    SchemaDecode {
        message: String,
    },
    InvalidStableKey {
        kind: DeckStableKeyKind,
        value: String,
    },
    NameMustNotBeEmpty,
    DuplicateSetRequirement {
        set: String,
        first_path: String,
    },
    RequiredSetUnavailable {
        set: String,
    },
    SetRevisionMismatch {
        set: String,
        required: u32,
        available: u32,
    },
    MissingSetRequirement {
        set: String,
    },
    UnresolvedCard {
        key: String,
    },
    StarterMustBeBase {
        key: String,
    },
    StarterInBody {
        key: String,
    },
    QuantityMustBePositive,
    CopyLimitExceeded {
        key: String,
        copies: u32,
        limit: u32,
    },
    BodyCardCount {
        expected: u32,
        found: u32,
    },
}

/// A stable, typed Deck load failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckLoadError {
    pub document: DocumentKind,
    pub phase: LoadPhase,
    pub schema_version: Option<i64>,
    pub path: String,
    pub cause: DeckLoadCause,
}

impl DeckLoadError {
    fn new(
        phase: LoadPhase,
        schema_version: Option<i64>,
        path: impl Into<String>,
        cause: DeckLoadCause,
    ) -> Self {
        Self {
            document: DocumentKind::Deck,
            phase,
            schema_version,
            path: path.into(),
            cause,
        }
    }
}

impl fmt::Display for DeckLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Deck load failed during {:?} at {}: {:?}",
            self.phase, self.path, self.cause
        )
    }
}

impl Error for DeckLoadError {}

/// One resolved Deck recipe with its Starter kept outside the expanded body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deck {
    id: String,
    name: String,
    starter: EntityId,
    body: Vec<EntityId>,
}

impl Deck {
    /// The Deck's stable authored code.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The Deck's display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The separate Base Starter definition.
    #[must_use]
    pub const fn starter(&self) -> EntityId {
        self.starter
    }

    /// The expanded 20-card body in authored recipe order.
    #[must_use]
    pub fn body(&self) -> &[EntityId] {
        &self.body
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckDto {
    schema_version: u32,
    id: String,
    name: String,
    starter: String,
    requires: Vec<SetRequirementDto>,
    cards: Vec<CardQuantityDto>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRequirementDto {
    set: String,
    revision: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CardQuantityDto {
    card: String,
    quantity: u32,
}

struct QualifiedCard {
    key: String,
    set: String,
}

/// Parse and resolve one complete Deck document from caller-held bytes.
pub fn parse_deck(bytes: &[u8], library: &CardLibrary) -> Result<Deck, DeckLoadError> {
    let source = decode_utf8(bytes)?;
    let version = read_version(source)?;
    if version != SCHEMA_VERSION {
        return Err(DeckLoadError::new(
            LoadPhase::Version,
            Some(version),
            "schema_version",
            DeckLoadCause::UnsupportedSchemaVersion { found: version },
        ));
    }
    let decoded = decode_v1(source)?;
    resolve(decoded, library)
}

fn decode_utf8(bytes: &[u8]) -> Result<&str, DeckLoadError> {
    std::str::from_utf8(bytes).map_err(|error| {
        DeckLoadError::new(
            LoadPhase::Utf8,
            None,
            "$",
            DeckLoadCause::InvalidUtf8 {
                valid_up_to: error.valid_up_to(),
                error_len: error.error_len(),
            },
        )
    })
}

fn read_version(source: &str) -> Result<i64, DeckLoadError> {
    let table: toml::Table = toml::from_str(source).map_err(|error| {
        DeckLoadError::new(
            LoadPhase::Version,
            None,
            "$",
            DeckLoadCause::TomlSyntax {
                message: error.message().to_string(),
            },
        )
    })?;
    let Some(value) = table.get("schema_version") else {
        return Err(DeckLoadError::new(
            LoadPhase::Version,
            None,
            "schema_version",
            DeckLoadCause::MissingSchemaVersion,
        ));
    };
    value.as_integer().ok_or_else(|| {
        DeckLoadError::new(
            LoadPhase::Version,
            None,
            "schema_version",
            DeckLoadCause::InvalidSchemaVersion {
                found: value.type_str().to_string(),
            },
        )
    })
}

fn decode_v1(source: &str) -> Result<DeckDto, DeckLoadError> {
    let deserializer = toml::de::Deserializer::parse(source).map_err(|error| {
        DeckLoadError::new(
            LoadPhase::Decode,
            Some(SCHEMA_VERSION),
            "$",
            DeckLoadCause::SchemaDecode {
                message: error.message().to_string(),
            },
        )
    })?;
    serde_path_to_error::deserialize(deserializer).map_err(|error| {
        let path = error.path().to_string();
        DeckLoadError::new(
            LoadPhase::Decode,
            Some(SCHEMA_VERSION),
            if path.is_empty() { "$" } else { &path },
            DeckLoadCause::SchemaDecode {
                message: error.inner().message().to_string(),
            },
        )
    })
}

fn resolve(raw: DeckDto, library: &CardLibrary) -> Result<Deck, DeckLoadError> {
    let DeckDto {
        schema_version,
        id,
        name,
        starter,
        requires,
        cards,
    } = raw;
    if i64::from(schema_version) != SCHEMA_VERSION {
        return Err(DeckLoadError::new(
            LoadPhase::Version,
            Some(i64::from(schema_version)),
            "schema_version",
            DeckLoadCause::UnsupportedSchemaVersion {
                found: i64::from(schema_version),
            },
        ));
    }
    require_stable_key(&id, DeckStableKeyKind::Deck, "id")?;
    if name.trim().is_empty() {
        return Err(semantic_error("name", DeckLoadCause::NameMustNotBeEmpty));
    }
    let requirements = resolve_requirements(requires, library)?;
    let starter_ref = parse_qualified(&starter, "starter")?;
    require_bound(&starter_ref, &requirements, "starter")?;
    let starter_id = resolve_card(library, &starter_ref, "starter")?;
    require_base_starter(library, starter_id, &starter_ref.key)?;
    let body = resolve_body(cards, library, &requirements, &starter_ref.key)?;
    Ok(Deck {
        id,
        name,
        starter: starter_id,
        body,
    })
}

fn resolve_requirements(
    raw: Vec<SetRequirementDto>,
    library: &CardLibrary,
) -> Result<BTreeMap<String, u32>, DeckLoadError> {
    let mut requirements = BTreeMap::new();
    let mut paths = BTreeMap::new();
    for (index, requirement) in raw.into_iter().enumerate() {
        let path = format!("requires[{index}]");
        require_stable_key(
            &requirement.set,
            DeckStableKeyKind::Set,
            &format!("{path}.set"),
        )?;
        if let Some(first_path) = paths.insert(requirement.set.clone(), path.clone()) {
            return Err(semantic_error(
                &format!("{path}.set"),
                DeckLoadCause::DuplicateSetRequirement {
                    set: requirement.set,
                    first_path,
                },
            ));
        }
        let Some(available) = library.set_revision(&requirement.set) else {
            return Err(semantic_error(
                &format!("{path}.set"),
                DeckLoadCause::RequiredSetUnavailable {
                    set: requirement.set,
                },
            ));
        };
        if available != requirement.revision {
            return Err(semantic_error(
                &format!("{path}.revision"),
                DeckLoadCause::SetRevisionMismatch {
                    set: requirement.set,
                    required: requirement.revision,
                    available,
                },
            ));
        }
        requirements.insert(requirement.set, requirement.revision);
    }
    Ok(requirements)
}

fn resolve_body(
    raw: Vec<CardQuantityDto>,
    library: &CardLibrary,
    requirements: &BTreeMap<String, u32>,
    starter: &str,
) -> Result<Vec<EntityId>, DeckLoadError> {
    let mut body = Vec::new();
    let mut copies = BTreeMap::<String, u32>::new();
    for (index, item) in raw.into_iter().enumerate() {
        let path = format!("cards[{index}]");
        let card = parse_qualified(&item.card, &format!("{path}.card"))?;
        require_bound(&card, requirements, &format!("{path}.card"))?;
        if item.quantity == 0 {
            return Err(semantic_error(
                &format!("{path}.quantity"),
                DeckLoadCause::QuantityMustBePositive,
            ));
        }
        if card.key == starter {
            return Err(semantic_error(
                &format!("{path}.card"),
                DeckLoadCause::StarterInBody {
                    key: card.key.clone(),
                },
            ));
        }
        let id = resolve_card(library, &card, &format!("{path}.card"))?;
        let count = copies.entry(card.key.clone()).or_default();
        *count = count.saturating_add(item.quantity);
        if *count > COPY_LIMIT {
            return Err(semantic_error(
                &format!("{path}.quantity"),
                DeckLoadCause::CopyLimitExceeded {
                    key: card.key,
                    copies: *count,
                    limit: COPY_LIMIT,
                },
            ));
        }
        body.extend(std::iter::repeat_n(id, item.quantity as usize));
    }
    if body.len() != BODY_SIZE as usize {
        return Err(semantic_error(
            "cards",
            DeckLoadCause::BodyCardCount {
                expected: BODY_SIZE,
                found: u32::try_from(body.len()).unwrap_or(u32::MAX),
            },
        ));
    }
    Ok(body)
}

fn parse_qualified(value: &str, path: &str) -> Result<QualifiedCard, DeckLoadError> {
    let mut parts = value.split('/');
    let Some(set) = parts.next() else {
        return Err(invalid_qualified(value, path));
    };
    let Some(card) = parts.next() else {
        return Err(invalid_qualified(value, path));
    };
    if parts.next().is_some() || !stable_key_syntax(set) || !stable_key_syntax(card) {
        return Err(invalid_qualified(value, path));
    }
    Ok(QualifiedCard {
        key: value.to_string(),
        set: set.to_string(),
    })
}

fn invalid_qualified(value: &str, path: &str) -> DeckLoadError {
    semantic_error(
        path,
        DeckLoadCause::InvalidStableKey {
            kind: DeckStableKeyKind::QualifiedCard,
            value: value.to_string(),
        },
    )
}

fn require_bound(
    card: &QualifiedCard,
    requirements: &BTreeMap<String, u32>,
    path: &str,
) -> Result<(), DeckLoadError> {
    if requirements.contains_key(&card.set) {
        Ok(())
    } else {
        Err(semantic_error(
            path,
            DeckLoadCause::MissingSetRequirement {
                set: card.set.clone(),
            },
        ))
    }
}

fn resolve_card(
    library: &CardLibrary,
    card: &QualifiedCard,
    path: &str,
) -> Result<EntityId, DeckLoadError> {
    library.card_id(&card.key).ok_or_else(|| {
        semantic_error(
            path,
            DeckLoadCause::UnresolvedCard {
                key: card.key.clone(),
            },
        )
    })
}

fn require_base_starter(
    library: &CardLibrary,
    id: EntityId,
    key: &str,
) -> Result<(), DeckLoadError> {
    let is_base = library
        .core_cards()
        .get(id)
        .and_then(|entity| entity.get::<Form>())
        == Some(&Form::Base);
    if is_base {
        Ok(())
    } else {
        Err(semantic_error(
            "starter",
            DeckLoadCause::StarterMustBeBase {
                key: key.to_string(),
            },
        ))
    }
}

fn require_stable_key(
    value: &str,
    kind: DeckStableKeyKind,
    path: &str,
) -> Result<(), DeckLoadError> {
    if stable_key_syntax(value) {
        Ok(())
    } else {
        Err(semantic_error(
            path,
            DeckLoadCause::InvalidStableKey {
                kind,
                value: value.to_string(),
            },
        ))
    }
}

fn stable_key_syntax(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
        && bytes.last() != Some(&b'-')
        && !bytes.windows(2).any(|pair| pair == b"--")
}

fn semantic_error(path: &str, cause: DeckLoadCause) -> DeckLoadError {
    DeckLoadError::new(LoadPhase::Semantics, Some(SCHEMA_VERSION), path, cause)
}

#[cfg(test)]
mod coverage_tests;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn stable_key_syntax_accepts_and_rejects_boundaries() {
        assert!(stable_key_syntax("a"));
        assert!(stable_key_syntax("set-paths-1"));
        for invalid in ["", "A", "-a", "a-", "a--b", "a_b"] {
            assert!(!stable_key_syntax(invalid), "{invalid}");
        }
    }

    #[test]
    fn qualified_reference_requires_exactly_two_stable_parts() {
        assert_eq!(
            parse_qualified("foundations/ember-lance", "card")
                .expect("valid reference")
                .set,
            "foundations"
        );
        for invalid in ["foundations", "/card", "set/", "a/b/c", "Set/card"] {
            assert!(parse_qualified(invalid, "card").is_err(), "{invalid}");
        }
    }

    #[test]
    fn utf8_decoder_reports_the_byte_location() {
        let error = decode_utf8(&[b'a', 0xff]).unwrap_err();
        assert_eq!(error.document, DocumentKind::Deck);
        assert_eq!(error.phase, LoadPhase::Utf8);
        assert_eq!(
            error.cause,
            DeckLoadCause::InvalidUtf8 {
                valid_up_to: 1,
                error_len: Some(1),
            }
        );
    }

    #[test]
    fn version_reader_covers_missing_non_integer_and_syntax_cases() {
        assert_eq!(
            read_version("id = \"x\"").unwrap_err().cause,
            DeckLoadCause::MissingSchemaVersion
        );
        assert_eq!(
            read_version("schema_version = \"1\"").unwrap_err().cause,
            DeckLoadCause::InvalidSchemaVersion {
                found: "string".to_string(),
            }
        );
        assert!(matches!(
            read_version("schema_version = [").unwrap_err().cause,
            DeckLoadCause::TomlSyntax { .. }
        ));
    }

    #[test]
    fn v1_decoder_rejects_unknown_nested_fields() {
        let source = r#"
schema_version = 1
id = "test"
name = "Test"
starter = "foundations/warden-initiate"
requires = [{ set = "foundations", revision = 1, extra = true }]
cards = []
"#;
        let error = decode_v1(source).unwrap_err();
        assert_eq!(error.phase, LoadPhase::Decode);
        assert!(matches!(error.cause, DeckLoadCause::SchemaDecode { .. }));
    }
}
