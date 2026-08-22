use std::{error::Error, fmt};

/// Source position and record identity known when parsing failed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParseContext {
    pub line: Option<usize>,
    pub sequence: Option<u64>,
    pub step: Option<u64>,
    pub event_index: Option<u64>,
    pub path: Option<String>,
}

/// A cross-record rule that a transcript did not satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LifecycleError {
    UnexpectedRecord {
        expected: &'static str,
        found: &'static str,
    },
    Sequence {
        expected: u64,
        found: u64,
    },
    Step {
        expected: u64,
        found: u64,
    },
    EventIndex {
        expected: u64,
        found: u64,
    },
    EventCount {
        expected: u64,
        found: u64,
    },
    EventWithoutAction,
    ActionBeforeStepResult,
    RejectedStepHasEvents,
    RejectedDigestMismatch,
    InitialStateDigestMismatch,
    InvalidInitialStatus,
    MissingGameEnded,
    MultipleGameEnded,
    RecordAfterTerminalStep,
    FinalStateDigestMismatch,
    FinalStateNotEnded,
    OutcomeMismatch,
    CompletionStepCount {
        expected: u64,
        found: u64,
    },
    CompletionEventCount {
        expected: u64,
        found: u64,
    },
    CompletionDigestMismatch,
    RecordAfterCompletion,
    Incomplete {
        expected: &'static str,
    },
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedRecord { expected, found } => {
                write!(formatter, "expected {expected}, found {found}")
            }
            Self::Sequence { expected, found } => {
                write!(formatter, "expected sequence {expected}, found {found}")
            }
            Self::Step { expected, found } => {
                write!(formatter, "expected step {expected}, found {found}")
            }
            Self::EventIndex { expected, found } => {
                write!(formatter, "expected event index {expected}, found {found}")
            }
            Self::EventCount { expected, found } => {
                write!(formatter, "expected event count {expected}, found {found}")
            }
            Self::EventWithoutAction => write!(formatter, "an event has no preceding action"),
            Self::ActionBeforeStepResult => {
                write!(formatter, "an action appears before the prior step result")
            }
            Self::RejectedStepHasEvents => write!(formatter, "a rejected step has events"),
            Self::RejectedDigestMismatch => {
                write!(
                    formatter,
                    "a rejected step changed the authoritative digest"
                )
            }
            Self::InitialStateDigestMismatch => {
                write!(
                    formatter,
                    "the initial state digest does not match the initial state"
                )
            }
            Self::InvalidInitialStatus => write!(formatter, "the initial state is not playing"),
            Self::MissingGameEnded => write!(formatter, "the transcript has no GameEnded event"),
            Self::MultipleGameEnded => {
                write!(
                    formatter,
                    "the transcript has more than one GameEnded event"
                )
            }
            Self::RecordAfterTerminalStep => {
                write!(
                    formatter,
                    "a record appears after the terminal accepted step"
                )
            }
            Self::FinalStateDigestMismatch => {
                write!(formatter, "the final state digest is inconsistent")
            }
            Self::FinalStateNotEnded => write!(formatter, "the final state is not ended"),
            Self::OutcomeMismatch => write!(formatter, "the recorded outcomes do not match"),
            Self::CompletionStepCount { expected, found } => {
                write!(
                    formatter,
                    "expected completion step count {expected}, found {found}"
                )
            }
            Self::CompletionEventCount { expected, found } => {
                write!(
                    formatter,
                    "expected completion event count {expected}, found {found}"
                )
            }
            Self::CompletionDigestMismatch => {
                write!(
                    formatter,
                    "the completion digest does not match the final state"
                )
            }
            Self::RecordAfterCompletion => {
                write!(formatter, "a record appears after match completion")
            }
            Self::Incomplete { expected } => {
                write!(formatter, "the transcript ended before {expected}")
            }
        }
    }
}

/// Why strict transcript parsing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseErrorKind {
    Read { message: String },
    InvalidUtf8,
    MalformedJson { message: String },
    InvalidRecord { message: String },
    InvalidEntityId { value: String },
    InvalidStateDigest { value: String },
    UnsupportedFormat { found: String },
    UnsupportedVersion { found: u32 },
    Lifecycle(LifecycleError),
}

impl fmt::Display for ParseErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { message } => write!(formatter, "transcript read failed: {message}"),
            Self::InvalidUtf8 => write!(formatter, "the record is not valid UTF-8"),
            Self::MalformedJson { message } => write!(formatter, "malformed JSON: {message}"),
            Self::InvalidRecord { message } => write!(formatter, "invalid record: {message}"),
            Self::InvalidEntityId { value } => {
                write!(formatter, "entity ID is not canonical UUID text: {value}")
            }
            Self::InvalidStateDigest { value } => {
                write!(
                    formatter,
                    "state digest is not canonical SHA-256 text: {value}"
                )
            }
            Self::UnsupportedFormat { found } => {
                write!(formatter, "unsupported transcript format: {found}")
            }
            Self::UnsupportedVersion { found } => {
                write!(formatter, "unsupported transcript version: {found}")
            }
            Self::Lifecycle(error) => error.fmt(formatter),
        }
    }
}

/// A strict decoding or lifecycle failure with available source context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    kind: ParseErrorKind,
    context: Box<ParseContext>,
}

impl ParseError {
    pub(crate) fn new(kind: ParseErrorKind, context: ParseContext) -> Self {
        Self {
            kind,
            context: Box::new(context),
        }
    }

    #[must_use]
    pub fn kind(&self) -> &ParseErrorKind {
        &self.kind
    }

    #[must_use]
    pub fn context(&self) -> &ParseContext {
        &self.context
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let context = &self.context;
        let mut parts = Vec::new();
        if let Some(line) = context.line {
            parts.push(format!("line {line}"));
        }
        if let Some(sequence) = context.sequence {
            parts.push(format!("sequence {sequence}"));
        }
        if let Some(step) = context.step {
            parts.push(format!("step {step}"));
        }
        if let Some(index) = context.event_index {
            parts.push(format!("event index {index}"));
        }
        if let Some(path) = &context.path {
            parts.push(format!("path {path}"));
        }
        if parts.is_empty() {
            self.kind.fmt(formatter)
        } else {
            write!(formatter, "{}: {}", parts.join(", "), self.kind)
        }
    }
}

impl Error for ParseError {}
