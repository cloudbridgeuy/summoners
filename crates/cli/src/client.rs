use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::PlayArgs;
use crate::prompt::{PromptEffect, PromptState, reduce};
use crate::protocol::{
    ClientEnvelope, OutcomeReasonView, OutcomeView, PlayerView, Seat, ServerEnvelope, VERSION,
};
use summoners_match_log::{ActionV1, wire::PlayerIdV1};

const MAX_SERVER_FRAME: usize = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, thiserror::Error)]
pub enum PlayError {
    #[error("cannot connect: {0}")]
    Connect(io::Error),
    #[error("cannot use socket: {0}")]
    Socket(io::Error),
    #[error("cannot encode protocol message: {0}")]
    Encode(serde_json::Error),
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    revision: u64,
    view: Option<PlayerView>,
    history: Vec<String>,
}

fn is_stale(tagged_revision: u64, snapshot_revision: u64) -> bool {
    tagged_revision < snapshot_revision
}

pub fn play(args: &PlayArgs) -> Result<(), PlayError> {
    let mut stream =
        TcpStream::connect((args.host.as_str(), args.port)).map_err(PlayError::Connect)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(PlayError::Socket)?;
    let seat = if args.player == 1 {
        Seat::One
    } else {
        Seat::Two
    };
    send(
        &mut stream,
        &ClientEnvelope::Join {
            version: VERSION,
            seat,
        },
    )?;
    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let reader = stream.try_clone().map_err(PlayError::Socket)?;
    let observed = Arc::clone(&snapshot);
    let (terminal_sender, terminal_receiver) = std::sync::mpsc::channel();
    let listener = std::thread::spawn(move || receive(reader, &observed, &terminal_sender));
    let (input_sender, input_receiver) = std::sync::mpsc::channel();
    let input_snapshot = Arc::clone(&snapshot);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let tagged = match input_snapshot.lock() {
                Ok(snapshot) => snapshot.revision,
                Err(_) => return,
            };
            if input_sender.send((line, tagged)).is_err() {
                return;
            }
        }
    });
    let mut request_id = 0_u64;
    let mut giveup_pending = false;
    let mut prompt = PromptState::Menu { revision: 0 };
    while matches!(
        terminal_receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ) {
        let (line, tagged_revision) = match input_receiver.recv_timeout(POLL_INTERVAL) {
            Ok((line, tagged)) => (line.map_err(PlayError::Socket)?, tagged),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let current = snapshot
                    .lock()
                    .map_err(|_| PlayError::Socket(io::Error::other("snapshot lock")))?
                    .revision;
                let (next, effects) = crate::prompt::revised(&prompt, current);
                if effects.contains(&PromptEffect::Cancelled) {
                    giveup_pending = false;
                    println!("Prompt cancelled");
                }
                prompt = next;
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let current_snapshot = snapshot
            .lock()
            .map_err(|_| PlayError::Socket(io::Error::other("snapshot lock")))?
            .clone();
        if is_stale(tagged_revision, current_snapshot.revision) {
            println!("Discarded stale input");
            continue;
        }
        if line.trim().eq_ignore_ascii_case("give up") {
            giveup_pending = true;
            println!("Confirm Give up with yes");
            continue;
        }
        if giveup_pending && line.trim().eq_ignore_ascii_case("yes") {
            giveup_pending = false;
            request_id += 1;
            send(
                &mut stream,
                &ClientEnvelope::Submit {
                    version: VERSION,
                    request_id,
                    based_on_revision: current_snapshot.revision,
                    action: ActionV1::Resign {
                        player: match seat {
                            Seat::One => PlayerIdV1::One,
                            Seat::Two => PlayerIdV1::Two,
                        },
                    },
                },
            )?;
            continue;
        }
        if giveup_pending {
            giveup_pending = false;
            println!("Give up cancelled");
            continue;
        }
        let Some(latest) = current_snapshot.view.clone() else {
            continue;
        };
        if let Some(card) = inspected_card(&latest, line.trim()) {
            println!("{}", inspect_card(card));
            continue;
        }
        if line.trim().starts_with("inspect ") {
            println!("Invalid inspection");
            continue;
        }
        if line.trim().eq_ignore_ascii_case("history") {
            for notice in current_snapshot.history {
                println!("{notice}");
            }
            continue;
        }
        let current = current_snapshot.revision;
        let (revised_prompt, revision_effects) = crate::prompt::revised(&prompt, current);
        prompt = revised_prompt;
        if revision_effects.contains(&PromptEffect::Cancelled) {
            println!("Prompt cancelled");
        }
        if matches!(prompt, PromptState::Menu { .. })
            && latest
                .pending
                .as_ref()
                .is_some_and(|pending| pending.owner == seat)
            && let Some(form) = crate::prompt::forced_form(&latest)
        {
            prompt = PromptState::Form {
                revision: current,
                form,
            };
        }
        let (next, effects) = reduce(
            prompt,
            &latest,
            if line.trim().eq_ignore_ascii_case("give up") {
                "8"
            } else {
                &line
            },
        );
        prompt = next;
        for effect in effects {
            match effect {
                PromptEffect::Render(lines) => {
                    for line in lines {
                        println!("{line}");
                    }
                }
                PromptEffect::Cancelled => println!("Prompt cancelled"),
                PromptEffect::Submit {
                    revision: based_on_revision,
                    action,
                } => {
                    if based_on_revision != current {
                        println!("Prompt cancelled");
                        continue;
                    }
                    request_id += 1;
                    send(
                        &mut stream,
                        &ClientEnvelope::Submit {
                            version: VERSION,
                            request_id,
                            based_on_revision,
                            action,
                        },
                    )?;
                }
            }
        }
    }
    drop(stream);
    let _ = listener.join();
    Ok(())
}

