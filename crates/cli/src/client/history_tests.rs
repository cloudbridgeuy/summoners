#![allow(clippy::expect_used)]

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use super::*;
use crate::protocol::{CardDescriptions, CardIdentityMap, OutcomeReasonView, player_notices};
use crate::setup::initial_state;
use summoners_cards::built_in_catalog;
use summoners_core::domain::cards::Name;
use summoners_core::domain::events::GameEvent;
use summoners_core::domain::ids::PlayerId;
use summoners_core::domain::state::GameState;

const SEED: u64 = 7;

fn opening_state() -> GameState {
    let catalog = built_in_catalog().expect("catalog");
    initial_state(
        catalog.library(),
        [catalog.set_paths(), catalog.barrow_herd()],
        SEED,
    )
    .expect("state")
}

fn card_name(state: &GameState, def: summoners_core::domain::cards::EntityId) -> String {
    state
        .cards
        .get(def)
        .and_then(|entity| entity.get::<Name>())
        .map(|name| name.0.clone())
        .unwrap_or_default()
}

fn write_frame(stream: &mut TcpStream, envelope: &ServerEnvelope) {
    let mut bytes = serde_json::to_vec(envelope).expect("encode");
    bytes.push(b'\n');
    stream.write_all(&bytes).expect("write");
}

fn run_receive(
    view: PlayerView,
    outcome: OutcomeView,
    update_notices: Vec<crate::protocol::Notice>,
    finished_notices: Vec<crate::protocol::Notice>,
) -> (Vec<String>, bool) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let address = listener.local_addr().expect("address");
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        write_frame(
            &mut socket,
            &ServerEnvelope::Update {
                revision: 1,
                view: view.clone(),
                notices: update_notices,
                reply: None,
                result: None,
            },
        );
        write_frame(
            &mut socket,
            &ServerEnvelope::Finished {
                outcome,
                view,
                notices: finished_notices,
                reply: None,
            },
        );
    });
    let stream = TcpStream::connect(address).expect("connect");
    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let (terminal_sender, terminal_receiver) = std::sync::mpsc::channel();
    receive(stream, &snapshot, &terminal_sender);
    server.join().expect("server joins");
    let history = snapshot.lock().expect("lock").history.clone();
    let signaled = terminal_receiver.try_recv().is_ok();
    (history, signaled)
}

#[test]
fn receive_extends_history_with_every_delivered_notice_in_delivery_order() {
    let state = opening_state();
    let descriptions = CardDescriptions::from_card_set(&state.cards);
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);

    let prize_instances: Vec<_> = state
        .players
        .two
        .prizes
        .iter()
        .map(|card| card.instance)
        .collect();
    let prize_names: Vec<String> = state
        .players
        .two
        .prizes
        .iter()
        .map(|card| card_name(&state, card.def))
        .collect();
    assert_eq!(prize_names.len(), 2);

    let drawn = state.players.one.hand[0];
    let drawn_name = card_name(&state, drawn.def);

    let update_events = vec![GameEvent::PrizesViewed {
        player: PlayerId::Two,
        prizes: prize_instances,
    }];
    let finished_events = vec![GameEvent::CardDrawn {
        player: PlayerId::One,
        card: drawn.instance,
    }];

    let view_one = crate::protocol::player_view(&state, PlayerId::One, &descriptions);
    let view_two = crate::protocol::player_view(&state, PlayerId::Two, &descriptions);
    let outcome = OutcomeView {
        winner: Seat::One,
        reason: OutcomeReasonView::Resignation,
    };

    let (history_two, signaled_two) = run_receive(
        view_two,
        outcome.clone(),
        player_notices(&update_events, &identities, PlayerId::Two),
        player_notices(&finished_events, &identities, PlayerId::Two),
    );
    let (history_one, signaled_one) = run_receive(
        view_one,
        outcome,
        player_notices(&update_events, &identities, PlayerId::One),
        player_notices(&finished_events, &identities, PlayerId::One),
    );

    assert!(signaled_two, "receive signals the terminal channel");
    assert!(signaled_one, "receive signals the terminal channel");

    assert_eq!(
        history_two,
        vec![
            format!("Player Two viewed Prizes: {}", prize_names.join(", ")),
            "Player One drew a card".to_string(),
        ]
    );
    for name in &prize_names {
        assert!(history_two[0].contains(name.as_str()));
    }

    assert_eq!(
        history_one,
        vec![
            "Player Two viewed 2 Prizes".to_string(),
            format!("Player One drew {drawn_name}"),
        ]
    );
    for name in &prize_names {
        assert!(!history_one[0].contains(name.as_str()));
    }
}

fn snapshot_with(view: PlayerView, history: Vec<String>) -> Snapshot {
    Snapshot {
        revision: 0,
        view: Some(view),
        history,
    }
}

#[test]
fn local_command_matches_inspection_and_invalid_and_history_lines() {
    let state = opening_state();
    let descriptions = CardDescriptions::from_card_set(&state.cards);
    let view = crate::protocol::player_view(&state, PlayerId::One, &descriptions);

    let empty = snapshot_with(view.clone(), Vec::new());
    assert!(local_command("inspect 1", &empty).is_some());
    assert_eq!(
        local_command("inspect 0", &empty),
        Some(vec!["Invalid inspection".to_string()])
    );
    assert!(local_command("inspect board 2 1", &empty).is_some());
    assert_eq!(
        local_command("inspect nonsense", &empty),
        Some(vec!["Invalid inspection".to_string()])
    );
    assert_eq!(local_command("history", &empty), Some(Vec::new()));

    let filled = snapshot_with(view, vec!["one".to_string(), "two".to_string()]);
    assert_eq!(
        local_command("history", &filled),
        Some(vec!["one".to_string(), "two".to_string()])
    );
}
