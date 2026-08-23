//! Pure rendering of a `GameState` for an interactive operator, and pure
//! formatting for one submitted action's outcome.
//!
//! Nothing here decides a game rule. `compact_view` reads
//! `summoners_core::engine::apply::required_actor` for who the engine will
//! accept an action from next — the one rule this view must never
//! re-derive — and otherwise only reads plain state fields.

use std::fmt;

use summoners_core::domain::cards::Breakage;
use summoners_core::domain::errors::ActionError;
use summoners_core::domain::events::GameEvent;
use summoners_core::domain::ids::PlayerId;
use summoners_core::domain::state::{
    GameState, GameStatus, LossReason, Phase, PlayerState, Readiness, SummonInstance,
};
use summoners_core::engine::apply::required_actor;
use summoners_match_log::wire::{ErrorV1, EventV1};

/// Everything an operator needs to see between prompts: whose turn it is,
/// which phase it rests in, who the engine will accept an action from, both
/// players' zones, and the match's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactView {
    pub active_player: PlayerId,
    pub phase: Phase,
    pub decides: PlayerId,
    pub one: PlayerView,
    pub two: PlayerView,
    pub status: StatusView,
}

/// One player's zones, reduced to what an operator needs to choose their
/// next command: what is on the board, what is in hand, and how much is
/// left in the deck, the Prizes, and the Mana bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerView {
    pub main: Option<SummonView>,
    pub bench: [Option<SummonView>; 3],
    pub hand: Vec<u32>,
    pub deck: usize,
    pub prizes: usize,
    pub mana: ManaView,
}

/// One Summon in play, reduced to its instance id, its top card, its
/// accumulated Damage, and whether it can currently activate a Skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonView {
    pub instance: u32,
    pub definition: summoners_core::domain::cards::EntityId,
    pub damage: u32,
    pub readiness: Readiness,
}

/// The three typed Mana pools a player has banked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManaView {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
}

/// Where the match currently stands, reduced from `GameStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusView {
    Playing,
    Ended {
        winner: PlayerId,
        reason: LossReason,
    },
    Broken(Breakage),
}

/// Render `state` as the view an interactive session prints between
/// prompts. Pure: the same state always renders the same view.
#[must_use]
pub fn compact_view(state: &GameState) -> CompactView {
    CompactView {
        active_player: state.turn.active_player,
        phase: state.turn.phase,
        decides: required_actor(state),
        one: player_view(state.players.get(PlayerId::One)),
        two: player_view(state.players.get(PlayerId::Two)),
        status: status_view(&state.status),
    }
}

/// Format one accepted action's events, one line per event, in the order
/// the engine produced them.
#[must_use]
pub fn format_accepted_events(events: &[GameEvent]) -> Vec<String> {
    events
        .iter()
        .enumerate()
        .map(|(index, event)| format_accepted_event(index, event))
        .collect()
}

/// Format one accepted event as `event <i>: <EventV1 as JSON>`.
#[must_use]
pub fn format_accepted_event(index: usize, event: &GameEvent) -> String {
    let wire = EventV1::from(event);
    let json = serde_json::to_string(&wire).unwrap_or_else(|_| "null".to_string());
    format!("event {index}: {json}")
}

/// Format one rejection as `rejected: <ErrorV1 as JSON>`.
#[must_use]
pub fn format_rejection(error: ActionError) -> String {
    let wire = ErrorV1::from(error);
    let json = serde_json::to_string(&wire).unwrap_or_else(|_| "null".to_string());
    format!("rejected: {json}")
}

fn player_view(player: &PlayerState) -> PlayerView {
    PlayerView {
        main: player.main.as_ref().map(summon_view),
        bench: [
            player.bench[0].as_ref().map(summon_view),
            player.bench[1].as_ref().map(summon_view),
            player.bench[2].as_ref().map(summon_view),
        ],
        hand: player.hand.iter().map(|card| card.instance.0).collect(),
        deck: player.deck.len(),
        prizes: player.prizes.len(),
        mana: ManaView {
            matter: player.mana.matter,
            mind: player.mana.mind,
            spirit: player.mana.spirit,
        },
    }
}

