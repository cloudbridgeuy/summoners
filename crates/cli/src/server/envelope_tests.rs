#![allow(clippy::expect_used)]
use super::*;
use summoners_core::domain::actions::GameAction;
use summoners_core::domain::errors::ActionError;
use summoners_core::domain::ids::PlayerId;
use tempfile::TempDir;

fn opening_recorder(
    seed: u64,
) -> (
    RecordedMatch<File>,
    crate::protocol::CardDescriptions,
    crate::protocol::CardIdentityMap,
    TempDir,
) {
    let catalog = built_in_catalog().expect("catalog");
    let state = initial_state(
        catalog.library(),
        [catalog.set_paths(), catalog.set_paths()],
        seed,
    )
    .expect("state");
    let descriptions = crate::protocol::CardDescriptions::from_card_set(&state.cards);
    let identities = crate::protocol::CardIdentityMap::from_initial_state(&state, &descriptions);
    let directory = TempDir::new().expect("directory");
    let transcript = directory.path().join("match.ndjson");
    let recorder = start_recording(
        File::create(&transcript).expect("transcript"),
        BTreeMap::new(),
        required_sets(catalog.library()).expect("requirements"),
        state,
    )
    .expect("recorder");
    (recorder, descriptions, identities, directory)
}

#[tokio::test]
async fn requester_only_rejection_result_leaves_state_unchanged_for_both_seats() {
    let (mut recorder, descriptions, identities, _directory) = opening_recorder(17);
    let opening_one_view =
        crate::protocol::player_view(recorder.state(), PlayerId::One, &descriptions);
    let opening_two_view =
        crate::protocol::player_view(recorder.state(), PlayerId::Two, &descriptions);

    let error = match recorder
        .submit(&GameAction::EndTurn {
            player: PlayerId::Two,
        })
        .expect("submission")
    {
        RecordedStep::Rejected { error } => error,
        RecordedStep::Accepted { .. } => panic!("expected rejection"),
    };
    assert!(matches!(error, ActionError::NotYourDecision));
    let reason = rejection_reason(error);
    assert_eq!(reason, "not your decision");

    let (one, mut one_messages) = mpsc::channel::<Outbound>(1);
    let (two, mut two_messages) = mpsc::channel::<Outbound>(1);
    let clients = [Some(one), Some(two)];

    let observe = async {
        let first = one_messages.recv().await.expect("seat one update");
        first.completed.send(Ok(())).expect("seat one acknowledges");
        let second = two_messages.recv().await.expect("seat two update");
        second
            .completed
            .send(Ok(()))
            .expect("seat two acknowledges");
        (first.envelope, second.envelope)
    };

    let (delivery, (one_envelope, two_envelope)) = tokio::join!(
        broadcast(
            &clients,
            Broadcast {
                recorder: &recorder,
                descriptions: &descriptions,
                identities: &identities,
                events: &[],
                revision: 1,
                reply: Some(1),
                requester: Some(Seat::Two),
                result: Some(SubmissionResult::Rejected {
                    reason: reason.clone(),
                }),
            }
        ),
        observe
    );
    delivery.expect("broadcast succeeds");

    let expected_one_view =
        crate::protocol::player_view(recorder.state(), PlayerId::One, &descriptions);
    let expected_two_view =
        crate::protocol::player_view(recorder.state(), PlayerId::Two, &descriptions);
    assert_eq!(expected_one_view, opening_one_view);
    assert_eq!(expected_two_view, opening_two_view);

    match &one_envelope {
        ServerEnvelope::Update {
            revision,
            view,
            reply,
            result,
            ..
        } => {
            assert_eq!(*revision, 1);
            assert_eq!(*reply, None);
            assert_eq!(result, &None);
            assert_eq!(view, &expected_one_view);
        }
        _ => panic!("expected update"),
    }
    match &two_envelope {
        ServerEnvelope::Update {
            revision,
            view,
            reply,
            result,
            ..
        } => {
            assert_eq!(*revision, 1);
            assert_eq!(*reply, Some(1));
            assert_eq!(
                result,
                &Some(SubmissionResult::Rejected {
                    reason: reason.clone()
                })
            );
            assert_eq!(view, &expected_two_view);
        }
        _ => panic!("expected update"),
    }

    let one_json = serde_json::to_string(&one_envelope).expect("seat one json");
    let two_json = serde_json::to_string(&two_envelope).expect("seat two json");
    assert!(!one_json.contains("not your decision"));
    assert!(two_json.contains("not your decision"));
}

