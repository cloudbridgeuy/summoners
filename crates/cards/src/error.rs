use std::{error::Error, fmt};

/// The authored document family that failed to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Set,
}

/// The deterministic parser phase that rejected the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadPhase {
    Utf8,
    Version,
    Decode,
    Semantics,
    Identity,
    Conversion,
}

/// The stable authoring-key family involved in an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StableKeyKind {
    Set,
    Card,
    Ability,
}

/// A closed semantic-policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticRule {
    SetMustContainCards,
    RevisionMustBePositive,
    NameMustNotBeEmpty,
    SummonRequiresStatistics,
    NonSummonForbidsStatistics,
    SummonRequiresOneAttack,
    ManaTypesMustBeUnique,
    AbilityShape,
    SpellShape,
    EnchantmentShape,
    EffectTarget,
    EffectCombination,
    AmountMustBePositive,
}

/// The typed reason a Set load failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetLoadCause {
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
        kind: StableKeyKind,
        value: String,
    },
    DuplicateStableKey {
        kind: StableKeyKind,
        value: String,
        first_path: String,
    },
    DuplicateGeneratedId {
        id: String,
        first_path: String,
    },
    MissingRequiredField {
        field: &'static str,
    },
    ForbiddenField {
        field: &'static str,
    },
    DuplicateValue {
        value: String,
        first_path: String,
    },
    InvalidSemantics {
        rule: SemanticRule,
    },
}

/// A stable, typed Set load failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetLoadError {
    pub document: DocumentKind,
    pub phase: LoadPhase,
    pub schema_version: Option<i64>,
    pub path: String,
    pub cause: SetLoadCause,
}

impl SetLoadError {
    pub(crate) fn new(
        phase: LoadPhase,
        schema_version: Option<i64>,
        path: impl Into<String>,
        cause: SetLoadCause,
    ) -> Self {
        Self {
            document: DocumentKind::Set,
            phase,
            schema_version,
            path: path.into(),
            cause,
        }
    }
}

impl fmt::Display for SetLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Set load failed during {:?} at {}: {:?}",
            self.phase, self.path, self.cause
        )
    }
}

impl Error for SetLoadError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_records_the_set_context() {
        let error = SetLoadError::new(
            LoadPhase::Version,
            Some(2),
            "schema_version",
            SetLoadCause::UnsupportedSchemaVersion { found: 2 },
        );
        assert_eq!(error.document, DocumentKind::Set);
        assert_eq!(error.schema_version, Some(2));
        assert_eq!(error.path, "schema_version");
    }

    #[test]
    fn display_names_the_phase_and_path() {
        let error = SetLoadError::new(
            LoadPhase::Version,
            None,
            "schema_version",
            SetLoadCause::MissingSchemaVersion,
        );
        let message = error.to_string();
        assert!(message.contains("Version"));
        assert!(message.contains("schema_version"));
    }
}
