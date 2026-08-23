#![allow(clippy::expect_used)]

use std::{cell::Cell, io::Cursor};

use serde_json::Value;
use summoners_cards::{BuiltInCatalog, built_in_catalog};
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::CardSet,
        errors::ActionError,
        ids::{CardInstanceId, PlayerId},
        state::{CardRef, Coin, ManaBank, PerPlayer, Readiness},
    },
    scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario},
};

use super::*;
use crate::{HeaderMetadataV1, RecordedMatch};

fn player(main: CardRef) -> ScenarioPlayer {
    ScenarioPlayer {
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        main: Some(ScenarioSummon {
            chain: vec![main],
            damage: 0,
            readiness: Readiness::Ready,
        }),
        bench: [None, None, None],
    }
}

fn fixture() -> (Arc<BuiltInCatalog>, Vec<u8>) {
    let catalog = built_in_catalog().expect("the built-in catalog is valid");
    let state = from_scenario(
        catalog.library().core_cards(),
        &Scenario {
            players: PerPlayer::new(
                player(CardRef {
                    instance: CardInstanceId(1),
                    def: catalog.set_paths().starter(),
                }),
                player(CardRef {
                    instance: CardInstanceId(2),
                    def: catalog.barrow_herd().starter(),
                }),
            ),
            active_player: PlayerId::One,
            coin: None,
        },
    )
    .expect("the built-in game is valid");
    let revision = catalog
        .library()
        .set_revision("foundations")
        .expect("Foundations is loaded");
    let mut recording = RecordedMatch::start(
        Vec::new(),
        HeaderMetadataV1::new(),
        vec![SetRequirementV1 {
            set: "foundations".to_string(),
            revision,
        }],
        state,
    )
    .expect("the memory writer is available");

    for action in [
        GameAction::EndTurn {
            player: PlayerId::Two,
        },
        GameAction::EndTurn {
            player: PlayerId::One,
        },
        GameAction::PassPriority {
            player: PlayerId::Two,
        },
        GameAction::PassPriority {
            player: PlayerId::One,
        },
    ] {
        recording.submit(&action).expect("the action is recorded");
    }

    (catalog, recording.into_writer())
}

fn parsed(bytes: &[u8]) -> TranscriptV1 {
    TranscriptV1::parse(Cursor::new(bytes)).expect("the fixture transcript is valid")
}

fn initial_state(transcript: &TranscriptV1, catalog: &BuiltInCatalog) -> GameState {
    transcript
        .match_created
        .initial_state
        .clone()
        .into_game_state(catalog.library().core_cards())
        .expect("the fixture state rebuilds")
}

fn fake_digest() -> StateDigestV1 {
    StateDigestV1(format!("sha256:{}", "0".repeat(64)))
}

fn rewrite_first(bytes: &[u8], mut change: impl FnMut(&mut Value) -> bool) -> Vec<u8> {
    let mut changed = false;
    let mut rewritten = Vec::new();
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let mut value: Value = serde_json::from_slice(line).expect("record JSON");
        if !changed && change(&mut value) {
            changed = true;
        }
        rewritten.extend(serde_json::to_vec(&value).expect("record encodes"));
        rewritten.push(b'\n');
    }
    assert!(changed, "one record must change");
    rewritten
}

fn accepted_case(
    transcript: &TranscriptV1,
    catalog: &BuiltInCatalog,
    wanted_step: u64,
) -> (Vec<crate::wire::EventRecordV1>, ActionOutcome) {
    let mut state = initial_state(transcript, catalog);
    for step in &transcript.steps {
        let action =
            GameAction::try_from(step.action.action.clone()).expect("the fixture action converts");
        match apply(&state, &action) {
            Ok(outcome) if step.action.step == wanted_step => {
                let TranscriptStepResultV1::Accepted { events, .. } = &step.result else {
                    panic!("the wanted step must be accepted");
                };
                return (events.clone(), outcome);
            }
            Ok(outcome) => state = outcome.state,
            Err(_) => {}
        }
    }
    panic!("the accepted step must exist");
}

fn prepared_action(step: &TranscriptStepV1) -> PreparedAction {
    let action =
        GameAction::try_from(step.action.action.clone()).expect("the fixture action converts");
    PreparedAction {
        step: step.action.step,
        action,
    }
}

fn replayed(transcript: &TranscriptV1, catalog: &BuiltInCatalog) -> (GameState, ReplayFacts) {
    let actions: Vec<PreparedAction> = transcript.steps.iter().map(prepared_action).collect();
    replay_steps_with(
        &transcript.steps,
        &actions,
        initial_state(transcript, catalog),
        |state, action| apply(state, action),
    )
    .expect("the fixture replays")
}

