use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngExt;
use serde_json::json;
use summoners_cards::{BuiltInError, DeckLoadError, built_in_catalog, parse_deck};
use summoners_core::domain::state::GameStatus;
use summoners_match_log::{RecordedMatch, RecordingError, SetRequirementV1};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc};

use crate::app::ServeArgs;
use crate::protocol::{
    ClientEnvelope, ProtocolError, Seat, ServerEnvelope, VERSION, normalize_action, player_view,
};
use crate::setup::{SetupError, initial_state};

const MAX_DECK_BYTES: u64 = 1024 * 1024;
const SHUFFLE_VERSION: &str = "std_rng_v1";
const MAX_CLIENT_LINE: usize = 64 * 1024;
const MAX_SERVER_LINE: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("cannot read deck {path}: {source}")]
    ReadDeck { path: PathBuf, source: io::Error },
    #[error("deck {path} exceeds 1 MiB")]
    DeckTooLarge { path: PathBuf },
    #[error("deck {path} is invalid: {source}")]
    ParseDeck {
        path: PathBuf,
        source: DeckLoadError,
    },
    #[error("built-in library failed to load: {0}")]
    Catalog(BuiltInError),
    #[error("the opening state failed: {0}")]
    Setup(SetupError),
    #[error("the required foundations Set is not loaded")]
    MissingFoundations,
    #[error("cannot bind {address}: {source}")]
    Bind {
        address: SocketAddr,
        source: io::Error,
    },
    #[error("cannot create output {path}: {source}")]
    Output { path: PathBuf, source: io::Error },
    #[error("cannot start recording: {0}")]
    Recording(RecordingError),
    #[error("cannot wait for interruption: {0}")]
    Interrupt(io::Error),
}

pub fn serve(args: &ServeArgs) -> Result<(), ServeError> {
    let catalog = built_in_catalog().map_err(ServeError::Catalog)?;
    let decks = load_decks(&args.decks, catalog.library())?;
    let seed = args.seed.unwrap_or_else(|| rand::rng().random());
    let state = initial_state(catalog.library(), [&decks[0], &decks[1]], seed)
        .map_err(ServeError::Setup)?;
    let address = SocketAddr::new(args.bind, args.port.unwrap_or(0));
    let listener =
        TcpListener::bind(address).map_err(|source| ServeError::Bind { address, source })?;
    let output = create_output(args.output.as_deref())?;
    let required_sets = required_sets(catalog.library())?;
    let mut metadata = BTreeMap::new();
    metadata.insert("seed".to_string(), json!(seed));
    metadata.insert("shuffle_version".to_string(), json!(SHUFFLE_VERSION));
    let recorder = start_recording(output.0, metadata, required_sets, state)?;
    println!(
        "Listening on {}; recording to {}",
        listener
            .local_addr()
            .map_err(|source| ServeError::Bind { address, source })?,
        output.1.display()
    );
    let runtime = tokio::runtime::Runtime::new().map_err(ServeError::Interrupt)?;
    runtime.block_on(serve_session(listener, recorder))
}

enum SessionEvent {
    Joined(Seat, mpsc::Sender<ServerEnvelope>),
    Submit(Seat, u64, u64, summoners_match_log::ActionV1),
    Gone(Seat),
}

