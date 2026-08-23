//! The `verify` command: replay a transcript and confirm it reproduces
//! itself exactly.

use std::{fs::File, io::BufReader, path::Path};

use summoners_cards::CardLibrary;
use summoners_match_log::TranscriptV1;
use summoners_match_log::replay::{ReplayError, verify_parsed_transcript};

use crate::error::ShellError;

const COMMAND: &str = "verify";

/// The step and event counts a completed verification confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifySummary {
    pub steps: u64,
    pub events: u64,
}

/// Open, parse, and replay one transcript through the current catalog,
/// confirming it reproduces itself exactly.
///
/// The transcript is parsed once into a `TranscriptV1` value; that same
/// value is both replayed and used to read the step and event counts, so
/// the counts in the success line always describe the bytes that were
/// actually verified.
pub fn run_verify(path: &Path, library: &CardLibrary) -> Result<VerifySummary, ShellError> {
    let transcript = parse_transcript(path)?;
    verify_parsed_transcript(&transcript, library).map_err(|source| ShellError::Transcript {
        command: COMMAND,
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    Ok(VerifySummary {
        steps: transcript.match_completed.step_count,
        events: transcript.match_completed.event_count,
    })
}

fn parse_transcript(path: &Path) -> Result<TranscriptV1, ShellError> {
    let reader = open(path)?;
    TranscriptV1::parse(reader).map_err(|error| ShellError::Transcript {
        command: COMMAND,
        path: path.to_path_buf(),
        source: Box::new(ReplayError::Parse(error)),
    })
}

fn open(path: &Path) -> Result<BufReader<File>, ShellError> {
    let file = File::open(path).map_err(|source| ShellError::Io {
        command: COMMAND,
        path: path.to_path_buf(),
        source,
    })?;
    Ok(BufReader::new(file))
}