fn summon_view(summon: &SummonInstance) -> SummonView {
    let top = summon.chain.top();
    SummonView {
        instance: top.instance.0,
        definition: top.def,
        damage: summon.damage,
        readiness: summon.readiness,
    }
}

fn status_view(status: &GameStatus) -> StatusView {
    match status {
        GameStatus::Playing => StatusView::Playing,
        GameStatus::Ended(outcome) => StatusView::Ended {
            winner: outcome.winner,
            reason: outcome.reason,
        },
        GameStatus::Broken(breakage) => StatusView::Broken(*breakage),
    }
}

impl fmt::Display for CompactView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "turn: {} / phase: {} / decides: {}",
            player_label(self.active_player),
            phase_label(self.phase),
            player_label(self.decides)
        )?;
        writeln!(formatter, "one: {}", self.one)?;
        writeln!(formatter, "two: {}", self.two)?;
        write!(formatter, "status: {}", self.status)
    }
}

impl fmt::Display for PlayerView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bench = self
            .bench
            .iter()
            .map(|slot| format_summon_or_dash(slot.as_ref()))
            .collect::<Vec<_>>()
            .join(", ");
        let hand = self
            .hand
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        write!(
            formatter,
            "main: {} / bench: [{bench}] / hand: [{hand}] / deck: {} / prizes: {} / mana: {}",
            format_summon_or_dash(self.main.as_ref()),
            self.deck,
            self.prizes,
            self.mana
        )
    }
}

impl fmt::Display for SummonView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}@{} dmg:{} {}",
            self.instance,
            self.definition,
            self.damage,
            readiness_label(self.readiness)
        )
    }
}

impl fmt::Display for ManaView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}/{}", self.matter, self.mind, self.spirit)
    }
}

impl fmt::Display for StatusView {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Playing => formatter.write_str("playing"),
            Self::Ended { winner, reason } => write!(
                formatter,
                "ended(winner:{} reason:{})",
                player_label(*winner),
                snake_case(&format!("{reason:?}"))
            ),
            Self::Broken(breakage) => write!(
                formatter,
                "broken({} expected:{})",
                breakage.rule,
                snake_case(&format!("{:?}", breakage.expected))
            ),
        }
    }
}

fn format_summon_or_dash(summon: Option<&SummonView>) -> String {
    summon.map_or_else(|| "-".to_string(), SummonView::to_string)
}

fn player_label(player: PlayerId) -> &'static str {
    match player {
        PlayerId::One => "one",
        PlayerId::Two => "two",
    }
}

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Upkeep => "upkeep",
        Phase::Main => "main",
        Phase::Combat => "combat",
    }
}

fn readiness_label(readiness: Readiness) -> &'static str {
    match readiness {
        Readiness::Ready => "ready",
        Readiness::Exhausted => "exhausted",
    }
}

