//! Strict transcript replay through the current engine.

use std::{error::Error, fmt, io::BufRead, sync::Arc};

use summoners_cards::CardLibrary;
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::CardSet,
        errors::ActionError,
        events::GameEvent,
        state::{GameState, GameStatus},
    },
    engine::apply::{ActionOutcome, apply},
};

use crate::{
    CanonicalStateError, ParseError, StateDigestV1, StateProjectionV1, StateRebuildError,
    TranscriptStepResultV1, TranscriptStepV1, TranscriptV1, WireConversionError,
    wire::{
        ErrorV1, EventV1, GameOutcomeV1, LossReasonV1, MatchCompletedV1, PlayerIdV1,
        SetRequirementV1,
    },
};

/// The replay stage that found an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayPhase {
    Requirements,
    InitialState,
    Step,
    Completion,
}

/// The exact semantic location of a replay error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayLocation {
    pub phase: ReplayPhase,
    pub step: Option<u64>,
    pub event_index: Option<u64>,
    pub path: &'static str,
}

impl ReplayLocation {
    const fn requirements(path: &'static str) -> Self {
        Self {
            phase: ReplayPhase::Requirements,
            step: None,
            event_index: None,
            path,
        }
    }

    const fn initial(path: &'static str) -> Self {
        Self {
            phase: ReplayPhase::InitialState,
            step: None,
            event_index: None,
            path,
        }
    }

    const fn step(step: u64, event_index: Option<u64>, path: &'static str) -> Self {
        Self {
            phase: ReplayPhase::Step,
            step: Some(step),
            event_index,
            path,
        }
    }

    const fn completion(path: &'static str) -> Self {
        Self {
            phase: ReplayPhase::Completion,
            step: None,
            event_index: None,
            path,
        }
    }
}

/// Whether one replayed action was accepted or rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayStepResult {
    Accepted,
    Rejected(ErrorV1),
}

/// Typed expected and actual values for one semantic replay difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayDivergenceKind {
    SetRevision {
        set: String,
        expected: u32,
        actual: Option<u32>,
    },
    InitialProjection {
        expected: StateProjectionV1,
        actual: StateProjectionV1,
    },
    StepResult {
        expected: ReplayStepResult,
        actual: ReplayStepResult,
    },
    Event {
        expected: Option<EventV1>,
        actual: Option<EventV1>,
    },
    RejectedError {
        expected: ErrorV1,
        actual: ErrorV1,
    },
    RejectedState {
        expected: StateProjectionV1,
        actual: StateProjectionV1,
    },
    CardSetIdentity,
    StateDigest {
        expected: StateDigestV1,
        actual: StateDigestV1,
    },
    FinalState {
        expected: StateProjectionV1,
        actual: StateProjectionV1,
    },
    GameEndedCount {
        expected: u64,
        actual: u64,
    },
    GameEndedOutcome {
        expected: GameOutcomeV1,
        actual: GameOutcomeV1,
    },
    Winner {
        expected: PlayerIdV1,
        actual: PlayerIdV1,
    },
    LossReason {
        expected: LossReasonV1,
        actual: LossReasonV1,
    },
    StepCount {
        expected: u64,
        actual: u64,
    },
    EventCount {
        expected: u64,
        actual: u64,
    },
}

/// The first semantic difference found during replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDivergence {
    pub location: ReplayLocation,
    pub kind: ReplayDivergenceKind,
}

/// Why a transcript could not be replayed and verified.
#[derive(Debug)]
pub enum ReplayError {
    Parse(ParseError),
    StateRebuild {
        location: ReplayLocation,
        error: StateRebuildError,
    },
    WireConversion {
        location: ReplayLocation,
        error: WireConversionError,
    },
    CanonicalState {
        location: ReplayLocation,
        error: CanonicalStateError,
    },
    Divergence(Box<ReplayDivergence>),
}

impl ReplayError {
    #[must_use]
    pub fn location(&self) -> Option<&ReplayLocation> {
        match self {
            Self::Parse(_) => None,
            Self::StateRebuild { location, .. }
            | Self::WireConversion { location, .. }
            | Self::CanonicalState { location, .. } => Some(location),
            Self::Divergence(divergence) => Some(&divergence.location),
        }
    }
}

