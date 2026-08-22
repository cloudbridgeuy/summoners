use std::io::Write;

use summoners_core::{
    domain::{
        actions::GameAction,
        errors::ActionError,
        events::GameEvent,
        state::{GameOutcome, GameState, GameStatus},
    },
    engine::apply::{ActionOutcome, apply},
};

use crate::{
    codec::encode_record,
    error::{RecordingError, RecordingStopped, TerminalEventError},
    state::{StateDigestV1, StateProjectionV1},
    wire::{
        ActionRecordKindV1, ActionRecordV1, ActionV1, ErrorV1, EventRecordKindV1, EventRecordV1,
        EventV1, FinalStateRecordKindV1, FinalStateV1, HeaderMetadataV1, HeaderV1,
        MatchCompletedRecordKindV1, MatchCompletedV1, MatchCreatedV1, RecordV1, SetRequirementV1,
        StepCompletedRecordKindV1, StepCompletedV1, StepRejectedRecordKindV1, StepRejectedV1,
    },
};

/// The engine result recorded for one submitted action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedStep {
    Accepted { events: Vec<GameEvent> },
    Rejected { error: ActionError },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    Active,
    Stopped(RecordingStopped),
}

/// An append-only, fail-closed match recording.
pub struct RecordedMatch<W> {
    writer: W,
    state: GameState,
    next_sequence: u64,
    next_step: u64,
    total_events: u64,
    lifecycle: Lifecycle,
}

impl<W: Write> RecordedMatch<W> {
    /// Write and flush the initial transcript records before an active handle
    /// becomes available.
    pub fn start(
        mut writer: W,
        metadata: HeaderMetadataV1,
        required_sets: Vec<SetRequirementV1>,
        initial_state: GameState,
    ) -> Result<Self, RecordingError> {
        check_initial_status(&initial_state).map_err(RecordingError::Stopped)?;
        let (header_record, match_created_record) =
            prepare_start(metadata, required_sets, &initial_state)?;
        let header = encode_record(&header_record)?;
        let created = encode_record(&match_created_record)?;

        write_record(&mut writer, &header)?;
        write_record(&mut writer, &created)?;
        flush_checkpoint(&mut writer)?;

        Ok(Self {
            writer,
            state: initial_state,
            next_sequence: 2,
            next_step: 1,
            total_events: 0,
            lifecycle: Lifecycle::Active,
        })
    }

    /// Record one normalized action and the exact result returned by the
    /// existing engine transition.
    pub fn submit(&mut self, action: &GameAction) -> Result<RecordedStep, RecordingError> {
        self.submit_with(action, apply)
    }

    /// Read the authoritative state owned by this recording.
    #[must_use]
    pub const fn state(&self) -> &GameState {
        &self.state
    }

    /// Return the caller's writer after recording work is complete.
    #[must_use]
    pub fn into_writer(self) -> W {
        self.writer
    }

    fn submit_with(
        &mut self,
        action: &GameAction,
        apply_action: impl FnOnce(&GameState, &GameAction) -> Result<ActionOutcome, ActionError>,
    ) -> Result<RecordedStep, RecordingError> {
        self.ensure_active()?;

        let step = self.next_step;
        let action_record = prepare_action(self.next_sequence, step, action);
        self.append(&action_record)?;
        self.next_step += 1;

        match apply_action(&self.state, action) {
            Ok(outcome) => self.record_accepted_step(step, outcome),
            Err(error) => self.record_rejected_step(step, error),
        }
    }