fn assert_divergence(
    error: ReplayError,
    phase: ReplayPhase,
    step: Option<u64>,
    path: &'static str,
) -> ReplayDivergenceKind {
    let ReplayError::Divergence(divergence) = error else {
        panic!("expected replay divergence, found {error}");
    };
    let ReplayDivergence { location, kind } = *divergence;
    assert_eq!(location.phase, phase);
    assert_eq!(location.step, step);
    assert_eq!(location.path, path);
    kind
}

#[test]
fn real_built_in_match_replays_unchanged() {
    let (catalog, bytes) = fixture();

    verify_transcript(Cursor::new(bytes), catalog.library())
        .expect("the real transcript must replay");
}

#[test]
fn parser_failure_is_distinct_from_replay_divergence() {
    let (catalog, _) = fixture();
    let error = verify_transcript(Cursor::new(b"{}\n"), catalog.library())
        .expect_err("the input is not a transcript");

    assert!(matches!(error, ReplayError::Parse(_)));
    assert!(error.location().is_none());
}

#[test]
fn missing_or_wrong_set_stops_before_card_pool_access() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    for actual in [None, Some(99)] {
        let accessed = Cell::new(false);
        let error = prepare_initial_state_with(
            &transcript.match_created.required_sets,
            &transcript.match_created.initial_state,
            &transcript.match_created.state_digest,
            |_| actual,
            || {
                accessed.set(true);
                catalog.library().core_cards()
            },
        )
        .expect_err("the required revision differs");

        assert!(!accessed.get());
        assert!(matches!(
            assert_divergence(
                error,
                ReplayPhase::Requirements,
                None,
                "match_created.required_sets.revision"
            ),
            ReplayDivergenceKind::SetRevision { actual: found, .. } if found == actual
        ));
    }
}

#[test]
fn initial_projection_and_digest_report_initial_paths() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let mut state = initial_state(&transcript, &catalog);
    state.coin = Some(Coin);

    let projection_error = verify_initial_state(
        &transcript.match_created.initial_state,
        &transcript.match_created.state_digest,
        &state,
    )
    .expect_err("the rebuilt projection changed");
    assert!(matches!(
        assert_divergence(
            projection_error,
            ReplayPhase::InitialState,
            None,
            "match_created.initial_state"
        ),
        ReplayDivergenceKind::InitialProjection { .. }
    ));

    let state = initial_state(&transcript, &catalog);
    let digest_error = verify_initial_state(
        &transcript.match_created.initial_state,
        &fake_digest(),
        &state,
    )
    .expect_err("the digest changed");
    assert!(matches!(
        assert_divergence(
            digest_error,
            ReplayPhase::InitialState,
            None,
            "match_created.state_digest"
        ),
        ReplayDivergenceKind::StateDigest { .. }
    ));
}

#[test]
fn malformed_initial_projection_is_a_state_rebuild_error() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let mut projection = transcript.match_created.initial_state.clone();
    projection
        .players
        .one
        .main
        .as_mut()
        .expect("main")
        .chain
        .layers
        .clear();

    let error = prepare_initial_state_with(
        &transcript.match_created.required_sets,
        &projection,
        &transcript.match_created.state_digest,
        |set| catalog.library().set_revision(set),
        || catalog.library().core_cards(),
    )
    .expect_err("an empty upgrade chain cannot rebuild");

    assert!(matches!(error, ReplayError::StateRebuild { .. }));
    assert_eq!(
        error.location().expect("rebuild location").path,
        "match_created.initial_state"
    );
}

#[test]
fn prepare_scenario_rejects_a_set_revision_mismatch() {
    let (catalog, bytes) = fixture();
    let mut transcript = parsed(&bytes);
    transcript.match_created.required_sets[0].revision += 1;

    let error = prepare_scenario(&transcript, catalog.library())
        .expect_err("the mismatched revision must be rejected");

    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Requirements,
            None,
            "match_created.required_sets.revision"
        ),
        ReplayDivergenceKind::SetRevision { .. }
    ));
}

#[test]
fn prepare_scenario_rejects_a_state_rebuild_failure() {
    let (catalog, bytes) = fixture();
    let mut transcript = parsed(&bytes);
    transcript
        .match_created
        .initial_state
        .players
        .one
        .main
        .as_mut()
        .expect("main")
        .chain
        .layers
        .clear();

    let error = prepare_scenario(&transcript, catalog.library())
        .expect_err("an empty upgrade chain cannot rebuild");

    assert!(matches!(error, ReplayError::StateRebuild { .. }));
    assert_eq!(
        error.location().expect("rebuild location").path,
        "match_created.initial_state"
    );
}

