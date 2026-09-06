#![allow(dead_code)]

use std::{collections::VecDeque, sync::Arc};

use summoners_core::domain::{
    actions::GameAction,
    cards::{CardSet, EntityId},
    ids::{ManaType, PlayerId, Position},
    state::{ManaBank, PerPlayer, Phase, PlayerState, TurnState},
};
use summoners_match_log::{HeaderMetadataV1, RecordedMatch};

pub fn valid_transcript_bytes() -> Vec<u8> {
    let mut recording =
        RecordedMatch::start(Vec::new(), HeaderMetadataV1::new(), vec![], initial_state())
            .expect("the fixture writer is available");

    recording
        .submit(&GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: EntityId::parse("01234567-89ab-cdef-0123-456789abcdef")
                .expect("the fixture ability ID is valid"),
            targets: vec![Position::Bench(
                summoners_core::domain::ids::BenchSlot::First,
            )],
            mana_hint: Some(ManaType::Mind),
        })
        .expect("the invalid board action is a recorded rejection");
    recording
        .submit(&GameAction::EndTurn {
            player: PlayerId::One,
        })
        .expect("ending Main opens Priority");
    recording
        .submit(&GameAction::PassPriority {
            player: PlayerId::Two,
        })
        .expect("the defender passes");
    recording
        .submit(&GameAction::PassPriority {
            player: PlayerId::One,
        })
        .expect("the empty draw ends the match");

    recording.into_writer()
}

pub fn resignation_transcript_bytes() -> Vec<u8> {
    let mut recording =
        RecordedMatch::start(Vec::new(), HeaderMetadataV1::new(), vec![], initial_state())
            .expect("the fixture writer is available");
    recording
        .submit(&GameAction::Resign {
            player: PlayerId::One,
        })
        .expect("resignation is legal while Playing");
    recording.into_writer()
}

pub fn initial_state() -> summoners_core::domain::state::GameState {
    let player = PlayerState {
        main: None,
        bench: [None, None, None],
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        enchantments: vec![],
    };
    summoners_core::domain::state::GameState {
        players: PerPlayer::new(player.clone(), player),
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
        status: summoners_core::domain::state::GameStatus::Playing,
        cards: Arc::new(CardSet::new(vec![])),
    }
}