    fn record_accepted_step(
        &mut self,
        step: u64,
        outcome: ActionOutcome,
    ) -> Result<RecordedStep, RecordingError> {
        self.state = outcome.state;
        let projection = StateProjectionV1::from_state(&self.state);
        let digest = self.compute_digest(&projection)?;
        let records =
            prepare_accepted_step(self.next_sequence, step, &outcome.events, digest.clone());

        self.append_records(records)?;
        self.flush()?;
        self.total_events += outcome.events.len() as u64;

        let status = self.state.status;
        let terminal_outcome = check_terminal_events(status, &outcome.events).map_err(|error| {
            let reason = RecordingStopped::InvalidTerminalEvents(error);
            self.lifecycle = Lifecycle::Stopped(reason);
            RecordingError::Stopped(reason)
        })?;

        match terminal_outcome {
            Some(outcome) => {
                self.complete_ended_match(outcome, projection, digest)?;
            }
            None if matches!(status, GameStatus::Broken(_)) => {
                self.lifecycle = Lifecycle::Stopped(RecordingStopped::GameBroken);
                return Err(RecordingError::Stopped(RecordingStopped::GameBroken));
            }
            None => {}
        }

        Ok(RecordedStep::Accepted {
            events: outcome.events,
        })
    }

    fn record_rejected_step(
        &mut self,
        step: u64,
        error: ActionError,
    ) -> Result<RecordedStep, RecordingError> {
        let projection = StateProjectionV1::from_state(&self.state);
        let digest = self.compute_digest(&projection)?;
        let record = prepare_rejected_step(self.next_sequence, step, error, digest);
        self.append(&record)?;
        self.flush()?;

        Ok(RecordedStep::Rejected { error })
    }

    fn complete_ended_match(
        &mut self,
        outcome: GameOutcome,
        projection: StateProjectionV1,
        digest: StateDigestV1,
    ) -> Result<(), RecordingError> {
        let records = prepare_completion(
            CompletionCounts {
                sequence: self.next_sequence,
                steps: self.next_step - 1,
                events: self.total_events,
            },
            outcome,
            projection,
            digest,
        );
        self.append_records(records)?;
        self.flush()?;
        self.lifecycle = Lifecycle::Stopped(RecordingStopped::MatchCompleted);
        Ok(())
    }

    fn ensure_active(&self) -> Result<(), RecordingError> {
        match self.lifecycle {
            Lifecycle::Active => Ok(()),
            Lifecycle::Stopped(reason) => Err(RecordingError::Stopped(reason)),
        }
    }

    fn compute_digest(
        &mut self,
        projection: &StateProjectionV1,
    ) -> Result<StateDigestV1, RecordingError> {
        StateDigestV1::compute(projection).map_err(|error| self.stop(error.into()))
    }

    fn append(&mut self, record: &RecordV1) -> Result<(), RecordingError> {
        let bytes = encode_record(record).map_err(|error| self.stop(error.into()))?;
        write_record(&mut self.writer, &bytes).map_err(|error| self.stop(error))?;
        self.next_sequence += 1;
        Ok(())
    }

