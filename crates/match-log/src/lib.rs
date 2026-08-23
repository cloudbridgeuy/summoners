//! Versioned match transcript values and recording adapters.
//!
//! Wire conversion, canonical state bytes, and digests are pure. The
//! `RecordedMatch` adapter is a small imperative shell that owns a caller's
//! writer and appends checkpoints selected by the pure core.
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod codec;
pub mod compare;
pub mod error;
pub mod parse;
mod parse_error;
pub mod record;
pub mod replay;
pub mod schema;
pub mod state;
pub mod wire;

pub use error::{
    CanonicalStateError, EncodeError, RecordingError, RecordingStopped, StateRebuildError,
    TerminalEventError, WireConversionError,
};
pub use parse::{TranscriptStepResultV1, TranscriptStepV1, TranscriptV1};
pub use parse_error::{LifecycleError, ParseContext, ParseError, ParseErrorKind};
pub use record::{RecordedMatch, RecordedStep};
pub use replay::{
    PreparedAction, PreparedScenario, prepare_scenario, verify_parsed_transcript, verify_transcript,
};
pub use state::{StateDigestV1, StateProjectionV1};
pub use wire::{
    ActionV1, ErrorV1, EventV1, HeaderMetadataV1, HeaderV1, MatchCreatedV1, RecordV1,
    SetRequirementV1,
};