fn send(stream: &mut TcpStream, message: &ClientEnvelope) -> Result<(), PlayError> {
    let mut bytes = serde_json::to_vec(message).map_err(PlayError::Encode)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(PlayError::Socket)
}

fn receive(
    stream: TcpStream,
    snapshot: &Arc<Mutex<Snapshot>>,
    terminal: &std::sync::mpsc::Sender<()>,
) {
    let mut reader = BufReader::new(stream);
    while let Ok(frame) = read_frame(&mut reader) {
        let Ok(message) = serde_json::from_slice::<ServerEnvelope>(&frame) else {
            return;
        };
        match message {
            ServerEnvelope::Update {
                revision: next,
                view,
                notices,
                result,
                ..
            } => {
                if let Ok(mut current) = snapshot.lock() {
                    current.revision = next;
                    current.view = Some(view.clone());
                    current
                        .history
                        .extend(notices.iter().map(|notice| notice.text.clone()));
                }
                println!("{}", render_view(&view));
                for notice in notices {
                    println!("{}", notice.text);
                }
                if let Some(crate::protocol::SubmissionResult::Rejected { reason }) = result {
                    println!("Rejected {reason}");
                }
                for line in crate::prompt::prompt(&view, next) {
                    println!("{line}");
                }
            }
            ServerEnvelope::Finished {
                outcome,
                view,
                notices,
                ..
            } => {
                if let Ok(mut current) = snapshot.lock() {
                    current
                        .history
                        .extend(notices.iter().map(|notice| notice.text.clone()));
                }
                println!("{}", render_view(&view));
                for notice in notices {
                    println!("{}", notice.text);
                }
                println!("Finished: {}", render_outcome(&outcome));
                let _ = terminal.send(());
                return;
            }
            ServerEnvelope::Waiting { .. } => println!("Waiting"),
            ServerEnvelope::Rejected { reason, .. } => println!("Rejected {reason}"),
            ServerEnvelope::Stopped { reason } => {
                println!("Stopped {reason}");
                let _ = terminal.send(());
                return;
            }
        }
    }
    let _ = terminal.send(());
}