    fn append_records(
        &mut self,
        records: impl IntoIterator<Item = RecordV1>,
    ) -> Result<(), RecordingError> {
        for record in records {
            self.append(&record)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), RecordingError> {
        flush_checkpoint(&mut self.writer).map_err(|error| self.stop(error))
    }

    fn stop(&mut self, error: RecordingError) -> RecordingError {
        self.lifecycle = Lifecycle::Stopped(RecordingStopped::RecordingFailed);
        error
    }
}

fn check_initial_status(state: &GameState) -> Result<(), RecordingStopped> {
    match state.status {
        GameStatus::Playing => Ok(()),
        GameStatus::Ended(_) => Err(RecordingStopped::GameAlreadyEnded),
        GameStatus::Broken(_) => Err(RecordingStopped::GameBroken),
    }
}

fn check_terminal_events(
    status: GameStatus,
    events: &[GameEvent],
) -> Result<Option<GameOutcome>, TerminalEventError> {
    if matches!(status, GameStatus::Broken(_)) {
        return Ok(None);
    }
    let mut ended_events = events.iter().filter_map(|event| match event {
        GameEvent::GameEnded { winner, reason } => Some(GameOutcome {
            winner: *winner,
            reason: *reason,
        }),
        _ => None,
    });
    let first = ended_events.next();
    if ended_events.next().is_some() {
        return Err(TerminalEventError::MultipleGameEnded);
    }

    match (status, first) {
        (GameStatus::Playing, None) => Ok(None),
        (GameStatus::Playing, Some(_)) => Err(TerminalEventError::UnexpectedGameEnded),
        (GameStatus::Ended(_), None) => Err(TerminalEventError::MissingGameEnded),
        (GameStatus::Ended(expected), Some(recorded)) if expected == recorded => Ok(Some(expected)),
        (GameStatus::Ended(_), Some(_)) => Err(TerminalEventError::OutcomeMismatch),
        (GameStatus::Broken(_), _) => Ok(None),
    }
}

fn prepare_start(
    metadata: HeaderMetadataV1,
    required_sets: Vec<SetRequirementV1>,
    state: &GameState,
) -> Result<(RecordV1, RecordV1), RecordingError> {
    let projection = StateProjectionV1::from_state(state);
    let digest = StateDigestV1::compute(&projection)?;
    Ok((
        RecordV1::Header(HeaderV1::new(metadata)),
        RecordV1::MatchCreated(Box::new(MatchCreatedV1::new(
            required_sets,
            projection,
            digest,
        ))),
    ))
}

fn prepare_action(sequence: u64, step: u64, action: &GameAction) -> RecordV1 {
    RecordV1::Action(ActionRecordV1 {
        sequence,
        record: ActionRecordKindV1::Action,
        step,
        action: ActionV1::from(action),
    })
}

fn prepare_accepted_step(
    sequence: u64,
    step: u64,
    events: &[GameEvent],
    digest: StateDigestV1,
) -> Vec<RecordV1> {
    let mut records = events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            RecordV1::Event(EventRecordV1 {
                sequence: sequence + index as u64,
                record: EventRecordKindV1::Event,
                step,
                index: index as u64,
                event: EventV1::from(event),
            })
        })
        .collect::<Vec<_>>();
    records.push(RecordV1::StepCompleted(StepCompletedV1 {
        sequence: sequence + events.len() as u64,
        record: StepCompletedRecordKindV1::StepCompleted,
        step,
        event_count: events.len() as u64,
        state_digest: digest,
    }));
    records
}

fn prepare_rejected_step(
    sequence: u64,
    step: u64,
    error: ActionError,
    digest: StateDigestV1,
) -> RecordV1 {
    RecordV1::StepRejected(StepRejectedV1 {
        sequence,
        record: StepRejectedRecordKindV1::StepRejected,
        step,
        error: ErrorV1::from(error),
        state_digest: digest,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompletionCounts {
    sequence: u64,
    steps: u64,
    events: u64,
}

fn prepare_completion(
    counts: CompletionCounts,
    outcome: GameOutcome,
    projection: StateProjectionV1,
    digest: StateDigestV1,
) -> [RecordV1; 2] {
    [
        RecordV1::FinalState(Box::new(FinalStateV1 {
            sequence: counts.sequence,
            record: FinalStateRecordKindV1::FinalState,
            final_state: projection,
            state_digest: digest.clone(),
        })),
        RecordV1::MatchCompleted(MatchCompletedV1 {
            sequence: counts.sequence + 1,
            record: MatchCompletedRecordKindV1::MatchCompleted,
            step_count: counts.steps,
            event_count: counts.events,
            state_digest: digest,
            winner: outcome.winner.into(),
            reason: outcome.reason.into(),
        }),
    ]
}

fn write_record(writer: &mut impl Write, bytes: &[u8]) -> Result<(), RecordingError> {
    writer.write_all(bytes).map_err(RecordingError::Write)
}

fn flush_checkpoint(writer: &mut impl Write) -> Result<(), RecordingError> {
    writer.flush().map_err(RecordingError::Flush)
}

#[cfg(test)]
mod tests;
