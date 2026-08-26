//! The transcript fixtures under `crates/cli/tests/goldens`, recorded from
//! scripted engine drives — never hand-written lines.
//!
//! The functional half builds each transcript entirely in memory through
//! `RecordedMatch`: one opening `Scenario` parsed through the built-in card
//! catalog, then a fixed pass-priority drive until the match ends. The shell
//! half resolves the output directory and writes the finished bytes to files.

use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::Args;
use color_eyre::eyre::{Result, eyre};
use summoners_cards::{BuiltInCatalog, BuiltInError, CardLibrary, built_in_catalog};
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::{CardSet, EntityId},
        errors::{ActionError, InvalidScenario},
        ids::{CardInstanceId, PlayerId},
        state::{
            CardRef, Coin, GameOutcome, GameState, GameStatus, LossReason, ManaBank, PerPlayer,
            Phase, Readiness,
        },
    },
    scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario},
};
use summoners_match_log::{
    HeaderMetadataV1, RecordedMatch, RecordedStep, RecordingError, SetRequirementV1,
};

/// Where the committed transcripts live, relative to the workspace root.
const DEFAULT_OUT_DIR: &str = "crates/cli/tests/goldens";

/// A replay with fewer recorded events than this gives its steps too little
/// to verify.
const MIN_RECORDED_EVENTS: usize = 12;

/// Upper bound on one drive, so a stalled script reports an error instead of
/// looping forever.
const MAX_SCRIPTED_ACTIONS: usize = 256;

/// The Set every transcript declares over its initial state; it must match
/// the revision embedded in the built-in catalog.
const REQUIRED_SET: (&str, u32) = ("foundations", 1);

/// Foundations bases anchoring exactly one Mana Type (Matter), so natural
/// production never pauses the drive on a type choice.
const MAIN_CARD: &str = "foundations/warden-initiate";
const DECK_CARDS: [&str; 2] = ["foundations/warden-initiate", "foundations/quarry-scout"];

/// Card instance ids must be unique across both boards; each seat owns one
/// disjoint block so decks can repeat printed cards freely.
const STARTER_INSTANCE_BASE: u32 = 1;
const RESPONDER_INSTANCE_BASE: u32 = 1_000;

// ---------------------------------------------------------------------------
// Functional core — typed generation through the recorder, no input or output
// ---------------------------------------------------------------------------

/// One transcript's shape. Both players draw once per round — each inside
/// the closing Priority pass of the previous round — so giving the responder
/// the shorter Deck makes their Upkeep draw hit an empty Deck during the
/// round after their last success. That ending runs exactly
/// `6 × responder_deck_len + 3` accepted actions, while the starter still
/// holds cards to draw.
#[derive(Debug)]
struct GoldenSpec {
    file_name: &'static str,
    starter: PlayerId,
    coin_for_two: bool,
    starter_deck_len: usize,
    responder_deck_len: usize,
}

const GOLDEN_SPECS: &[GoldenSpec] = &[
    GoldenSpec {
        file_name: "four_card_deck_loss.ndjson",
        starter: PlayerId::One,
        coin_for_two: false,
        starter_deck_len: 4,
        responder_deck_len: 2,
    },
    GoldenSpec {
        file_name: "five_card_deck_loss.ndjson",
        starter: PlayerId::Two,
        coin_for_two: true,
        starter_deck_len: 6,
        responder_deck_len: 5,
    },
];

/// One finished recording and the facts an operator line reports about it.
#[derive(Debug)]
struct GeneratedTranscript {
    bytes: Vec<u8>,
    steps: usize,
    events: usize,
    outcome: GameOutcome,
}

/// Why one transcript could not be generated.
#[derive(Debug)]
enum GenerationError {
    Catalog(BuiltInError),
    UnknownCard {
        qualified_key: String,
    },
    Parse(InvalidScenario),
    Recording(RecordingError),
    PendingDecision {
        step: usize,
    },
    UnexpectedShape {
        step: usize,
    },
    IllegalAction {
        step: usize,
        source: ActionError,
    },
    NotEnded {
        status: GameStatus,
    },
    NotAnEmptyDeckLoss {
        outcome: GameOutcome,
    },
    TooShort {
        file_name: &'static str,
        events: usize,
        minimum: usize,
    },
    DriveOverrun {
        step: usize,
    },
}

