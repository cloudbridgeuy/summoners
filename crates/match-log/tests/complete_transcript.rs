use std::{collections::VecDeque, sync::Arc};

use serde_json::Value;
use summoners_core::domain::{
    actions::GameAction,
    cards::CardSet,
    errors::ActionError,
    ids::PlayerId,
    state::{
        GameOutcome, GameState, GameStatus, LossReason, ManaBank, PerPlayer, Phase, PlayerState,
        TurnState,
    },
};
use summoners_match_log::{HeaderMetadataV1, RecordedMatch, RecordedStep};

fn initial_state() -> GameState {
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
    GameState {
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
        status: GameStatus::Playing,
        cards: Arc::new(CardSet::new(vec![])),
    }
}

#[test]
fn public_recorder_produces_a_complete_transcript() {
    let mut recording =
        RecordedMatch::start(Vec::new(), HeaderMetadataV1::new(), vec![], initial_state())
            .expect("the in-memory writer is available");

    assert_eq!(
        recording
            .submit(&GameAction::EndTurn {
                player: PlayerId::One,
            })
            .expect("ending Main opens Priority"),
        RecordedStep::Accepted { events: vec![] }
    );
    assert_eq!(
        recording
            .submit(&GameAction::EndTurn {
                player: PlayerId::Two,
            })
            .expect("the rejection is a complete step"),
        RecordedStep::Rejected {
            error: ActionError::WrongPhase
        }
    );
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

    assert_eq!(
        recording.state().status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::EmptyDeckDraw,
        })
    );
    let bytes = recording.into_writer();
    let lines = std::str::from_utf8(&bytes)
        .expect("records are UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("record is JSON"))
        .collect::<Vec<_>>();
    let names = lines
        .iter()
        .map(|line| line["record"].as_str().expect("record name"))
        .collect::<Vec<_>>();

    assert_eq!(names.first(), Some(&"header"));
    assert_eq!(names.get(1), Some(&"match_created"));
    assert!(names.contains(&"step_completed"));
    assert!(names.contains(&"step_rejected"));
    assert_eq!(
        names.iter().filter(|name| **name == "final_state").count(),
        1
    );
    assert_eq!(
        names
            .iter()
            .filter(|name| **name == "match_completed")
            .count(),
        1
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line["event"]["kind"] == "game_ended")
            .count(),
        1
    );
}