async fn serve_session(
    listener: TcpListener,
    mut recorder: RecordedMatch<File>,
) -> Result<(), ServeError> {
    listener
        .set_nonblocking(true)
        .map_err(ServeError::Interrupt)?;
    let listener = tokio::net::TcpListener::from_std(listener).map_err(ServeError::Interrupt)?;
    let (sender, mut receiver) = mpsc::channel(32);
    let permits = std::sync::Arc::new(Semaphore::new(4));
    let mut clients: [Option<mpsc::Sender<ServerEnvelope>>; 2] = [None, None];
    let mut revision = 0_u64;
    loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => { interrupted.map_err(ServeError::Interrupt)?; stop_all(&clients, "interrupted").await; return Ok(()); }
            accepted = listener.accept() => { let (stream, _) = accepted.map_err(ServeError::Interrupt)?; if let Ok(permit) = permits.clone().try_acquire_owned() { let sender = sender.clone(); tokio::spawn(async move { let _permit = permit; handshake(stream, sender).await; }); } }
            event = receiver.recv() => match event {
                Some(SessionEvent::Joined(seat, client)) => { let index = seat_index(seat); if clients[index].is_some() || clients.iter().all(Option::is_some) { let _ = client.send(ServerEnvelope::Rejected { request_id: None, reason: "seat is occupied".to_string() }).await; } else { let _ = client.send(ServerEnvelope::Waiting { seat }).await; clients[index] = Some(client); if clients.iter().all(Option::is_some) { broadcast(&clients, &recorder, revision, None, false).await; } } }
                Some(SessionEvent::Gone(seat)) => { if clients[seat_index(seat)].is_some() { stop_all(&clients, "connection closed").await; return Ok(()); } }
                Some(SessionEvent::Submit(seat, request_id, based_on_revision, action)) => { let Some(client) = &clients[seat_index(seat)] else { continue; }; let action = match normalize_action(seat.into(), action) { Ok(action) => action, Err(ProtocolError::Actor) => { let _ = client.send(ServerEnvelope::Rejected { request_id: Some(request_id), reason: "actor does not own this connection".to_string() }).await; continue; }, Err(ProtocolError::Conversion(reason)) => { let _ = client.send(ServerEnvelope::Rejected { request_id: Some(request_id), reason }).await; continue; } }; if based_on_revision != revision && !matches!(action, summoners_core::domain::actions::GameAction::Resign { .. }) { let _ = client.send(ServerEnvelope::Rejected { request_id: Some(request_id), reason: "stale revision".to_string() }).await; continue; } recorder.submit(&action).map_err(ServeError::Recording)?; revision += 1; let terminal = matches!(recorder.state().status, GameStatus::Ended(_)); broadcast(&clients, &recorder, revision, Some(request_id), terminal).await; if terminal { return Ok(()); } }
                None => return Ok(()),
            }
        }
    }
}