impl fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Catalog(source) => write!(formatter, "built-in catalog failed to load: {source}"),
            Self::UnknownCard { qualified_key } => {
                write!(
                    formatter,
                    "the built-in catalog holds no card {qualified_key}"
                )
            }
            Self::Parse(source) => {
                write!(formatter, "the opening scenario does not parse: {source:?}")
            }
            Self::Recording(source) => write!(formatter, "the recording failed: {source}"),
            Self::PendingDecision { step } => write!(
                formatter,
                "step {step}: the engine paused for a decision the pass-priority script cannot answer"
            ),
            Self::UnexpectedShape { step } => write!(
                formatter,
                "step {step}: neither a resting Main Phase nor an open Priority window"
            ),
            Self::IllegalAction { step, source } => write!(
                formatter,
                "step {step}: the engine rejected a scripted action: {source:?}"
            ),
            Self::NotEnded { status } => {
                write!(
                    formatter,
                    "the drive settled without ending the match: {status:?}"
                )
            }
            Self::NotAnEmptyDeckLoss { outcome } => write!(
                formatter,
                "the match ended another way: winner {}, reason {}",
                seat_name(outcome.winner),
                loss_reason_name(outcome.reason)
            ),
            Self::TooShort {
                file_name,
                events,
                minimum,
            } => write!(
                formatter,
                "{file_name}: only {events} events were recorded; at least {minimum} give replay steps meaning"
            ),
            Self::DriveOverrun { step } => write!(
                formatter,
                "the pass-priority drive ran past {step} actions without ending the match"
            ),
        }
    }
}

impl Error for GenerationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Catalog(source) => Some(source),
            Self::Recording(source) => Some(source),
            _ => None,
        }
    }
}

fn generate(
    catalog: &BuiltInCatalog,
    spec: &GoldenSpec,
) -> Result<GeneratedTranscript, GenerationError> {
    let (cards, scenario) = scenario_of(spec, catalog.library())?;
    let transcript = record_transcript(Arc::clone(&cards), &scenario)?;
    if transcript.events < MIN_RECORDED_EVENTS {
        return Err(GenerationError::TooShort {
            file_name: spec.file_name,
            events: transcript.events,
            minimum: MIN_RECORDED_EVENTS,
        });
    }
    Ok(transcript)
}

fn scenario_of(
    spec: &GoldenSpec,
    library: &CardLibrary,
) -> Result<(Arc<CardSet>, Scenario), GenerationError> {
    let main_definition = card_definition(library, MAIN_CARD)?;
    let mut deck_definitions = Vec::new();
    for key in DECK_CARDS {
        deck_definitions.push(card_definition(library, key)?);
    }

    let starter_player = seated_player(
        main_definition,
        &deck_definitions,
        spec.starter_deck_len,
        STARTER_INSTANCE_BASE,
    );
    let responder_player = seated_player(
        main_definition,
        &deck_definitions,
        spec.responder_deck_len,
        RESPONDER_INSTANCE_BASE,
    );
    let players = match spec.starter {
        PlayerId::One => PerPlayer::new(starter_player, responder_player),
        PlayerId::Two => PerPlayer::new(responder_player, starter_player),
    };

    let scenario = Scenario {
        players,
        active_player: spec.starter,
        coin: spec.coin_for_two.then_some(Coin),
    };
    Ok((library.core_cards(), scenario))
}

fn seated_player(
    main_definition: EntityId,
    deck_definitions: &[EntityId],
    deck_len: usize,
    instance_base: u32,
) -> ScenarioPlayer {
    let main = Some(ScenarioSummon {
        chain: vec![numbered(main_definition, instance_base)],
        damage: 0,
        readiness: Readiness::Ready,
    });
    let deck = (0..deck_len)
        .map(|index| {
            numbered(
                deck_definitions[index % deck_definitions.len()],
                instance_base + index as u32 + 1,
            )
        })
        .collect();

    ScenarioPlayer {
        main,
        bench: [None, None, None],
        deck,
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
    }
}

fn card_definition(
    library: &CardLibrary,
    qualified_key: &str,
) -> Result<EntityId, GenerationError> {
    library
        .card_id(qualified_key)
        .ok_or_else(|| GenerationError::UnknownCard {
            qualified_key: qualified_key.to_string(),
        })
}

fn numbered(definition: EntityId, instance: u32) -> CardRef {
    CardRef {
        instance: CardInstanceId(instance),
        def: definition,
    }
}