/// Convert one `Debug`-rendered `PascalCase` identifier (every fieldless
/// enum variant this module reads renders this way) to the `snake_case`
/// spelling the wire layer already uses for the same names — so a status
/// line and a recorded transcript describe the same fact with the same
/// word, without this module hand-copying the wire layer's own rename
/// table.
fn snake_case(input: &str) -> String {
    let mut output = String::new();
    for (index, character) in input.chars().enumerate() {
        if character.is_uppercase() {
            if index != 0 {
                output.push('_');
            }
            output.extend(character.to_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::path::PathBuf;

    use summoners_cards::built_in_catalog;
    use summoners_core::domain::cards::ComponentKind;
    use summoners_core::domain::state::LossReason as CoreLossReason;
    use summoners_match_log::TranscriptV1;
    use summoners_match_log::replay::prepare_scenario;

    use super::*;

    fn golden(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../match-log/tests/goldens")
            .join(name)
    }

    fn initial_state(name: &str) -> GameState {
        let catalog = built_in_catalog().expect("the built-in catalog loads");
        let bytes = std::fs::read(golden(name)).expect("the golden reads");
        let transcript = TranscriptV1::parse(bytes.as_slice()).expect("the golden parses");
        prepare_scenario(&transcript, catalog.library())
            .expect("the golden's scenario prepares")
            .initial_state
    }

    #[test]
    fn an_empty_board_renders_every_zone_as_empty() {
        let view = compact_view(&initial_state("resignation.ndjson"));

        assert_eq!(
            view.to_string(),
            "turn: one / phase: main / decides: one\n\
             one: main: - / bench: [-, -, -] / hand: [] / deck: 0 / prizes: 0 / mana: 0/0/0\n\
             two: main: - / bench: [-, -, -] / hand: [] / deck: 0 / prizes: 0 / mana: 0/0/0\n\
             status: playing"
        );
    }

    #[test]
    fn a_populated_main_summon_renders_its_instance_and_definition() {
        let state = initial_state("terminal_empty_deck.ndjson");
        let view = compact_view(&state);

        let expected_one = state
            .players
            .one
            .main
            .as_ref()
            .expect("the golden seeds a Main Summon for player one")
            .chain
            .top();
        assert_eq!(
            view.one.main,
            Some(SummonView {
                instance: expected_one.instance.0,
                definition: expected_one.def,
                damage: 0,
                readiness: Readiness::Ready,
            })
        );
        assert!(
            view.one.to_string().contains(&format!(
                "main: {}@{} dmg:0 ready",
                expected_one.instance.0, expected_one.def
            )),
            "unexpected rendering: {}",
            view.one
        );
    }

    #[test]
    fn decides_reads_the_one_core_rule_rather_than_re_deriving_it() {
        let state = initial_state("terminal_empty_deck.ndjson");

        assert_eq!(compact_view(&state).decides, required_actor(&state));
    }

    #[test]
    fn status_view_renders_ended_with_the_winner_and_the_reason() {
        let view = StatusView::Ended {
            winner: PlayerId::Two,
            reason: LossReason::Resignation,
        };

        assert_eq!(view.to_string(), "ended(winner:two reason:resignation)");
    }

    #[test]
    fn status_view_renders_broken_with_the_rule_and_the_expected_component() {
        let breakage = Breakage {
            rule: "destruction",
            entity: summoners_core::domain::cards::EntityId::parse(&"bb".repeat(16))
                .expect("valid test entity ID"),
            expected: ComponentKind::RetreatCost,
        };

        assert_eq!(
            StatusView::Broken(breakage).to_string(),
            "broken(destruction expected:retreat_cost)"
        );
    }

    #[test]
    fn format_accepted_event_numbers_and_serializes_the_event() {
        let event = GameEvent::GameEnded {
            winner: PlayerId::Two,
            reason: CoreLossReason::Resignation,
        };

        let line = format_accepted_event(0, &event);

        assert!(line.starts_with("event 0: "));
        let json = line.trim_start_matches("event 0: ");
        let parsed: EventV1 = serde_json::from_str(json).expect("the event serializes as JSON");
        assert_eq!(parsed, EventV1::from(&event));
    }

    #[test]
    fn format_accepted_events_numbers_every_event_in_order() {
        let events = vec![
            GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: CoreLossReason::Resignation,
            },
            GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: CoreLossReason::Resignation,
            },
        ];

        let lines = format_accepted_events(&events);

        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("event 0: "));
        assert!(lines[1].starts_with("event 1: "));
    }

    #[test]
    fn format_rejection_serializes_the_error() {
        let line = format_rejection(ActionError::NotYourDecision);

        assert!(line.starts_with("rejected: "));
        let json = line.trim_start_matches("rejected: ");
        let parsed: ErrorV1 = serde_json::from_str(json).expect("the error serializes as JSON");
        assert_eq!(parsed, ErrorV1::from(ActionError::NotYourDecision));
    }
}
