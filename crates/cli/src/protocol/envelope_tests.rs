#![allow(clippy::expect_used)]
use super::*;
use crate::setup::initial_state;
use summoners_cards::built_in_catalog;

fn state_and_descriptions() -> (GameState, CardDescriptions) {
    let catalog = built_in_catalog().expect("catalog");
    let state = initial_state(
        catalog.library(),
        [catalog.set_paths(), catalog.barrow_herd()],
        4,
    )
    .expect("state");
    let descriptions = CardDescriptions::from_card_set(&state.cards);
    (state, descriptions)
}

fn opponent(player: PlayerId) -> PlayerId {
    match player {
        PlayerId::One => PlayerId::Two,
        PlayerId::Two => PlayerId::One,
    }
}

#[test]
fn player_notices_preserve_event_order_for_both_viewers() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let draw_card = state.players.one.deck[0].instance;
    let hand_card = state.players.one.hand[0].instance;
    let events = vec![
        GameEvent::TurnBegan {
            player: PlayerId::One,
        },
        GameEvent::CardDrawn {
            player: PlayerId::One,
            card: draw_card,
        },
        GameEvent::SummonPlayed {
            player: PlayerId::One,
            card: hand_card,
            slot: BenchSlot::First,
        },
        GameEvent::PriorityPassed {
            player: PlayerId::One,
        },
    ];

    for viewer in [PlayerId::One, PlayerId::Two] {
        let notices = player_notices(&events, &identities, viewer);
        let texts: Vec<&str> = notices.iter().map(|notice| notice.text.as_str()).collect();
        assert_eq!(texts.len(), 4);
        assert!(texts[0].contains("began a turn"));
        assert!(texts[1].contains("drew"));
        assert!(texts[2].contains("played"));
        assert!(texts[3].contains("passed Priority"));

        let encoded = serde_json::to_string(&notices).expect("notices");
        let began = encoded.find("began a turn").expect("began index");
        let drew = encoded.find("drew").expect("drew index");
        let played = encoded.find("played").expect("played index");
        let passed = encoded.find("passed Priority").expect("passed index");
        assert!(began < drew);
        assert!(drew < played);
        assert!(played < passed);
    }
}

#[test]
fn card_drawn_notice_names_the_card_only_for_the_drawing_player() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    for (player, deck) in [
        (PlayerId::One, &state.players.one.deck),
        (PlayerId::Two, &state.players.two.deck),
    ] {
        let card = deck[0].instance;
        let name = identities.name(card);
        let event = GameEvent::CardDrawn { player, card };
        let owner_notice = player_notices(std::slice::from_ref(&event), &identities, player);
        let opponent_notice = player_notices(&[event], &identities, opponent(player));

        assert_eq!(
            owner_notice[0].text,
            format!("{} drew {}", player_text(player), name)
        );
        assert_eq!(
            opponent_notice[0].text,
            format!("{} drew a card", player_text(player))
        );
        assert!(!opponent_notice[0].text.contains(&name));

        let owner_json = serde_json::to_string(&owner_notice).expect("owner json");
        let opponent_json = serde_json::to_string(&opponent_notice).expect("opponent json");
        assert!(owner_json.contains(&name));
        assert!(!opponent_json.contains(&name));
    }
}

#[test]
fn prize_recovered_notice_names_the_card_only_for_the_recovering_player() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    for (player, prizes) in [
        (PlayerId::One, &state.players.one.prizes),
        (PlayerId::Two, &state.players.two.prizes),
    ] {
        let card = prizes[0].instance;
        let name = identities.name(card);
        let event = GameEvent::PrizeRecovered { player, card };
        let owner_notice = player_notices(std::slice::from_ref(&event), &identities, player);
        let opponent_notice = player_notices(&[event], &identities, opponent(player));

        assert!(owner_notice[0].text.contains(&name));
        assert!(!opponent_notice[0].text.contains(&name));
        assert!(opponent_notice[0].text.ends_with("a card"));

        let owner_json = serde_json::to_string(&owner_notice).expect("owner json");
        let opponent_json = serde_json::to_string(&opponent_notice).expect("opponent json");
        assert!(owner_json.contains(&name));
        assert!(!opponent_json.contains(&name));
    }
}

#[test]
fn prizes_viewed_notice_names_cards_only_for_the_viewing_player() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let prizes: Vec<CardInstanceId> = state
        .players
        .one
        .prizes
        .iter()
        .map(|card| card.instance)
        .collect();
    let names: Vec<String> = prizes.iter().map(|card| identities.name(*card)).collect();
    let event = GameEvent::PrizesViewed {
        player: PlayerId::One,
        prizes: prizes.clone(),
    };

    let owner_notice = player_notices(std::slice::from_ref(&event), &identities, PlayerId::One);
    let opponent_notice = player_notices(&[event], &identities, PlayerId::Two);

    for name in &names {
        assert!(owner_notice[0].text.contains(name));
    }
    assert_eq!(
        opponent_notice[0].text,
        format!("Player One viewed {} Prizes", prizes.len())
    );
    for name in &names {
        assert!(!opponent_notice[0].text.contains(name));
    }

    let owner_json = serde_json::to_string(&owner_notice).expect("owner json");
    let opponent_json = serde_json::to_string(&opponent_notice).expect("opponent json");
    for name in &names {
        assert!(owner_json.contains(name));
        assert!(!opponent_json.contains(name));
    }
}

#[test]
fn update_envelope_serializes_reply_and_result_option_fields() {
    let (state, descriptions) = state_and_descriptions();
    let view = player_view(&state, PlayerId::One, &descriptions);
    let rejected = ServerEnvelope::Update {
        revision: 3,
        view: view.clone(),
        notices: Vec::new(),
        reply: Some(3),
        result: Some(SubmissionResult::Rejected {
            reason: "not your decision".to_string(),
        }),
    };
    let encoded = serde_json::to_string(&rejected).expect("rejected update");
    assert!(encoded.contains("not your decision"));
    assert!(encoded.contains("\"reply\":3"));

    let empty = ServerEnvelope::Update {
        revision: 3,
        view,
        notices: Vec::new(),
        reply: None,
        result: None,
    };
    let encoded_empty = serde_json::to_string(&empty).expect("empty update");
    assert!(!encoded_empty.contains("not your decision"));
    assert!(encoded_empty.contains("\"reply\":null"));
    assert!(encoded_empty.contains("\"result\":null"));
}