#[test]
fn prepare_scenario_rejects_an_initial_digest_mismatch() {
    let (catalog, bytes) = fixture();
    let mut transcript = parsed(&bytes);
    transcript.match_created.state_digest = fake_digest();

    let error = prepare_scenario(&transcript, catalog.library()).expect_err("the digest changed");

    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::InitialState,
            None,
            "match_created.state_digest"
        ),
        ReplayDivergenceKind::StateDigest { .. }
    ));
}

#[test]
fn prepare_scenario_converts_every_action_eagerly() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);

    let scenario =
        prepare_scenario(&transcript, catalog.library()).expect("the fixture transcript prepares");

    let expected_steps: Vec<u64> = transcript
        .steps
        .iter()
        .map(|step| step.action.step)
        .collect();
    let actual_steps: Vec<u64> = scenario
        .actions
        .iter()
        .map(|prepared| prepared.step)
        .collect();
    assert_eq!(actual_steps, expected_steps);
    assert_eq!(
        scenario.required_sets,
        transcript.match_created.required_sets
    );
    assert_eq!(scenario.metadata, transcript.header.metadata);
}

#[test]
fn accepted_step_detects_changed_result_and_every_event_shape() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let accepted = transcript
        .steps
        .iter()
        .find(|step| step.action.step == 2)
        .expect("step two")
        .clone();
    let accepted_action = prepared_action(&accepted);
    let result_error = replay_steps_with(
        &[accepted],
        &[accepted_action],
        initial_state(&transcript, &catalog),
        |_, _| Err(ActionError::WrongPhase),
    )
    .expect_err("the accepted result changed");
    assert!(matches!(
        assert_divergence(result_error, ReplayPhase::Step, Some(2), "result"),
        ReplayDivergenceKind::StepResult { .. }
    ));

    let (events, outcome) = accepted_case(&transcript, &catalog, 4);
    let digest = StateDigestV1::compute(&StateProjectionV1::from_state(&outcome.state))
        .expect("state encodes");

    let mut missing = events.clone();
    missing.pop();
    let missing_error = verify_accepted_step(4, &missing, &digest, &outcome)
        .expect_err("one expected event is missing");
    assert!(matches!(
        assert_divergence(missing_error, ReplayPhase::Step, Some(4), "events.event"),
        ReplayDivergenceKind::Event {
            expected: None,
            actual: Some(_)
        }
    ));

    let mut extra = events.clone();
    extra.push(events[0].clone());
    let extra_error = verify_accepted_step(4, &extra, &digest, &outcome)
        .expect_err("one expected event is extra");
    assert!(matches!(
        assert_divergence(extra_error, ReplayPhase::Step, Some(4), "events.event"),
        ReplayDivergenceKind::Event {
            expected: Some(_),
            actual: None
        }
    ));

    let mut reordered = events.clone();
    reordered.swap(0, 1);
    let reordered_error =
        verify_accepted_step(4, &reordered, &digest, &outcome).expect_err("event order changed");
    assert!(matches!(
        assert_divergence(reordered_error, ReplayPhase::Step, Some(4), "events.event"),
        ReplayDivergenceKind::Event { .. }
    ));

    let digest_error = verify_accepted_step(4, &events, &fake_digest(), &outcome)
        .expect_err("the accepted digest changed");
    assert!(matches!(
        assert_divergence(digest_error, ReplayPhase::Step, Some(4), "state_digest"),
        ReplayDivergenceKind::StateDigest { .. }
    ));
}