fn read_frame(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    loop {
        if bytes.len() == MAX_SERVER_FRAME {
            return Err(io::Error::other("frame limit"));
        }
        let count = reader.read(&mut byte)?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed"));
        }
        if byte[0] == b'\n' {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
}

fn render_view(view: &PlayerView) -> String {
    let hand = view
        .hand
        .iter()
        .map(|card| format!("#{} {}", card.instance, render_card(&card.card)))
        .collect::<Vec<_>>()
        .join(", ");
    let one = render_board("Player One", &view.players.one.board);
    let two = render_board("Player Two", &view.players.two.board);
    format!("Hand: {hand}\nBoard: {one}; {two}\nPhase: {:?}", view.phase)
}

fn render_board(name: &str, board: &crate::protocol::BoardView) -> String {
    let main = render_summon(board.main.as_ref());
    let bench = board
        .bench
        .iter()
        .map(|summon| render_summon(summon.as_ref()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name} Main {main}; Bench [{bench}]")
}
fn render_summon(summon: Option<&crate::protocol::SummonView>) -> String {
    summon.map_or_else(
        || "empty".to_string(),
        |summon| {
            let chain = summon
                .chain
                .iter()
                .map(render_card)
                .collect::<Vec<_>>()
                .join(" -> ");
            format!("{chain} damage {} {:?}", summon.damage, summon.readiness)
        },
    )
}

fn render_card(card: &crate::protocol::CardDescription) -> String {
    let life = card
        .life
        .map_or_else(String::new, |life| format!(" life {life}"));
    let effects = if card.effects.is_empty() {
        String::new()
    } else {
        format!(" [{}]", card.effects.join(", "))
    };
    format!("{}{}{}", card.name, life, effects)
}
fn inspect_card(card: &crate::protocol::CardDescription) -> String {
    let abilities = if card.abilities.is_empty() {
        String::new()
    } else {
        format!(
            " [{}]",
            card.abilities
                .iter()
                .map(render_ability)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let cost = card
        .cost
        .as_ref()
        .map_or_else(String::new, |cost| format!(" cost {cost}"));
    let retreat = card
        .retreat_cost
        .map_or_else(String::new, |cost| format!(" retreat {cost}"));
    let mana = if card.mana_types.is_empty() {
        String::new()
    } else {
        format!(" Mana {}", card.mana_types.join(", "))
    };
    format!(
        "{}{}{}{}{}{}{}",
        render_card(card),
        retreat,
        mana,
        cost,
        abilities,
        if card.modifiers.is_empty() {
            String::new()
        } else {
            format!(" modifiers {}", card.modifiers.join(", "))
        },
        if card.persistent { " persistent" } else { "" }
    )
}
fn render_ability(ability: &crate::protocol::AbilityDescription) -> String {
    let kind = match ability.kind {
        crate::protocol::AbilityKind::Skill => "Skill",
        crate::protocol::AbilityKind::Attack => "Attack",
        crate::protocol::AbilityKind::Trigger => "Trigger",
    };
    let cost = ability
        .cost
        .as_ref()
        .map_or_else(String::new, |cost| format!(" cost {cost}"));
    let event = ability
        .trigger_event
        .as_ref()
        .map_or_else(String::new, |event| format!(" after {event}"));
    let timing = ability
        .timing
        .as_ref()
        .map_or_else(String::new, |timing| format!(" {timing}"));
    let effects = if ability.effects.is_empty() {
        String::new()
    } else {
        format!(": {}", ability.effects.join(", "))
    };
    format!(
        "{kind} {}{}{}{}{}{}{}{}",
        ability.name,
        cost,
        event,
        timing,
        if ability.respondable {
            " respondable"
        } else {
            ""
        },
        effects,
        if ability.modifiers.is_empty() {
            String::new()
        } else {
            format!(" modifiers {}", ability.modifiers.join(", "))
        },
        if ability.persistent {
            " persistent"
        } else {
            ""
        }
    )
}

fn inspected_card<'a>(
    view: &'a PlayerView,
    command: &str,
) -> Option<&'a crate::protocol::CardDescription> {
    let mut fields = command.split_whitespace();
    if fields.next()? != "inspect" {
        return None;
    }
    let target = fields.next()?;
    if target == "board" {
        let seat = match fields.next()? {
            "1" => Seat::One,
            "2" => Seat::Two,
            _ => return None,
        };
        let position = fields.next()?.parse::<usize>().ok()?;
        if fields.next().is_some() || !(1..=4).contains(&position) {
            return None;
        }
        let board = match seat {
            Seat::One => &view.players.one.board,
            Seat::Two => &view.players.two.board,
        };
        let summon = if position == 1 {
            board.main.as_ref()
        } else {
            board.bench[position - 2].as_ref()
        };
        return summon?.chain.last();
    }
    let index = target.parse::<usize>().ok()?;
    if fields.next().is_some() || index == 0 {
        return None;
    }
    view.hand.get(index - 1).map(|card| &card.card)
}

fn render_outcome(outcome: &OutcomeView) -> String {
    let winner = match outcome.winner {
        Seat::One => "Player One",
        Seat::Two => "Player Two",
    };
    let reason = match outcome.reason {
        OutcomeReasonView::ThirdMainLoss => "third Main loss",
        OutcomeReasonView::NoPromotionAvailable => "no promotion available",
        OutcomeReasonView::EmptyDeckDraw => "empty deck draw",
        OutcomeReasonView::Resignation => "resignation",
    };
    format!("{winner} wins by {reason}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::protocol::{
        AbilityDescription, AbilityKind, BoardView, CardDescription, ManaView, PhaseView,
        PlayerPublicView, ReadinessView, SeatsView, SummonView,
    };
    use summoners_match_log::wire::EntityIdV1;

    fn view() -> PlayerView {
        let card = CardDescription {
            name: "Whelp".to_string(),
            life: Some(40),
            retreat_cost: Some(1),
            mana_types: vec!["Matter".to_string()],
            cost: None,
            abilities: Vec::new(),
            effects: vec!["DealDamage".to_string()],
            modifiers: Vec::new(),
            timing: None,
            persistent: false,
        };
        let player = PlayerPublicView {
            board: BoardView {
                main: None,
                bench: [None, None, None],
            },
            mana: ManaView {
                matter: 0,
                mind: 0,
                spirit: 0,
            },
            main_losses: 0,
            discard: Vec::new(),
            persistent: Vec::new(),
            deck_count: 13,
            prize_count: 2,
            hand_count: 5,
        };
        PlayerView {
            you: Seat::One,
            hand: vec![crate::protocol::HandCardView { instance: 7, card }],
            players: SeatsView {
                one: player.clone(),
                two: player,
            },
            coin: true,
            stack: Vec::new(),
            phase: PhaseView::Main,
            active_player: Seat::One,
            priority_holder: None,
            pending: None,
            outcome: None,
        }
    }

    #[test]
    fn renderer_shows_opening_hand_and_board() {
        let rendered = render_view(&view());
        assert!(rendered.contains("Hand: #7 Whelp"));
        assert!(rendered.contains("Board: Player One"));
    }

    #[test]
    fn render_helpers_show_chain_damage_and_bench() {
        let card = CardDescription {
            name: "Whelp".to_string(),
            life: Some(40),
            retreat_cost: None,
            mana_types: Vec::new(),
            cost: None,
            abilities: Vec::new(),
            effects: vec!["draw 2 cards".to_string()],
            modifiers: Vec::new(),
            timing: None,
            persistent: false,
        };
        let summon = SummonView {
            chain: vec![card.clone()],
            damage: 3,
            readiness: ReadinessView::Ready,
            owner: Seat::One,
            controller: Seat::One,
        };
        let board = BoardView {
            main: Some(summon.clone()),
            bench: [Some(summon), None, None],
        };
        assert!(render_card(&card).contains("Whelp life 40"));
        assert!(render_summon(board.main.as_ref()).contains("damage 3"));
        assert!(render_board("Player One", &board).contains("Bench [Whelp"));
    }

    #[test]
    fn resignation_outcome_is_readable() {
        assert_eq!(
            render_outcome(&OutcomeView {
                winner: Seat::One,
                reason: OutcomeReasonView::Resignation
            }),
            "Player One wins by resignation"
        );
    }

    #[test]
    fn frame_bound_rejects_unterminated_server_input() {
        let bytes = vec![b'x'; MAX_SERVER_FRAME];
        assert!(read_frame(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn input_tagged_behind_the_snapshot_revision_is_stale() {
        assert!(is_stale(0, 1));
        assert!(!is_stale(1, 1));
        assert!(!is_stale(2, 1));
    }

    #[test]
    fn inspection_uses_strict_one_based_hand_indices_and_visible_board_cards() {
        let mut view = view();
        view.hand[0].card.name = "Hand".to_string();
        let skill = AbilityDescription {
            kind: AbilityKind::Skill,
            id: EntityIdV1("skill-id".to_string()),
            name: "Skill".to_string(),
            cost: Some("matter 1 mind 0 spirit 0 generic 0".to_string()),
            effects: vec!["look at Prizes".to_string()],
            trigger_event: None,
            timing: Some("support".to_string()),
            respondable: true,
            persistent: true,
            modifiers: vec!["reduce incoming attack damage by 1".to_string()],
        };
        let board_card = CardDescription {
            name: "Board".to_string(),
            life: None,
            retreat_cost: None,
            mana_types: Vec::new(),
            cost: None,
            abilities: vec![skill],
            effects: Vec::new(),
            modifiers: Vec::new(),
            timing: None,
            persistent: false,
        };
        view.players.two.board.main = Some(SummonView {
            chain: vec![board_card],
            damage: 0,
            readiness: ReadinessView::Ready,
            owner: Seat::Two,
            controller: Seat::Two,
        });
        assert_eq!(
            inspected_card(&view, "inspect 1").map(|card| &card.name),
            Some(&"Hand".to_string())
        );
        assert!(inspected_card(&view, "inspect 0").is_none());
        let inspected =
            inspect_card(inspected_card(&view, "inspect board 2 1").expect("board card"));
        assert!(inspected.contains("Skill Skill"));
        assert!(inspected.contains("look at Prizes"));
        assert!(inspected.contains("respondable"));
        assert!(inspected.contains("persistent"));
        assert!(inspected_card(&view, "inspect board 2 0").is_none());
    }
}