fn record_transcript(
    cards: Arc<CardSet>,
    scenario: &Scenario,
) -> Result<GeneratedTranscript, GenerationError> {
    let initial_state = from_scenario(cards, scenario).map_err(GenerationError::Parse)?;
    let mut recording = RecordedMatch::start(
        Vec::new(),
        HeaderMetadataV1::new(),
        vec![required_set()],
        initial_state,
    )
    .map_err(GenerationError::Recording)?;

    let mut steps = 0;
    let mut events = 0;
    while recording.state().status.is_playing() {
        if steps >= MAX_SCRIPTED_ACTIONS {
            return Err(GenerationError::DriveOverrun { step: steps });
        }
        let action = next_scripted_action(recording.state(), steps + 1)?;
        match recording
            .submit(&action)
            .map_err(GenerationError::Recording)?
        {
            RecordedStep::Accepted { events: emitted } => events += emitted.len(),
            RecordedStep::Rejected { error } => {
                return Err(GenerationError::IllegalAction {
                    step: steps + 1,
                    source: error,
                });
            }
        }
        steps += 1;
    }

    let status = recording.state().status;
    let outcome = match status {
        GameStatus::Ended(outcome) => outcome,
        _ => return Err(GenerationError::NotEnded { status }),
    };
    if outcome.reason != LossReason::EmptyDeckDraw {
        return Err(GenerationError::NotAnEmptyDeckLoss { outcome });
    }

    Ok(GeneratedTranscript {
        bytes: recording.into_writer(),
        steps,
        events,
        outcome,
    })
}

/// The next action of the pass-priority drive. Inside an open window the
/// holder passes (a second consecutive pass closes the window); from a
/// resting Main Phase the active player ends their turn, which opens the
/// final response window defender-first.
fn next_scripted_action(state: &GameState, step: usize) -> Result<GameAction, GenerationError> {
    if state.pending.is_some() {
        return Err(GenerationError::PendingDecision { step });
    }
    if let Some(window) = state.turn.window {
        return Ok(GameAction::PassPriority {
            player: window.holder,
        });
    }
    if state.turn.phase == Phase::Main {
        return Ok(GameAction::EndTurn {
            player: state.turn.active_player,
        });
    }
    Err(GenerationError::UnexpectedShape { step })
}

fn required_set() -> SetRequirementV1 {
    SetRequirementV1 {
        set: REQUIRED_SET.0.to_string(),
        revision: REQUIRED_SET.1,
    }
}

fn seat_name(player: PlayerId) -> &'static str {
    match player {
        PlayerId::One => "one",
        PlayerId::Two => "two",
    }
}

fn loss_reason_name(reason: LossReason) -> &'static str {
    match reason {
        LossReason::ThirdMainLoss => "third_main_loss",
        LossReason::NoPromotionAvailable => "no_promotion_available",
        LossReason::EmptyDeckDraw => "empty_deck_draw",
    }
}

// ---------------------------------------------------------------------------
// Imperative shell — catalog load, filesystem writes, operator output
// ---------------------------------------------------------------------------

/// Command line for `cargo run -p xtask -- golden`.
#[derive(Args)]
pub struct GoldenArgs {
    /// Write the transcripts here instead of the committed default directory
    #[arg(long)]
    pub out: Option<PathBuf>,
}

/// Record every fixture and write it out, one summary line per transcript.
pub fn run(args: &GoldenArgs) -> Result<()> {
    let out_dir = args.out.clone().unwrap_or_else(default_goldens_dir);
    let catalog = built_in_catalog().map_err(GenerationError::Catalog)?;
    fs::create_dir_all(&out_dir)?;

    for spec in GOLDEN_SPECS {
        let transcript = generate(&catalog, spec).map_err(|error| eyre!("{spec:?}: {error}"))?;
        let path = out_dir.join(spec.file_name);
        fs::write(&path, &transcript.bytes)?;
        println!(
            "wrote {} ({} steps, {} events; {} won by {})",
            path.display(),
            transcript.steps,
            transcript.events,
            seat_name(transcript.outcome.winner),
            loss_reason_name(transcript.outcome.reason)
        );
    }
    Ok(())
}