async fn handshake(mut stream: TcpStream, sender: mpsc::Sender<SessionEvent>) {
    let frame =
        tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(&mut stream)).await;
    let Ok(Ok(frame)) = frame else {
        return;
    };
    let Ok(ClientEnvelope::Join {
        version: VERSION,
        seat,
    }) = serde_json::from_slice(&frame)
    else {
        reject_stream(&mut stream, None, "join required").await;
        return;
    };
    let (outbound, mut output) = mpsc::channel(16);
    let (mut reader, mut writer) = stream.into_split();
    tokio::spawn(async move {
        while let Some(envelope) = output.recv().await {
            let Ok(mut bytes) = serde_json::to_vec(&envelope) else {
                return;
            };
            if bytes.len() > MAX_SERVER_LINE {
                return;
            }
            bytes.push(b'\n');
            if tokio::time::timeout(std::time::Duration::from_secs(5), writer.write_all(&bytes))
                .await
                .is_err()
            {
                return;
            }
        }
    });
    if sender
        .send(SessionEvent::Joined(seat, outbound))
        .await
        .is_err()
    {
        return;
    }
    let mut last_request = 0_u64;
    loop {
        match read_frame(&mut reader).await {
            Ok(frame) => match serde_json::from_slice::<ClientEnvelope>(&frame) {
                Ok(ClientEnvelope::Submit {
                    request_id,
                    based_on_revision,
                    action,
                }) if request_id > last_request => {
                    last_request = request_id;
                    if sender
                        .send(SessionEvent::Submit(
                            seat,
                            request_id,
                            based_on_revision,
                            action,
                        ))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                _ => {
                    let _ = sender.send(SessionEvent::Gone(seat)).await;
                    return;
                }
            },
            Err(_) => {
                let _ = sender.send(SessionEvent::Gone(seat)).await;
                return;
            }
        }
    }
}
async fn read_frame(reader: &mut (impl AsyncReadExt + Unpin)) -> Result<Vec<u8>, io::Error> {
    let mut bytes = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    loop {
        if bytes.len() == MAX_CLIENT_LINE {
            return Err(io::Error::other("frame limit"));
        }
        if reader.read_exact(&mut byte).await.is_err() {
            return Err(io::Error::other("closed"));
        }
        if byte[0] == b'\n' {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
}
async fn reject_stream(stream: &mut TcpStream, request_id: Option<u64>, reason: &str) {
    let envelope = ServerEnvelope::Rejected {
        request_id,
        reason: reason.to_string(),
    };
    if let Ok(mut bytes) = serde_json::to_vec(&envelope) {
        bytes.push(b'\n');
        let _ =
            tokio::time::timeout(std::time::Duration::from_secs(5), stream.write_all(&bytes)).await;
    }
}
fn seat_index(seat: Seat) -> usize {
    match seat {
        Seat::One => 0,
        Seat::Two => 1,
    }
}
async fn broadcast(
    clients: &[Option<mpsc::Sender<ServerEnvelope>>; 2],
    recorder: &RecordedMatch<File>,
    revision: u64,
    reply: Option<u64>,
    terminal: bool,
) {
    for seat in [Seat::One, Seat::Two] {
        if let Some(client) = &clients[seat_index(seat)] {
            let view = player_view(recorder.state(), seat.into());
            let envelope = if terminal {
                ServerEnvelope::Finished {
                    outcome: view.outcome.clone().unwrap_or(serde_json::Value::Null),
                    view,
                    notices: vec!["resignation".to_string()],
                    reply,
                }
            } else {
                ServerEnvelope::Update {
                    revision,
                    view,
                    notices: Vec::new(),
                    reply,
                }
            };
            let _ = client.send(envelope).await;
        }
    }
}
async fn stop_all(clients: &[Option<mpsc::Sender<ServerEnvelope>>; 2], reason: &str) {
    for client in clients.iter().flatten() {
        let _ = client
            .send(ServerEnvelope::Stopped {
                reason: reason.to_string(),
            })
            .await;
    }
}

fn load_decks(
    paths: &[PathBuf],
    library: &summoners_cards::CardLibrary,
) -> Result<[summoners_cards::Deck; 2], ServeError> {
    let [one, two] = paths else { unreachable!() };
    Ok([load_deck(one, library)?, load_deck(two, library)?])
}

fn load_deck(
    path: &Path,
    library: &summoners_cards::CardLibrary,
) -> Result<summoners_cards::Deck, ServeError> {
    let bytes = read_deck(path)?;
    parse_deck(&bytes, library).map_err(|source| ServeError::ParseDeck {
        path: path.to_path_buf(),
        source,
    })
}

fn read_deck(path: &Path) -> Result<Vec<u8>, ServeError> {
    let metadata = fs::metadata(path).map_err(|source| ServeError::ReadDeck {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_DECK_BYTES {
        return Err(ServeError::DeckTooLarge {
            path: path.to_path_buf(),
        });
    }
    let mut file = File::open(path).map_err(|source| ServeError::ReadDeck {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(MAX_DECK_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| ServeError::ReadDeck {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() as u64 > MAX_DECK_BYTES {
        return Err(ServeError::DeckTooLarge {
            path: path.to_path_buf(),
        });
    }
    Ok(bytes)
}

fn required_sets(
    library: &summoners_cards::CardLibrary,
) -> Result<Vec<SetRequirementV1>, ServeError> {
    let revision = library
        .set_revision("foundations")
        .ok_or(ServeError::MissingFoundations)?;
    Ok(vec![SetRequirementV1 {
        set: "foundations".to_string(),
        revision,
    }])
}

fn start_recording<W: Write>(
    writer: W,
    metadata: BTreeMap<String, serde_json::Value>,
    required_sets: Vec<SetRequirementV1>,
    state: summoners_core::domain::state::GameState,
) -> Result<RecordedMatch<W>, ServeError> {
    RecordedMatch::start(writer, metadata, required_sets, state).map_err(ServeError::Recording)
}

fn create_output(explicit: Option<&Path>) -> Result<(File, PathBuf), ServeError> {
    match explicit {
        Some(path) => create_explicit_output(path),
        None => create_default_output(),
    }
}

fn create_explicit_output(path: &Path) -> Result<(File, PathBuf), ServeError> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| ServeError::Output {
            path: path.to_path_buf(),
            source,
        })?;
    Ok((file, path.to_path_buf()))
}

fn create_default_output() -> Result<(File, PathBuf), ServeError> {
    let directory = Path::new("matches");
    fs::create_dir_all(directory).map_err(|source| ServeError::Output {
        path: directory.to_path_buf(),
        source,
    })?;
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| ServeError::Output {
            path: directory.to_path_buf(),
            source: io::Error::other(error),
        })?
        .as_secs();
    create_default_output_at(directory, seconds)
}

fn create_default_output_at(directory: &Path, seconds: u64) -> Result<(File, PathBuf), ServeError> {
    for suffix in 0..1000_u16 {
        let path = directory.join(format!("{seconds}-{suffix}.ndjson"));
        match create_explicit_output(&path) {
            Ok(output) => return Ok(output),
            Err(ServeError::Output { source, .. })
                if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(ServeError::Output {
        path: directory.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "no unique transcript output name",
        ),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use tempfile::TempDir;

    struct CheckpointFailure;

    impl Write for CheckpointFailure {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("checkpoint"))
        }
    }

    #[test]
    fn bounded_deck_read_rejects_oversize_input() {
        let directory = TempDir::new().expect("directory");
        let path = directory.path().join("big.deck");
        fs::write(&path, vec![0; MAX_DECK_BYTES as usize + 1]).expect("deck");
        assert!(matches!(
            read_deck(&path),
            Err(ServeError::DeckTooLarge { .. })
        ));
    }

    #[test]
    fn both_deck_positions_are_read_and_parsed() {
        let directory = TempDir::new().expect("directory");
        let one = directory.path().join("one.toml");
        let two = directory.path().join("two.toml");
        fs::write(&one, include_bytes!("../../cards/data/set-paths.toml")).expect("one");
        fs::write(&two, include_bytes!("../../cards/data/barrow-herd.toml")).expect("two");
        let catalog = built_in_catalog().expect("catalog");
        let decks = load_decks(&[one, two], catalog.library()).expect("decks");
        assert_eq!(decks[0].id(), "set-paths");
        assert_eq!(decks[1].id(), "barrow-herd");
    }

    #[test]
    fn invalid_deck_fails_in_each_position() {
        let directory = TempDir::new().expect("directory");
        let valid = directory.path().join("valid.toml");
        let invalid = directory.path().join("invalid.toml");
        fs::write(&valid, include_bytes!("../../cards/data/set-paths.toml")).expect("valid");
        fs::write(&invalid, "not a deck").expect("invalid");
        let catalog = built_in_catalog().expect("catalog");
        for paths in [
            [invalid.clone(), valid.clone()],
            [valid.clone(), invalid.clone()],
        ] {
            assert!(matches!(
                load_decks(&paths, catalog.library()),
                Err(ServeError::ParseDeck { path, .. }) if path == invalid
            ));
        }
    }

    #[test]
    fn explicit_output_never_overwrites() {
        let directory = TempDir::new().expect("directory");
        let path = directory.path().join("match.ndjson");
        fs::write(&path, "existing").expect("output");
        assert!(
            matches!(create_explicit_output(&path), Err(ServeError::Output { source, .. }) if source.kind() == io::ErrorKind::AlreadyExists)
        );
        assert_eq!(fs::read_to_string(path).expect("read"), "existing");
    }

    #[test]
    fn explicit_output_requires_its_parent() {
        let directory = TempDir::new().expect("directory");
        let path = directory.path().join("missing/match.ndjson");
        assert!(
            matches!(create_explicit_output(&path), Err(ServeError::Output { source, .. }) if source.kind() == io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn default_output_collision_uses_the_next_exclusive_name() {
        let directory = TempDir::new().expect("directory");
        let occupied = directory.path().join("10-0.ndjson");
        fs::write(&occupied, "existing").expect("occupied");
        let (_, path) = create_default_output_at(directory.path(), 10).expect("output");
        assert_eq!(path, directory.path().join("10-1.ndjson"));
        assert_eq!(fs::read_to_string(occupied).expect("existing"), "existing");
    }

    #[test]
    fn recorder_checkpoint_failure_prevents_startup() {
        let catalog = built_in_catalog().expect("catalog");
        let state = initial_state(
            catalog.library(),
            [catalog.set_paths(), catalog.barrow_herd()],
            1,
        )
        .expect("state");
        let result = start_recording(
            CheckpointFailure,
            BTreeMap::new(),
            required_sets(catalog.library()).expect("requirements"),
            state,
        );
        let Err(error) = result else {
            panic!("checkpoint failure")
        };
        assert!(matches!(
            error,
            ServeError::Recording(RecordingError::Flush(_))
        ));
    }
}
