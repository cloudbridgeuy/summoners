use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use summoners_core::domain::{actions::GameAction, ids::PlayerId, state::GameState};
use summoners_match_log::{ActionV1, StateProjectionV1};

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    One,
    Two,
}

impl From<Seat> for PlayerId {
    fn from(value: Seat) -> Self {
        match value {
            Seat::One => Self::One,
            Seat::Two => Self::Two,
        }
    }
}
impl From<PlayerId> for Seat {
    fn from(value: PlayerId) -> Self {
        match value {
            PlayerId::One => Self::One,
            PlayerId::Two => Self::Two,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientEnvelope {
    Join {
        version: u32,
        seat: Seat,
    },
    Submit {
        request_id: u64,
        based_on_revision: u64,
        action: ActionV1,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerEnvelope {
    Waiting {
        seat: Seat,
    },
    Update {
        revision: u64,
        view: PlayerView,
        notices: Vec<String>,
        reply: Option<u64>,
    },
    Finished {
        outcome: Value,
        view: PlayerView,
        notices: Vec<String>,
        reply: Option<u64>,
    },
    Stopped {
        reason: String,
    },
    Rejected {
        request_id: Option<u64>,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerView {
    pub you: Seat,
    pub hand: Value,
    pub boards: Value,
    pub mana: Value,
    pub main_losses: Value,
    pub discards: Value,
    pub persistent: Value,
    pub coin: bool,
    pub stack: Value,
    pub opponent_hand_count: usize,
    pub deck_counts: Value,
    pub prize_counts: Value,
    pub phase: Value,
    pub active_player: Seat,
    pub priority_holder: Option<Seat>,
    pub pending: Value,
    pub outcome: Option<Value>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    Actor,
    Conversion(String),
}

pub fn normalize_action(seat: PlayerId, action: ActionV1) -> Result<GameAction, ProtocolError> {
    let action = GameAction::try_from(action)
        .map_err(|error| ProtocolError::Conversion(error.to_string()))?;
    if action.actor() == seat {
        Ok(action)
    } else {
        Err(ProtocolError::Actor)
    }
}

pub fn player_view(state: &GameState, viewer: PlayerId) -> PlayerView {
    let projection = StateProjectionV1::from_state(state);
    let data = serde_json::to_value(projection).unwrap_or(Value::Null);
    let viewer_key = match viewer {
        PlayerId::One => "one",
        PlayerId::Two => "two",
    };
    let other_key = match viewer {
        PlayerId::One => "two",
        PlayerId::Two => "one",
    };
    let players = &data["players"];
    let own = &players[viewer_key];
    let other = &players[other_key];
    let board = |player: &Value| json!({"main": player["main"], "bench": player["bench"]});
    let holder = data["turn"]["window"]["holder"].as_str().map(seat_text);
    PlayerView {
        you: viewer.into(),
        hand: own["hand"].clone(),
        boards: json!({"one": board(&players["one"]), "two": board(&players["two"])}),
        mana: json!({"one": players["one"]["mana"], "two": players["two"]["mana"]}),
        main_losses: json!({"one": players["one"]["main_losses"], "two": players["two"]["main_losses"]}),
        discards: json!({"one": players["one"]["discard"], "two": players["two"]["discard"]}),
        persistent: json!({"one": players["one"]["enchantments"], "two": players["two"]["enchantments"]}),
        coin: data["coin"].as_bool().unwrap_or(false),
        stack: data["stack"].clone(),
        opponent_hand_count: other["hand"].as_array().map_or(0, Vec::len),
        deck_counts: json!({"one": players["one"]["deck"].as_array().map_or(0, Vec::len), "two": players["two"]["deck"].as_array().map_or(0, Vec::len)}),
        prize_counts: json!({"one": players["one"]["prizes"].as_array().map_or(0, Vec::len), "two": players["two"]["prizes"].as_array().map_or(0, Vec::len)}),
        phase: data["turn"]["phase"].clone(),
        active_player: seat_text(data["turn"]["active_player"].as_str().unwrap_or("one")),
        priority_holder: holder,
        pending: pending_kind(&data["pending"]),
        outcome: data["status"]["outcome"]
            .as_object()
            .map(|_| data["status"]["outcome"].clone()),
    }
}

fn seat_text(value: &str) -> Seat {
    if value == "two" { Seat::Two } else { Seat::One }
}
fn pending_kind(value: &Value) -> Value {
    value
        .as_object()
        .map_or(Value::Null, |pending| json!({"kind": pending.get("kind")}))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::setup::initial_state;
    use summoners_cards::built_in_catalog;
    #[test]
    fn view_hides_other_private_zones() {
        let catalog = built_in_catalog().expect("catalog");
        let state = initial_state(
            catalog.library(),
            [catalog.set_paths(), catalog.barrow_herd()],
            4,
        )
        .expect("state");
        let encoded = serde_json::to_string(&player_view(&state, PlayerId::One)).expect("view");
        assert!(!encoded.contains("prizes\":[]"));
        assert!(!encoded.contains("stack_segment_bases"));
        assert!(!encoded.contains("work"));
    }
    #[test]
    fn spoofed_actor_is_rejected() {
        let action = ActionV1::Resign {
            player: summoners_match_log::wire::PlayerIdV1::Two,
        };
        assert_eq!(
            normalize_action(PlayerId::One, action),
            Err(ProtocolError::Actor)
        );
    }
}