impl fmt::Display for ReplayPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Requirements => "requirements",
            Self::InitialState => "initial state",
            Self::Step => "step",
            Self::Completion => "completion",
        };
        formatter.write_str(name)
    }
}

impl fmt::Display for ReplayLocation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.phase)?;
        if let Some(step) = self.step {
            write!(formatter, " {step}")?;
        }
        if let Some(event_index) = self.event_index {
            write!(formatter, ", event {event_index}")?;
        }
        write!(formatter, ", path {}", self.path)
    }
}

impl fmt::Display for ReplayStepResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => formatter.write_str("accepted"),
            Self::Rejected(_) => formatter.write_str("rejected"),
        }
    }
}

impl fmt::Display for ReplayDivergence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: ", self.location)?;
        match &self.kind {
            ReplayDivergenceKind::SetRevision {
                set,
                expected,
                actual,
            } => match actual {
                Some(actual) => write!(
                    formatter,
                    "Set {set} requires revision {expected}, but revision {actual} is loaded"
                ),
                None => write!(
                    formatter,
                    "Set {set} requires revision {expected}, but it is not loaded"
                ),
            },
            ReplayDivergenceKind::InitialProjection { .. } => {
                formatter.write_str("the rebuilt initial state differs from the transcript")
            }
            ReplayDivergenceKind::StepResult { expected, actual } => {
                write!(formatter, "expected {expected}, found {actual}")
            }
            ReplayDivergenceKind::Event { .. } => formatter.write_str("the ordered event differs"),
            ReplayDivergenceKind::RejectedError { .. } => {
                formatter.write_str("the rejection error differs")
            }
            ReplayDivergenceKind::RejectedState { .. } => {
                formatter.write_str("the rejected action changed the state")
            }
            ReplayDivergenceKind::CardSetIdentity => {
                formatter.write_str("the rejected action changed the shared card pool")
            }
            ReplayDivergenceKind::StateDigest { expected, actual } => write!(
                formatter,
                "expected state digest {}, found {}",
                expected.0, actual.0
            ),
            ReplayDivergenceKind::FinalState { .. } => {
                formatter.write_str("the final state differs")
            }
            ReplayDivergenceKind::GameEndedCount { expected, actual } => write!(
                formatter,
                "expected {expected} GameEnded event, found {actual}"
            ),
            ReplayDivergenceKind::GameEndedOutcome { .. } => {
                formatter.write_str("the GameEnded outcome differs")
            }
            ReplayDivergenceKind::Winner { expected, actual } => write!(
                formatter,
                "expected winner {}, found {}",
                player_name(*expected),
                player_name(*actual)
            ),
            ReplayDivergenceKind::LossReason { expected, actual } => write!(
                formatter,
                "expected loss reason {}, found {}",
                reason_name(*expected),
                reason_name(*actual)
            ),
            ReplayDivergenceKind::StepCount { expected, actual } => {
                write!(formatter, "expected {expected} steps, found {actual}")
            }
            ReplayDivergenceKind::EventCount { expected, actual } => {
                write!(formatter, "expected {expected} events, found {actual}")
            }
        }
    }
}

impl fmt::Display for ReplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(formatter, "transcript parse failed: {error}"),
            Self::StateRebuild { location, error } => {
                write!(formatter, "{location}: state rebuild failed: {error}")
            }
            Self::WireConversion { location, error } => {
                write!(formatter, "{location}: wire conversion failed: {error}")
            }
            Self::CanonicalState { location, error } => {
                write!(formatter, "{location}: {error}")
            }
            Self::Divergence(divergence) => divergence.fmt(formatter),
        }
    }
}

impl Error for ReplayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Parse(error) => Some(error),
            Self::StateRebuild { error, .. } => Some(error),
            Self::WireConversion { error, .. } => Some(error),
            Self::CanonicalState { error, .. } => Some(error),
            Self::Divergence(_) => None,
        }
    }
}

/// Parse one complete transcript and replay it through the current engine.
pub fn verify_transcript(reader: impl BufRead, library: &CardLibrary) -> Result<(), ReplayError> {
    let transcript = TranscriptV1::parse(reader).map_err(ReplayError::Parse)?;
    verify_parsed_transcript(&transcript, library)
}