#[test]
fn rejected_step_detects_changed_error_state_digest_and_card_pool() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let rejected = transcript.steps[0].clone();
    let rejected_action = prepared_action(&rejected);
    let initial = initial_state(&transcript, &catalog);

    let error = replay_steps_with(
        std::slice::from_ref(&rejected),
        std::slice::from_ref(&rejected_action),
        initial.clone(),
        |_, _| Err(ActionError::WrongPhase),
    )
    .expect_err("the typed rejection changed");
    assert!(matches!(
        assert_divergence(error, ReplayPhase::Step, Some(1), "error"),
        ReplayDivergenceKind::RejectedError { .. }
    ));

    let error = replay_steps_with(
        std::slice::from_ref(&rejected),
        std::slice::from_ref(&rejected_action),
        initial.clone(),
        |state, _| {
            state.coin = Some(Coin);
            Err(ActionError::NotYourDecision)
        },
    )
    .expect_err("the rejected action mutated state");
    assert!(matches!(
        assert_divergence(error, ReplayPhase::Step, Some(1), "state"),
        ReplayDivergenceKind::RejectedState { .. }
    ));

    let error = replay_steps_with(
        std::slice::from_ref(&rejected),
        std::slice::from_ref(&rejected_action),
        initial.clone(),
        |state, _| {
            state.cards = Arc::new(CardSet::new(vec![]));
            Err(ActionError::NotYourDecision)
        },
    )
    .expect_err("the rejected action replaced the card pool");
    assert!(matches!(
        assert_divergence(error, ReplayPhase::Step, Some(1), "state.cards"),
        ReplayDivergenceKind::CardSetIdentity
    ));

    let mut changed_digest = rejected;
    let TranscriptStepResultV1::Rejected { rejection } = &mut changed_digest.result else {
        panic!("step one is rejected");
    };
    rejection.state_digest = fake_digest();
    let error = replay_steps_with(&[changed_digest], &[rejected_action], initial, |_, _| {
        Err(ActionError::NotYourDecision)
    })
    .expect_err("the rejected digest changed");
    assert!(matches!(
        assert_divergence(error, ReplayPhase::Step, Some(1), "state_digest"),
        ReplayDivergenceKind::StateDigest { .. }
    ));
}

#[test]
fn completion_verifies_state_terminal_outcome_counts_and_digests() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let (state, facts) = replayed(&transcript, &catalog);
    verify_completion(&transcript, &state, &facts).expect("completion matches");

    let mut changed = transcript.clone();
    changed.final_state.final_state.coin = true;
    let error =
        verify_completion(&changed, &state, &facts).expect_err("the full final state changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "final_state.final_state"
        ),
        ReplayDivergenceKind::FinalState { .. }
    ));

    let no_end = ReplayFacts {
        game_ended: vec![],
        ..ReplayFacts {
            step_count: facts.step_count,
            event_count: facts.event_count,
            game_ended: Vec::new(),
        }
    };
    let error = verify_completion(&transcript, &state, &no_end).expect_err("GameEnded is required");
    assert!(matches!(
        assert_divergence(error, ReplayPhase::Completion, None, "events.game_ended"),
        ReplayDivergenceKind::GameEndedCount { actual: 0, .. }
    ));

    let mut changed_facts = ReplayFacts {
        step_count: facts.step_count,
        event_count: facts.event_count,
        game_ended: facts.game_ended.clone(),
    };
    changed_facts.game_ended[0].winner = PlayerIdV1::Two;
    let error = verify_completion(&transcript, &state, &changed_facts)
        .expect_err("the terminal event outcome changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "final_state.final_state.status.outcome"
        ),
        ReplayDivergenceKind::GameEndedOutcome { .. }
    ));

    let mut changed = transcript.clone();
    changed.match_completed.winner = PlayerIdV1::Two;
    let error = verify_completion(&changed, &state, &facts).expect_err("the winner changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "match_completed.winner"
        ),
        ReplayDivergenceKind::Winner { .. }
    ));

    let mut changed = transcript.clone();
    changed.match_completed.reason = LossReasonV1::ThirdMainLoss;
    let error = verify_completion(&changed, &state, &facts).expect_err("the loss reason changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "match_completed.reason"
        ),
        ReplayDivergenceKind::LossReason { .. }
    ));

    let mut changed = transcript.clone();
    changed.match_completed.step_count += 1;
    let error = verify_completion(&changed, &state, &facts).expect_err("the step count changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "match_completed.step_count"
        ),
        ReplayDivergenceKind::StepCount { .. }
    ));

    let mut changed = transcript.clone();
    changed.match_completed.event_count += 1;
    let error = verify_completion(&changed, &state, &facts).expect_err("the event count changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "match_completed.event_count"
        ),
        ReplayDivergenceKind::EventCount { .. }
    ));

    let mut changed = transcript.clone();
    changed.final_state.state_digest = fake_digest();
    let error = verify_completion(&changed, &state, &facts).expect_err("the final digest changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "final_state.state_digest"
        ),
        ReplayDivergenceKind::StateDigest { .. }
    ));

    let mut changed = transcript;
    changed.match_completed.state_digest = fake_digest();
    let error =
        verify_completion(&changed, &state, &facts).expect_err("the completion digest changed");
    assert!(matches!(
        assert_divergence(
            error,
            ReplayPhase::Completion,
            None,
            "match_completed.state_digest"
        ),
        ReplayDivergenceKind::StateDigest { .. }
    ));
}