/// The committed default output directory, resolved against the workspace
/// root this binary was built from rather than the invoking shell's cwd.
fn default_goldens_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
        .join(DEFAULT_OUT_DIR)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use summoners_core::engine::apply::apply;

    fn catalog() -> Arc<BuiltInCatalog> {
        built_in_catalog().expect("the embedded catalog loads")
    }

    #[test]
    fn the_drive_opens_with_the_starter_ending_their_turn() {
        let catalog = catalog();
        let (_, scenario) =
            scenario_of(&GOLDEN_SPECS[0], catalog.library()).expect("the scenario resolves");
        let state = from_scenario(catalog.library().core_cards(), &scenario).expect("parses");

        assert_eq!(
            next_scripted_action(&state, 1).expect("a resting Main Phase takes EndTurn"),
            GameAction::EndTurn {
                player: GOLDEN_SPECS[0].starter
            }
        );
    }

    #[test]
    fn the_defender_answers_an_opened_window_first() {
        let catalog = catalog();
        let (_, scenario) =
            scenario_of(&GOLDEN_SPECS[0], catalog.library()).expect("the scenario resolves");
        let state = from_scenario(catalog.library().core_cards(), &scenario).expect("parses");

        let opened = apply(
            &state,
            &GameAction::EndTurn {
                player: PlayerId::One,
            },
        )
        .expect("EndTurn opens the response window");

        assert_eq!(
            next_scripted_action(&opened.state, 2).expect("an open window takes a pass"),
            GameAction::PassPriority {
                player: PlayerId::Two
            }
        );
    }

    #[test]
    fn four_card_deck_loss_records_the_responder_running_dry_under_one() {
        let transcript = generate(&catalog(), &GOLDEN_SPECS[0]).expect("generates");
        assert_eq!(transcript.outcome.winner, PlayerId::One);
        assert_eq!(transcript.outcome.reason, LossReason::EmptyDeckDraw);
        assert_eq!(transcript.steps, 15);
        assert!(transcript.events >= MIN_RECORDED_EVENTS);
    }

    #[test]
    fn five_card_deck_loss_records_the_responder_running_dry_under_two() {
        let transcript = generate(&catalog(), &GOLDEN_SPECS[1]).expect("generates");
        assert_eq!(transcript.outcome.winner, PlayerId::Two);
        assert_eq!(transcript.outcome.reason, LossReason::EmptyDeckDraw);
        assert_eq!(transcript.steps, 33);
        assert!(transcript.events >= MIN_RECORDED_EVENTS);
    }

    #[test]
    fn identical_specs_record_identical_bytes() {
        let first = generate(&catalog(), &GOLDEN_SPECS[0])
            .expect("generates")
            .bytes;
        let second = generate(&catalog(), &GOLDEN_SPECS[0])
            .expect("generates")
            .bytes;
        assert_eq!(first, second);
    }

    #[test]
    fn a_too_soon_empty_deck_end_rejects_a_transcript_as_too_short() {
        let tiny_spec = GoldenSpec {
            file_name: "tiny.ndjson",
            starter: PlayerId::One,
            coin_for_two: false,
            starter_deck_len: 1,
            responder_deck_len: 0,
        };
        let error = generate(&catalog(), &tiny_spec)
            .expect_err("an end after three actions records too few events to be meaningful");

        let GenerationError::TooShort {
            file_name, minimum, ..
        } = error
        else {
            panic!("expected a too-short rejection, found {error}");
        };
        assert_eq!(file_name, "tiny.ndjson");
        assert_eq!(minimum, MIN_RECORDED_EVENTS);
    }

    #[test]
    fn a_main_without_any_board_summon_cannot_parse_into_a_drive() {
        // An empty board is the one scenario shape this generator can hand
        // over that the parser refuses outright.
        let empty_board = GoldenSpec {
            file_name: "empty.ndjson",
            starter: PlayerId::One,
            coin_for_two: false,
            starter_deck_len: 2,
            responder_deck_len: 2,
        };
        let (_, mut scenario) = scenario_of(&empty_board, catalog().library()).expect("resolves");
        scenario.players.one.main = None;

        let error = record_transcript(catalog().library().core_cards(), &scenario)
            .expect_err("a player with no Summon in play cannot open a match");

        assert!(matches!(error, GenerationError::Parse(_)));
    }

    #[test]
    fn seat_names_read_out_lowercase_words() {
        assert_eq!(seat_name(PlayerId::One), "one");
        assert_eq!(seat_name(PlayerId::Two), "two");
    }

    #[test]
    fn loss_reason_names_match_the_wire_snake_case() {
        assert_eq!(
            loss_reason_name(LossReason::ThirdMainLoss),
            "third_main_loss"
        );
        assert_eq!(
            loss_reason_name(LossReason::NoPromotionAvailable),
            "no_promotion_available"
        );
        assert_eq!(
            loss_reason_name(LossReason::EmptyDeckDraw),
            "empty_deck_draw"
        );
    }

    #[test]
    fn every_declared_set_requirement_pins_the_embedded_revision() {
        let requirement = required_set();
        let revision = catalog()
            .library()
            .set_revision(&requirement.set)
            .expect("foundations ships in the embedded catalog");
        assert_eq!(revision, requirement.revision);
    }
}