fn verify_parsed_transcript(
    transcript: &TranscriptV1,
    library: &CardLibrary,
) -> Result<(), ReplayError> {
    let initial_state = prepare_initial_state_with(
        &transcript.match_created.required_sets,
        &transcript.match_created.initial_state,
        &transcript.match_created.state_digest,
        |set| library.set_revision(set),
        || library.core_cards(),
    )?;
    let (state, facts) = replay_steps_with(&transcript.steps, initial_state, |state, action| {
        apply(state, action)
    })?;
    verify_completion(transcript, &state, &facts)
}

fn prepare_initial_state_with(
    requirements: &[SetRequirementV1],
    projection: &StateProjectionV1,
    digest: &StateDigestV1,
    mut set_revision: impl FnMut(&str) -> Option<u32>,
    core_cards: impl FnOnce() -> Arc<CardSet>,
) -> Result<GameState, ReplayError> {
    validate_required_sets_with(requirements, &mut set_revision)?;
    let state = projection
        .clone()
        .into_game_state(core_cards())
        .map_err(|error| ReplayError::StateRebuild {
            location: ReplayLocation::initial("match_created.initial_state"),
            error,
        })?;
    verify_initial_state(projection, digest, &state)?;
    Ok(state)
}

fn validate_required_sets_with(
    requirements: &[SetRequirementV1],
    mut set_revision: impl FnMut(&str) -> Option<u32>,
) -> Result<(), ReplayError> {
    for requirement in requirements {
        let actual = set_revision(&requirement.set);
        if actual != Some(requirement.revision) {
            return Err(divergence(
                ReplayLocation::requirements("match_created.required_sets.revision"),
                ReplayDivergenceKind::SetRevision {
                    set: requirement.set.clone(),
                    expected: requirement.revision,
                    actual,
                },
            ));
        }
    }
    Ok(())
}

fn verify_initial_state(
    expected_projection: &StateProjectionV1,
    expected_digest: &StateDigestV1,
    state: &GameState,
) -> Result<(), ReplayError> {
    let actual_projection = StateProjectionV1::from_state(state);
    if &actual_projection != expected_projection {
        return Err(divergence(
            ReplayLocation::initial("match_created.initial_state"),
            ReplayDivergenceKind::InitialProjection {
                expected: expected_projection.clone(),
                actual: actual_projection,
            },
        ));
    }
    verify_digest(
        expected_digest,
        &actual_projection,
        ReplayLocation::initial("match_created.state_digest"),
    )
}

#[derive(Debug, Default)]
struct ReplayFacts {
    step_count: u64,
    event_count: u64,
    game_ended: Vec<GameOutcomeV1>,
}

fn replay_steps_with(
    steps: &[TranscriptStepV1],
    mut state: GameState,
    mut engine: impl FnMut(&mut GameState, &GameAction) -> Result<ActionOutcome, ActionError>,
) -> Result<(GameState, ReplayFacts), ReplayError> {
    let mut facts = ReplayFacts::default();
    let mut digest = compute_digest(
        &StateProjectionV1::from_state(&state),
        ReplayLocation::initial("match_created.state_digest"),
    )?;

    for step in steps {
        let step_number = step.action.step;
        let action = GameAction::try_from(step.action.action.clone()).map_err(|error| {
            ReplayError::WireConversion {
                location: ReplayLocation::step(step_number, None, "action"),
                error,
            }
        })?;
        let before = state.clone();
        let before_digest = digest.clone();
        let result = engine(&mut state, &action);

        match (&step.result, result) {
            (TranscriptStepResultV1::Accepted { events, completion }, Ok(outcome)) => {
                digest =
                    verify_accepted_step(step_number, events, &completion.state_digest, &outcome)?;
                record_events(&mut facts, &outcome.events);
                state = outcome.state;
            }
            (TranscriptStepResultV1::Accepted { .. }, Err(error)) => {
                return Err(divergence(
                    ReplayLocation::step(step_number, None, "result"),
                    ReplayDivergenceKind::StepResult {
                        expected: ReplayStepResult::Accepted,
                        actual: ReplayStepResult::Rejected(ErrorV1::from(error)),
                    },
                ));
            }
            (TranscriptStepResultV1::Rejected { rejection }, Err(error)) => {
                verify_rejected_step(RejectedStepCheck {
                    step_number,
                    rejection,
                    actual_error: error,
                    before: &before,
                    after: &state,
                    before_digest: &before_digest,
                })?;
                digest = before_digest;
            }
            (TranscriptStepResultV1::Rejected { rejection }, Ok(_)) => {
                return Err(divergence(
                    ReplayLocation::step(step_number, None, "result"),
                    ReplayDivergenceKind::StepResult {
                        expected: ReplayStepResult::Rejected(rejection.error.clone()),
                        actual: ReplayStepResult::Accepted,
                    },
                ));
            }
        }
        facts.step_count += 1;
    }

    Ok((state, facts))
}

