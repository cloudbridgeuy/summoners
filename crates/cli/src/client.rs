use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::PlayArgs;
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
    let revision = Arc::new(Mutex::new(0_u64));
    let reader = stream.try_clone().map_err(PlayError::Socket)?;
    let observed = Arc::clone(&revision);
    let (terminal_sender, terminal_receiver) = std::sync::mpsc::channel();
    let listener = std::thread::spawn(move || receive(reader, &observed, &terminal_sender));
    let (input_sender, input_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            if input_sender.send(line).is_err() {
                return;
            }
        }
    });
    let mut request_id = 0_u64;
    while matches!(
        terminal_receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ) {
        let line = match input_receiver.recv_timeout(POLL_INTERVAL) {
            Ok(line) => line.map_err(PlayError::Socket)?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if line.trim().eq_ignore_ascii_case("give up") {
            println!("Confirm Give up with yes");
            match confirmed(&input_receiver, &terminal_receiver)? {
                Confirmation::Confirmed => {
                    request_id += 1;
                    let revision = *revision
                        .lock()
                        .map_err(|_| PlayError::Socket(io::Error::other("revision lock")))?;
                    let player = if seat == Seat::One {
                        PlayerIdV1::One
                    } else {
                        PlayerIdV1::Two
                    };
                    send(
                        &mut stream,
                        &ClientEnvelope::Submit {
                            version: VERSION,
                            request_id,
                            based_on_revision: revision,
                            action: ActionV1::Resign { player },
                        },
                    )?;
                }
                Confirmation::Cancelled => {}
                Confirmation::Terminal => break,
            }
        }
    }
    drop(stream);
    let _ = listener.join();
    Ok(())
}

enum Confirmation {
    Confirmed,
    Cancelled,
    Terminal,
}

fn confirmed(
    input: &std::sync::mpsc::Receiver<Result<String, io::Error>>,
    terminal: &std::sync::mpsc::Receiver<()>,
) -> Result<Confirmation, PlayError> {
    loop {
        match terminal.try_recv() {
            Ok(()) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Ok(Confirmation::Terminal);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        match input.recv_timeout(POLL_INTERVAL) {
            Ok(Ok(answer)) => {
                return Ok(if answer.trim().eq_ignore_ascii_case("yes") {
                    Confirmation::Confirmed
                } else {
                    Confirmation::Cancelled
                });
            }
            Ok(Err(error)) => return Err(PlayError::Socket(error)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Ok(Confirmation::Cancelled);
            }
        }
    }
}

fn send(stream: &mut TcpStream, message: &ClientEnvelope) -> Result<(), PlayError> {
    let mut bytes = serde_json::to_vec(message).map_err(PlayError::Encode)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(PlayError::Socket)
}

fn receive(stream: TcpStream, revision: &Arc<Mutex<u64>>, terminal: &std::sync::mpsc::Sender<()>) {
    let mut reader = BufReader::new(stream);
    while let Ok(frame) = read_frame(&mut reader) {
        let Ok(message) = serde_json::from_slice::<ServerEnvelope>(&frame) else {
            return;
        };
        match message {
            ServerEnvelope::Update {
                revision: next,
                view,
                ..
            } => {
                if let Ok(mut current) = revision.lock() {
                    *current = next;
                }
                println!("{}", render_view(&view));
            }
            ServerEnvelope::Finished { outcome, view, .. } => {
                println!("{}", render_view(&view));
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
        BoardView, CardDescription, ManaView, PhaseView, PlayerPublicView, ReadinessView,
        SeatsView, SummonView,
    };

    fn view() -> PlayerView {
        let card = CardDescription {
            name: "Whelp".to_string(),
            life: Some(40),
            retreat_cost: Some(1),
            mana_types: vec!["Matter".to_string()],
            cost: None,
            abilities: Vec::new(),
            effects: vec!["DealDamage".to_string()],
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
    fn confirmation_cancel_keeps_the_client_running() {
        let (input_sender, input) = std::sync::mpsc::channel();
        let (_terminal_sender, terminal) = std::sync::mpsc::channel();
        input_sender.send(Ok("no".to_string())).expect("send");
        assert!(matches!(
            confirmed(&input, &terminal).expect("confirmation"),
            Confirmation::Cancelled
        ));
    }

    #[test]
    fn confirmation_observes_terminal_message() {
        let (_input_sender, input) = std::sync::mpsc::channel();
        let (terminal_sender, terminal) = std::sync::mpsc::channel();
        terminal_sender.send(()).expect("send");
        assert!(matches!(
            confirmed(&input, &terminal).expect("confirmation"),
            Confirmation::Terminal
        ));
    }

    #[test]
    fn confirmation_observes_terminal_disconnect() {
        let (_input_sender, input) = std::sync::mpsc::channel();
        let (terminal_sender, terminal) = std::sync::mpsc::channel();
        drop(terminal_sender);
        assert!(matches!(
            confirmed(&input, &terminal).expect("confirmation"),
            Confirmation::Terminal
        ));
    }
}
