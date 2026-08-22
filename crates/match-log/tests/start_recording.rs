#![allow(clippy::expect_used)]

use std::{
    collections::{BTreeMap, VecDeque},
    io::{self, Cursor, Write},
    sync::Arc,
};

use summoners_core::domain::{
    cards::CardSet,
    ids::PlayerId,
    state::{GameState, GameStatus, ManaBank, PerPlayer, Phase, PlayerState, TurnState},
};
use summoners_match_log::{
    MatchCreatedV1, RecordedMatch, RecordingError, SetRequirementV1, StateDigestV1,
    StateProjectionV1,
};

const STATE_JSON: &str = "{\"players\":{\"one\":{\"main\":null,\"bench\":[null,null,null],\"deck\":[],\"hand\":[],\"prizes\":[],\"discard\":[],\"mana\":{\"matter\":0,\"mind\":0,\"spirit\":0},\"main_losses\":0,\"enchantments\":[]},\"two\":{\"main\":null,\"bench\":[null,null,null],\"deck\":[],\"hand\":[],\"prizes\":[],\"discard\":[],\"mana\":{\"matter\":0,\"mind\":0,\"spirit\":0},\"main_losses\":0,\"enchantments\":[]}},\"coin\":false,\"turn\":{\"active_player\":\"one\",\"phase\":\"main\",\"window\":null,\"normal_attack_used\":false,\"normal_retreat_used\":false,\"spell_played_this_turn\":{\"one\":false,\"two\":false}},\"stack\":[],\"stack_segment_bases\":[],\"work\":[],\"pending\":null,\"status\":{\"kind\":\"playing\"}}";
const DIGEST: &str = "sha256:814c2a07f135c91346400e5196fb5b820e2b09f60180a925c3865ef25c906520";

fn player() -> PlayerState {
    PlayerState {
        main: None,
        bench: [None, None, None],
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        enchantments: vec![],
    }
}

fn initial_state() -> GameState {
    GameState {
        players: PerPlayer::new(player(), player()),
        coin: None,
        turn: TurnState {
            active_player: PlayerId::One,
            phase: Phase::Main,
            window: None,
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: PerPlayer::new(false, false),
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: VecDeque::new(),
        pending: None,
        status: GameStatus::Playing,
        cards: Arc::new(CardSet::new(vec![])),
    }
}

fn requirements() -> Vec<SetRequirementV1> {
    vec![SetRequirementV1 {
        set: "foundations".to_string(),
        revision: 1,
    }]
}

#[test]
fn start_writes_exact_records_rebuilds_state_and_exposes_unchanged_state() {
    let state = initial_state();
    let cards = Arc::clone(&state.cards);
    let metadata = BTreeMap::from([("shell".to_string(), serde_json::json!("test"))]);

    let recording = RecordedMatch::start(
        Cursor::new(Vec::new()),
        metadata,
        requirements(),
        state.clone(),
    )
    .expect("the initial checkpoint is written");

    assert_eq!(recording.state(), &state);
    let bytes = recording.into_writer().into_inner();
    let expected = format!(
        "{{\"sequence\":0,\"record\":\"header\",\"format\":\"summoners_match\",\"format_version\":1,\"metadata\":{{\"shell\":\"test\"}}}}\n{{\"sequence\":1,\"record\":\"match_created\",\"required_sets\":[{{\"set\":\"foundations\",\"revision\":1}}],\"initial_state\":{STATE_JSON},\"state_digest\":\"{DIGEST}\"}}\n"
    );
    assert_eq!(bytes, expected.as_bytes());

    let created_line = bytes
        .split(|byte| *byte == b'\n')
        .nth(1)
        .expect("the match-created line exists");
    let created: MatchCreatedV1 =
        serde_json::from_slice(created_line).expect("the match-created line is valid");
    let rebuilt = created
        .initial_state
        .clone()
        .into_game_state(Arc::clone(&cards))
        .expect("the initial state rebuilds");
    assert_eq!(rebuilt, state);
    assert_eq!(
        StateDigestV1::compute(&StateProjectionV1::from_state(&rebuilt))
            .expect("the rebuilt state hashes"),
        created.state_digest
    );
}

#[test]
fn start_returns_a_typed_error_and_no_handle_after_write_failure() {
    let result = RecordedMatch::start(WriteFails, BTreeMap::new(), requirements(), initial_state());
    assert!(matches!(result, Err(RecordingError::Write(_))));
}

#[test]
fn start_returns_a_typed_error_and_no_handle_after_flush_failure() {
    let result = RecordedMatch::start(FlushFails, BTreeMap::new(), requirements(), initial_state());
    assert!(matches!(result, Err(RecordingError::Flush(_))));
}

struct WriteFails;

impl Write for WriteFails {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct FlushFails;

impl Write for FlushFails {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("injected flush failure"))
    }
}