fn verify_accepted_step(
    step: u64,
    expected_events: &[crate::wire::EventRecordV1],
    expected_digest: &StateDigestV1,
    outcome: &ActionOutcome,
) -> Result<StateDigestV1, ReplayError> {
    let event_count = expected_events.len().max(outcome.events.len());
    for index in 0..event_count {
        let expected = expected_events.get(index);
        let actual = outcome.events.get(index);
        let event_index = index as u64;
        let expected_core = expected
            .map(|record| GameEvent::try_from(record.event.clone()))
            .transpose()
            .map_err(|error| ReplayError::WireConversion {
                location: ReplayLocation::step(step, Some(event_index), "events.event"),
                error,
            })?;
        if expected_core.as_ref() != actual {
            return Err(divergence(
                ReplayLocation::step(step, Some(event_index), "events.event"),
                ReplayDivergenceKind::Event {
                    expected: expected.map(|record| record.event.clone()),
                    actual: actual.map(EventV1::from),
                },
            ));
        }
    }

    let projection = StateProjectionV1::from_state(&outcome.state);
    verify_digest(
        expected_digest,
        &projection,
        ReplayLocation::step(step, None, "state_digest"),
    )?;
    compute_digest(
        &projection,
        ReplayLocation::step(step, None, "state_digest"),
    )
}

#[derive(Clone, Copy)]
struct RejectedStepCheck<'a> {
    step_number: u64,
    rejection: &'a crate::wire::StepRejectedV1,
    actual_error: ActionError,
    before: &'a GameState,
    after: &'a GameState,
    before_digest: &'a StateDigestV1,
}

fn verify_rejected_step(check: RejectedStepCheck<'_>) -> Result<(), ReplayError> {
    let RejectedStepCheck {
        step_number,
        rejection,
        actual_error,
        before,
        after,
        before_digest,
    } = check;
    let expected_error = ActionError::from(rejection.error.clone());
    if actual_error != expected_error {
        return Err(divergence(
            ReplayLocation::step(step_number, None, "error"),
            ReplayDivergenceKind::RejectedError {
                expected: rejection.error.clone(),
                actual: ErrorV1::from(actual_error),
            },
        ));
    }

    let expected_projection = StateProjectionV1::from_state(before);
    let actual_projection = StateProjectionV1::from_state(after);
    if actual_projection != expected_projection {
        return Err(divergence(
            ReplayLocation::step(step_number, None, "state"),
            ReplayDivergenceKind::RejectedState {
                expected: expected_projection,
                actual: actual_projection,
            },
        ));
    }
    if !Arc::ptr_eq(&before.cards, &after.cards) {
        return Err(divergence(
            ReplayLocation::step(step_number, None, "state.cards"),
            ReplayDivergenceKind::CardSetIdentity,
        ));
    }
    verify_digest(
        before_digest,
        &StateProjectionV1::from_state(after),
        ReplayLocation::step(step_number, None, "state"),
    )?;
    if rejection.state_digest != *before_digest {
        return Err(divergence(
            ReplayLocation::step(step_number, None, "state_digest"),
            ReplayDivergenceKind::StateDigest {
                expected: rejection.state_digest.clone(),
                actual: before_digest.clone(),
            },
        ));
    }
    Ok(())
}

fn record_events(facts: &mut ReplayFacts, events: &[GameEvent]) {
    facts.event_count += events.len() as u64;
    facts.game_ended.extend(events.iter().filter_map(|event| {
        if let GameEvent::GameEnded { winner, reason } = event {
            Some(GameOutcomeV1 {
                winner: (*winner).into(),
                reason: (*reason).into(),
            })
        } else {
            None
        }
    }));
}

