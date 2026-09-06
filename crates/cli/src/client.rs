use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use crate::app::PlayArgs;
use crate::protocol::{ClientEnvelope, Seat, ServerEnvelope, VERSION};
use summoners_match_log::{ActionV1, wire::PlayerIdV1};

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
    let listener = std::thread::spawn(move || receive(reader, observed));
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(line) = lines.next() {
        let line = line.map_err(PlayError::Socket)?;
        if line.trim().eq_ignore_ascii_case("give up") {
            println!("Confirm Give up with yes");
            let Some(answer) = lines.next() else {
                break;
            };
            if answer
                .map_err(PlayError::Socket)?
                .trim()
                .eq_ignore_ascii_case("yes")
            {
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
                        request_id: 1,
                        based_on_revision: revision,
                        action: ActionV1::Resign { player },
                    },
                )?;
                break;
            }
        }
    }
    let _ = listener.join();
    Ok(())
}

fn send(stream: &mut TcpStream, message: &ClientEnvelope) -> Result<(), PlayError> {
    let mut bytes = serde_json::to_vec(message).map_err(PlayError::Encode)?;
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(PlayError::Socket)
}
fn receive(stream: TcpStream, revision: Arc<Mutex<u64>>) {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        let Ok(size) = reader.read_line(&mut line) else {
            return;
        };
        if size == 0 {
            return;
        }
        let Ok(message) = serde_json::from_str::<ServerEnvelope>(&line) else {
            return;
        };
        match message {
            ServerEnvelope::Update { revision: next, .. } => {
                if let Ok(mut current) = revision.lock() {
                    *current = next;
                }
                println!("Update {next}");
            }
            ServerEnvelope::Finished { outcome, .. } => {
                println!("Finished {outcome}");
                return;
            }
            ServerEnvelope::Waiting { .. } => println!("Waiting"),
            ServerEnvelope::Rejected { reason, .. } => println!("Rejected {reason}"),
            ServerEnvelope::Stopped { reason } => {
                println!("Stopped {reason}");
                return;
            }
        }
    }
}