#[tokio::test]
async fn requester_only_terminal_reply_shares_the_same_outcome() {
    let (mut recorder, descriptions, identities, _directory) = opening_recorder(17);
    recorder
        .submit(&GameAction::Resign {
            player: PlayerId::One,
        })
        .expect("resignation");

    let (one, mut one_messages) = mpsc::channel::<Outbound>(1);
    let (two, mut two_messages) = mpsc::channel::<Outbound>(1);
    let clients = [Some(one), Some(two)];

    let observe = async {
        let first = one_messages.recv().await.expect("seat one finished");
        first.completed.send(Ok(())).expect("seat one acknowledges");
        let second = two_messages.recv().await.expect("seat two finished");
        second
            .completed
            .send(Ok(()))
            .expect("seat two acknowledges");
        (first.envelope, second.envelope)
    };

    let (delivery, (one_envelope, two_envelope)) = tokio::join!(
        broadcast(
            &clients,
            Broadcast {
                recorder: &recorder,
                descriptions: &descriptions,
                identities: &identities,
                events: &[],
                revision: 1,
                reply: Some(2),
                requester: Some(Seat::One),
                result: Some(SubmissionResult::Accepted),
            }
        ),
        observe
    );
    delivery.expect("broadcast succeeds");

    let (one_outcome, one_reply) = match &one_envelope {
        ServerEnvelope::Finished { outcome, reply, .. } => (outcome.clone(), *reply),
        _ => panic!("expected finished"),
    };
    let (two_outcome, two_reply) = match &two_envelope {
        ServerEnvelope::Finished { outcome, reply, .. } => (outcome.clone(), *reply),
        _ => panic!("expected finished"),
    };
    assert_eq!(one_reply, Some(2));
    assert_eq!(two_reply, None);
    assert_eq!(one_outcome, two_outcome);

    let one_json = serde_json::to_string(&one_envelope).expect("seat one json");
    let two_json = serde_json::to_string(&two_envelope).expect("seat two json");
    assert!(one_json.contains("\"reply\":2"));
    assert!(!two_json.contains("\"reply\":2"));
    assert!(!one_json.contains("\"result\""));
    assert!(!two_json.contains("\"result\""));
    assert!(!one_json.contains("\"revision\""));
    assert!(!two_json.contains("\"revision\""));
}

#[test]
fn rejection_reason_covers_the_named_variants() {
    assert_eq!(
        rejection_reason(ActionError::NotYourDecision),
        "not your decision"
    );
    assert_eq!(rejection_reason(ActionError::WrongPhase), "wrong phase");
    assert_eq!(
        rejection_reason(ActionError::InvalidManaHint),
        "invalid mana hint"
    );
    assert_eq!(
        rejection_reason(ActionError::InsufficientMana {
            short: summoners_core::domain::state::ManaBank {
                matter: 1,
                mind: 2,
                spirit: 3,
            }
        }),
        "insufficient mana: matter 1 mind 2 spirit 3"
    );
}

#[tokio::test]
async fn accepted_handover_notices_name_the_drawn_card_only_for_the_drawing_seat() {
    let (mut recorder, descriptions, identities, _directory) = opening_recorder(17);
    recorder
        .submit(&GameAction::EndTurn {
            player: PlayerId::One,
        })
        .expect("end turn opens the window");
    recorder
        .submit(&GameAction::PassPriority {
            player: PlayerId::Two,
        })
        .expect("defender passes");
    let events = match recorder
        .submit(&GameAction::PassPriority {
            player: PlayerId::One,
        })
        .expect("handover")
    {
        RecordedStep::Accepted { events } => events,
        RecordedStep::Rejected { error } => panic!("expected acceptance, got {error:?}"),
    };
    let drawn = events
        .iter()
        .find_map(|event| match event {
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card,
            } => Some(*card),
            _ => None,
        })
        .expect("handover draws a card for player two");
    let two_view = crate::protocol::player_view(recorder.state(), PlayerId::Two, &descriptions);
    let name = two_view
        .hand
        .iter()
        .find(|card| card.instance == drawn.0)
        .expect("drawn card is in hand")
        .card
        .name
        .clone();

    let (one, mut one_messages) = mpsc::channel::<Outbound>(1);
    let (two, mut two_messages) = mpsc::channel::<Outbound>(1);
    let clients = [Some(one), Some(two)];

    let observe = async {
        let first = one_messages.recv().await.expect("seat one update");
        first.completed.send(Ok(())).expect("seat one acknowledges");
        let second = two_messages.recv().await.expect("seat two update");
        second
            .completed
            .send(Ok(()))
            .expect("seat two acknowledges");
        (first.envelope, second.envelope)
    };

    let (delivery, (one_envelope, two_envelope)) = tokio::join!(
        broadcast(
            &clients,
            Broadcast {
                recorder: &recorder,
                descriptions: &descriptions,
                identities: &identities,
                events: &events,
                revision: 3,
                reply: Some(3),
                requester: Some(Seat::One),
                result: Some(SubmissionResult::Accepted),
            }
        ),
        observe
    );
    delivery.expect("broadcast succeeds");

    let ServerEnvelope::Update {
        notices: one_notices,
        ..
    } = &one_envelope
    else {
        panic!("expected update")
    };
    let ServerEnvelope::Update {
        notices: two_notices,
        ..
    } = &two_envelope
    else {
        panic!("expected update")
    };
    assert!(two_notices.iter().any(|notice| notice.text.contains(&name)));
    assert!(
        one_notices
            .iter()
            .any(|notice| notice.text.contains("Player Two drew a card"))
    );
    assert!(!one_notices.iter().any(|notice| notice.text.contains(&name)));
}
