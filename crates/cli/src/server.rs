use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::RngExt;
use serde_json::json;
use summoners_cards::{BuiltInError, DeckLoadError, built_in_catalog, parse_deck};
use summoners_core::domain::state::GameStatus;
use summoners_match_log::{RecordedMatch, RecordingError, SetRequirementV1};

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
    serve_session(listener, recorder)
}

struct Client {
    seat: Seat,
    stream: TcpStream,
}

fn serve_session(
    listener: TcpListener,
    mut recorder: RecordedMatch<File>,
) -> Result<(), ServeError> {
    listener
        .set_nonblocking(true)
        .map_err(ServeError::Interrupt)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut clients = Vec::new();
    while clients.len() < 2 {
        if std::time::Instant::now() >= deadline {
            return Ok(());
        }
        match listener.accept() {
            Ok((mut stream, _)) => match read_join(&mut stream) {
                Ok(seat) if clients.iter().all(|client: &Client| client.seat != seat) => {
                    write_envelope(&mut stream, &ServerEnvelope::Waiting { seat })
                        .map_err(ServeError::Interrupt)?;
                    clients.push(Client { seat, stream });
                }
                Ok(_) => {
                    let _ = write_envelope(
                        &mut stream,
                        &ServerEnvelope::Rejected {
                            request_id: None,
                            reason: "seat is occupied".to_string(),
                        },
                    );
                }
                Err(reason) => {
                    let _ = write_envelope(
                        &mut stream,
                        &ServerEnvelope::Rejected {
                            request_id: None,
                            reason,
                        },
                    );
                }
            },
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(source) => {
                return Err(ServeError::Bind {
                    address: listener.local_addr().map_err(ServeError::Interrupt)?,
                    source,
                });
            }
        }
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(16);
    for client in &clients {
        let stream = client.stream.try_clone().map_err(ServeError::Interrupt)?;
        let sender = sender.clone();
        let seat = client.seat;
        std::thread::spawn(move || read_frames(stream, seat, sender));
    }
    drop(sender);
    let mut revision = 0_u64;
    broadcast(&mut clients, &recorder, revision, None, false)?;
    loop {
        let Ok((seat, item)) = receiver.recv() else {
            stop_clients(&mut clients, "connection closed")?;
            return Ok(());
        };
        let ClientEnvelope::Submit {
            request_id,
            based_on_revision,
            action,
        } = item
        else {
            stop_clients(&mut clients, "protocol violation")?;
            return Ok(());
        };
        let normalized = normalize_action(seat.into(), action);
        let action = match normalized {
            Ok(action) => action,
            Err(ProtocolError::Actor) => {
                reject(
                    client_for(&mut clients, seat),
                    Some(request_id),
                    "actor does not own this connection",
                )?;
                continue;
            }
            Err(ProtocolError::Conversion(error)) => {
                reject(client_for(&mut clients, seat), Some(request_id), &error)?;
                continue;
            }
        };
        if based_on_revision != revision
            && !matches!(
                action,
                summoners_core::domain::actions::GameAction::Resign { .. }
            )
        {
            reject(
                client_for(&mut clients, seat),
                Some(request_id),
                "stale revision",
            )?;
            continue;
        }
        let result = recorder.submit(&action).map_err(ServeError::Recording)?;
        revision += 1;
        let terminal = matches!(recorder.state().status, GameStatus::Ended(_));
        let reply = Some(request_id);
        let _ = result;
        broadcast(&mut clients, &recorder, revision, reply, terminal)?;
        if terminal {
            return Ok(());
        }
    }
}

fn read_join(stream: &mut TcpStream) -> Result<Seat, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|error| error.to_string())?);
    let frame = read_frame(&mut reader)?;
    match serde_json::from_slice::<ClientEnvelope>(&frame).map_err(|error| error.to_string())? {
        ClientEnvelope::Join {
            version: VERSION,
            seat,
        } => Ok(seat),
        ClientEnvelope::Join { .. } => Err("unsupported version".to_string()),
        ClientEnvelope::Submit { .. } => Err("join required".to_string()),
    }
}
fn read_frames(
    stream: TcpStream,
    seat: Seat,
    sender: std::sync::mpsc::SyncSender<(Seat, ClientEnvelope)>,
) {
    let mut reader = BufReader::new(stream);
    loop {
        let Ok(frame) = read_frame(&mut reader) else {
            return;
        };
        let Ok(envelope) = serde_json::from_slice::<ClientEnvelope>(&frame) else {
            return;
        };
        if sender.send((seat, envelope)).is_err() {
            return;
        }
    }
}
fn read_frame(reader: &mut impl BufRead) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(256);
    let size = reader
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if size == 0 {
        return Err("connection closed".to_string());
    }
    if bytes.len() > MAX_CLIENT_LINE || !bytes.ends_with(b"\n") {
        return Err("invalid frame".to_string());
    }
    bytes.pop();
    Ok(bytes)
}
fn write_envelope(stream: &mut TcpStream, envelope: &ServerEnvelope) -> io::Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut bytes = serde_json::to_vec(envelope).map_err(io::Error::other)?;
    if bytes.len() > MAX_SERVER_LINE {
        return Err(io::Error::other("server frame exceeds limit"));
    }
    bytes.push(b'\n');
    stream.write_all(&bytes)
}
fn client_for(clients: &mut [Client], seat: Seat) -> &mut TcpStream {
    let client = clients
        .iter_mut()
        .find(|client| client.seat == seat)
        .unwrap_or_else(|| unreachable!());
    &mut client.stream
}
fn reject(stream: &mut TcpStream, request_id: Option<u64>, reason: &str) -> Result<(), ServeError> {
    write_envelope(
        stream,
        &ServerEnvelope::Rejected {
            request_id,
            reason: reason.to_string(),
        },
    )
    .map_err(ServeError::Interrupt)
}
fn broadcast(
    clients: &mut [Client],
    recorder: &RecordedMatch<File>,
    revision: u64,
    reply: Option<u64>,
    terminal: bool,
) -> Result<(), ServeError> {
    for client in clients {
        let view = player_view(recorder.state(), client.seat.into());
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
        write_envelope(&mut client.stream, &envelope).map_err(ServeError::Interrupt)?;
    }
    Ok(())
}
fn stop_clients(clients: &mut [Client], reason: &str) -> Result<(), ServeError> {
    for client in clients {
        write_envelope(
            &mut client.stream,
            &ServerEnvelope::Stopped {
                reason: reason.to_string(),
            },
        )
        .map_err(ServeError::Interrupt)?;
    }
    Ok(())
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