#[test]
fn changed_event_and_digest_report_the_first_bad_step() {
    let (catalog, bytes) = fixture();
    let changed_event = rewrite_first(&bytes, |record| {
        if record["record"] == "event" && record["event"]["kind"] == "priority_passed" {
            record["event"]["player"] = Value::String("one".to_string());
            true
        } else {
            false
        }
    });
    let error = verify_transcript(Cursor::new(changed_event), catalog.library())
        .expect_err("the expected event changed");
    let ReplayError::Divergence(divergence) = error else {
        panic!("expected event divergence");
    };
    assert_eq!(divergence.location.step, Some(3));
    assert_eq!(divergence.location.event_index, Some(0));
    assert!(matches!(
        divergence.kind,
        ReplayDivergenceKind::Event { .. }
    ));

    let changed_digest = rewrite_first(&bytes, |record| {
        if record["record"] == "step_completed" {
            record["state_digest"] = Value::String(fake_digest().0);
            true
        } else {
            false
        }
    });
    let error = verify_transcript(Cursor::new(changed_digest), catalog.library())
        .expect_err("the expected digest changed");
    let ReplayError::Divergence(divergence) = error else {
        panic!("expected digest divergence");
    };
    assert_eq!(divergence.location.step, Some(2));
    assert!(matches!(
        divergence.kind,
        ReplayDivergenceKind::StateDigest { .. }
    ));
}

#[test]
fn parser_reports_initial_digest_path_before_replay() {
    let (catalog, bytes) = fixture();
    let changed = rewrite_first(&bytes, |record| {
        if record["record"] == "match_created" {
            record["state_digest"] = Value::String(fake_digest().0);
            true
        } else {
            false
        }
    });
    let error = verify_transcript(Cursor::new(changed), catalog.library())
        .expect_err("the initial digest changed");
    let ReplayError::Parse(error) = error else {
        panic!("the parser owns internal transcript consistency");
    };
    assert_eq!(error.context().path.as_deref(), Some("state_digest"));
}

#[test]
fn more_recorded_steps_than_prepared_actions_is_rejected_before_replay() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let recorded_steps = &transcript.steps;
    let prepared_actions: Vec<PreparedAction> =
        transcript.steps[..2].iter().map(prepared_action).collect();
    let invoked = Cell::new(false);

    let error = replay_steps_with(
        recorded_steps,
        &prepared_actions,
        initial_state(&transcript, &catalog),
        |_, _| {
            invoked.set(true);
            Err(ActionError::WrongPhase)
        },
    )
    .expect_err("more recorded steps than prepared actions must be rejected");

    assert!(!invoked.get(), "the engine must not run before the guard");
    assert!(matches!(
        error,
        ReplayError::StepActionMismatch { steps, actions }
            if steps == recorded_steps.len() && actions == 2
    ));
}

#[test]
fn more_prepared_actions_than_recorded_steps_is_rejected_before_replay() {
    let (catalog, bytes) = fixture();
    let transcript = parsed(&bytes);
    let recorded_steps = &transcript.steps[..2];
    let prepared_actions: Vec<PreparedAction> =
        transcript.steps.iter().map(prepared_action).collect();
    let invoked = Cell::new(false);

    let error = replay_steps_with(
        recorded_steps,
        &prepared_actions,
        initial_state(&transcript, &catalog),
        |_, _| {
            invoked.set(true);
            Err(ActionError::WrongPhase)
        },
    )
    .expect_err("more prepared actions than recorded steps must be rejected");

    assert!(!invoked.get(), "the engine must not run before the guard");
    assert!(matches!(
        error,
        ReplayError::StepActionMismatch { steps, actions }
            if steps == 2 && actions == prepared_actions.len()
    ));
}

#[test]
fn step_action_mismatch_reports_counts_and_has_no_location() {
    let error = ReplayError::StepActionMismatch {
        steps: 4,
        actions: 2,
    };

    assert_eq!(
        error.to_string(),
        "replay received 4 recorded steps but 2 prepared actions"
    );
    assert!(error.location().is_none());
}

#[test]
fn display_uses_semantic_names_without_debug_contracts() {
    let divergence = ReplayDivergence {
        location: ReplayLocation::completion("match_completed.reason"),
        kind: ReplayDivergenceKind::LossReason {
            expected: LossReasonV1::ThirdMainLoss,
            actual: LossReasonV1::EmptyDeckDraw,
        },
    };
    assert_eq!(
        divergence.to_string(),
        "completion, path match_completed.reason: expected loss reason third_main_loss, found empty_deck_draw"
    );
    assert_eq!(player_name(PlayerIdV1::One), "one");
    assert_eq!(
        reason_name(LossReasonV1::NoPromotionAvailable),
        "no_promotion_available"
    );
}