fn verify_completion(
    transcript: &TranscriptV1,
    state: &GameState,
    facts: &ReplayFacts,
) -> Result<(), ReplayError> {
    let actual_projection = StateProjectionV1::from_state(state);
    if actual_projection != transcript.final_state.final_state {
        return Err(divergence(
            ReplayLocation::completion("final_state.final_state"),
            ReplayDivergenceKind::FinalState {
                expected: transcript.final_state.final_state.clone(),
                actual: actual_projection,
            },
        ));
    }

    let game_ended_count = facts.game_ended.len() as u64;
    if game_ended_count != 1 {
        return Err(divergence(
            ReplayLocation::completion("events.game_ended"),
            ReplayDivergenceKind::GameEndedCount {
                expected: 1,
                actual: game_ended_count,
            },
        ));
    }
    let actual_outcome = facts.game_ended[0];
    let final_outcome = match state.status {
        GameStatus::Ended(outcome) => GameOutcomeV1::from(outcome),
        GameStatus::Playing | GameStatus::Broken(_) => {
            return Err(divergence(
                ReplayLocation::completion("final_state.final_state.status"),
                ReplayDivergenceKind::GameEndedCount {
                    expected: 1,
                    actual: 0,
                },
            ));
        }
    };
    if final_outcome != actual_outcome {
        return Err(divergence(
            ReplayLocation::completion("final_state.final_state.status.outcome"),
            ReplayDivergenceKind::GameEndedOutcome {
                expected: final_outcome,
                actual: actual_outcome,
            },
        ));
    }

    let completion = &transcript.match_completed;
    verify_winner_and_reason(completion, actual_outcome)?;
    if completion.step_count != facts.step_count {
        return Err(divergence(
            ReplayLocation::completion("match_completed.step_count"),
            ReplayDivergenceKind::StepCount {
                expected: completion.step_count,
                actual: facts.step_count,
            },
        ));
    }
    if completion.event_count != facts.event_count {
        return Err(divergence(
            ReplayLocation::completion("match_completed.event_count"),
            ReplayDivergenceKind::EventCount {
                expected: completion.event_count,
                actual: facts.event_count,
            },
        ));
    }
    verify_digest(
        &transcript.final_state.state_digest,
        &StateProjectionV1::from_state(state),
        ReplayLocation::completion("final_state.state_digest"),
    )?;
    verify_digest(
        &completion.state_digest,
        &StateProjectionV1::from_state(state),
        ReplayLocation::completion("match_completed.state_digest"),
    )
}

fn verify_winner_and_reason(
    completion: &MatchCompletedV1,
    actual: GameOutcomeV1,
) -> Result<(), ReplayError> {
    if completion.winner != actual.winner {
        return Err(divergence(
            ReplayLocation::completion("match_completed.winner"),
            ReplayDivergenceKind::Winner {
                expected: completion.winner,
                actual: actual.winner,
            },
        ));
    }
    if completion.reason != actual.reason {
        return Err(divergence(
            ReplayLocation::completion("match_completed.reason"),
            ReplayDivergenceKind::LossReason {
                expected: completion.reason,
                actual: actual.reason,
            },
        ));
    }
    Ok(())
}

fn verify_digest(
    expected: &StateDigestV1,
    projection: &StateProjectionV1,
    location: ReplayLocation,
) -> Result<(), ReplayError> {
    let actual = compute_digest(projection, location.clone())?;
    if actual == *expected {
        Ok(())
    } else {
        Err(divergence(
            location,
            ReplayDivergenceKind::StateDigest {
                expected: expected.clone(),
                actual,
            },
        ))
    }
}

fn compute_digest(
    projection: &StateProjectionV1,
    location: ReplayLocation,
) -> Result<StateDigestV1, ReplayError> {
    StateDigestV1::compute(projection)
        .map_err(|error| ReplayError::CanonicalState { location, error })
}

fn divergence(location: ReplayLocation, kind: ReplayDivergenceKind) -> ReplayError {
    ReplayError::Divergence(Box::new(ReplayDivergence { location, kind }))
}

const fn player_name(player: PlayerIdV1) -> &'static str {
    match player {
        PlayerIdV1::One => "one",
        PlayerIdV1::Two => "two",
    }
}

const fn reason_name(reason: LossReasonV1) -> &'static str {
    match reason {
        LossReasonV1::ThirdMainLoss => "third_main_loss",
        LossReasonV1::NoPromotionAvailable => "no_promotion_available",
        LossReasonV1::EmptyDeckDraw => "empty_deck_draw",
    }
}

#[cfg(test)]
mod tests;
