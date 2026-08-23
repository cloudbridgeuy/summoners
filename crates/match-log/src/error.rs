use std::{error::Error, fmt, io};

/// Why an accepted event batch cannot be a complete terminal step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalEventError {
    MissingGameEnded,
    MultipleGameEnded,
    UnexpectedGameEnded,
    OutcomeMismatch,
}

impl fmt::Display for TerminalEventError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingGameEnded => write!(formatter, "the terminal step has no GameEnded event"),
            Self::MultipleGameEnded => {
                write!(formatter, "the terminal step has multiple GameEnded events")
            }
            Self::UnexpectedGameEnded => {
                write!(formatter, "a playing step has a GameEnded event")
            }
            Self::OutcomeMismatch => {
                write!(
                    formatter,
                    "the GameEnded event does not match the final state"
                )
            }
        }
    }
}

impl Error for TerminalEventError {}

/// Why a typed wire value could not be converted to a core domain value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireConversionError {
    InvalidEntityId { value: String },
    IntegerOutOfRange { field: &'static str, value: u64 },
    StateRebuild(StateRebuildError),
}

impl fmt::Display for WireConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntityId { value } => {
                write!(formatter, "the entity ID is not valid: {value}")
            }
            Self::IntegerOutOfRange { field, value } => {
                write!(formatter, "{field} is too large: {value}")
            }
            Self::StateRebuild(error) => error.fmt(formatter),
        }
    }
}

impl Error for WireConversionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StateRebuild(error) => Some(error),
            Self::InvalidEntityId { .. } | Self::IntegerOutOfRange { .. } => None,
        }
    }
}

impl From<StateRebuildError> for WireConversionError {
    fn from(error: StateRebuildError) -> Self {
        Self::StateRebuild(error)
    }
}

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

/// Why an active recording stopped accepting actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingStopped {
    MatchCompleted,
    RecordingFailed,
    GameAlreadyEnded,
    GameBroken,
    InvalidTerminalEvents(TerminalEventError),
}

impl fmt::Display for RecordingStopped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MatchCompleted => write!(formatter, "the match recording is complete"),
            Self::RecordingFailed => write!(formatter, "the match recording previously failed"),
            Self::GameAlreadyEnded => write!(formatter, "the game is already ended"),
            Self::GameBroken => write!(formatter, "the game entered a broken state"),
            Self::InvalidTerminalEvents(error) => error.fmt(formatter),
        }
    }
}

impl Error for RecordingStopped {}

/// Why a recording operation could not reach its durable checkpoint.
#[derive(Debug)]
pub enum RecordingError {
    CanonicalState(CanonicalStateError),
    Encode(EncodeError),
    Write(io::Error),
    Flush(io::Error),
    Stopped(RecordingStopped),
}

impl fmt::Display for RecordingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CanonicalState(error) => error.fmt(formatter),
            Self::Encode(error) => error.fmt(formatter),
            Self::Write(error) => write!(formatter, "record write failed: {error}"),
            Self::Flush(error) => write!(formatter, "record flush failed: {error}"),
            Self::Stopped(error) => error.fmt(formatter),
        }
    }
}

impl Error for RecordingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CanonicalState(error) => Some(error),
            Self::Encode(error) => Some(error),
            Self::Write(error) | Self::Flush(error) => Some(error),
            Self::Stopped(error) => Some(error),
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
