use std::{error::Error, fmt, io};

/// Why a state projection could not be rebuilt as a core state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateRebuildError {
    InvalidEntityId { value: String },
    EmptyUpgradeChain,
    StackSegmentBaseOutOfRange { value: u64 },
    UnsupportedBreakageRule { rule: String },
}

impl fmt::Display for StateRebuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntityId { value } => {
                write!(formatter, "the entity ID is not valid: {value}")
            }
            Self::EmptyUpgradeChain => write!(formatter, "the upgrade chain has no layers"),
            Self::StackSegmentBaseOutOfRange { value } => {
                write!(formatter, "the Stack segment base is too large: {value}")
            }
            Self::UnsupportedBreakageRule { rule } => {
                write!(formatter, "the breakage rule is not supported: {rule}")
            }
        }
    }
}

impl Error for StateRebuildError {}

/// Why canonical state JSON could not be encoded.
#[derive(Debug)]
pub struct CanonicalStateError {
    source: serde_json::Error,
}

impl CanonicalStateError {
    pub(crate) fn new(source: serde_json::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for CanonicalStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "canonical state encoding failed: {}",
            self.source
        )
    }
}

impl Error for CanonicalStateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Why one wire record could not be encoded.
#[derive(Debug)]
pub struct EncodeError {
    source: serde_json::Error,
}

impl EncodeError {
    pub(crate) fn new(source: serde_json::Error) -> Self {
        Self { source }
    }
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "record encoding failed: {}", self.source)
    }
}

impl Error for EncodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Why a recording could not reach its first durable checkpoint.
#[derive(Debug)]
pub enum RecordingError {
    CanonicalState(CanonicalStateError),
    Encode(EncodeError),
    Write(io::Error),
    Flush(io::Error),
}

impl fmt::Display for RecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CanonicalState(error) => error.fmt(formatter),
            Self::Encode(error) => error.fmt(formatter),
            Self::Write(error) => write!(formatter, "record write failed: {error}"),
            Self::Flush(error) => write!(formatter, "record flush failed: {error}"),
        }
    }
}

impl Error for RecordingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CanonicalState(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Write(error) | Self::Flush(error) => Some(error),
        }
    }
}

impl From<CanonicalStateError> for RecordingError {
    fn from(error: CanonicalStateError) -> Self {
        Self::CanonicalState(error)
    }
}

impl From<EncodeError> for RecordingError {
    fn from(error: EncodeError) -> Self {
        Self::Encode(error)
    }
}
