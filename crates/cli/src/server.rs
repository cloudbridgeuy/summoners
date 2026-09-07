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
use summoners_match_log::{RecordedMatch, RecordedStep, RecordingError, SetRequirementV1};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::app::ServeArgs;
use crate::protocol::{
    ClientEnvelope, ProtocolError, Seat, ServerEnvelope, SubmissionResult, VERSION,
    normalize_action, player_view,
};
use crate::setup::{SetupError, initial_state};

const MAX_DECK_BYTES: u64 = 1024 * 1024;
const SHUFFLE_VERSION: &str = "std_rng_v1";
const MAX_CLIENT_LINE: usize = 64 * 1024;
const MAX_SERVER_LINE: usize = 1024 * 1024;
const CONNECTION_QUEUE: usize = 16;
const HANDSHAKE_LIMIT: usize = 4;
const CONNECTION_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

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
    #[error("client delivery failed: {0}")]
    Delivery(io::Error),
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
    let descriptions = crate::protocol::CardDescriptions::from_card_set(&state.cards);
    let recorder = start_recording(output.0, metadata, required_sets, state)?;
    println!(
        "Listening on {}; recording to {}",
        listener
            .local_addr()
            .map_err(|source| ServeError::Bind { address, source })?,
        output.1.display()
    );
    let runtime = tokio::runtime::Runtime::new().map_err(ServeError::Interrupt)?;
    runtime.block_on(serve_session(listener, recorder, &descriptions))
}

enum SessionEvent {
    Joined(Seat, mpsc::Sender<Outbound>, oneshot::Sender<Admission>),
    Submit(Seat, u64, u64, summoners_match_log::ActionV1),
    UnsupportedVersion(Seat, u64),
    Gone(Seat),
}

struct Outbound {
    envelope: ServerEnvelope,
    completed: oneshot::Sender<io::Result<()>>,
}

enum Admission {
    Accepted,
    Rejected,
}

async fn serve_session(
    listener: TcpListener,
    mut recorder: RecordedMatch<File>,
    descriptions: &crate::protocol::CardDescriptions,
) -> Result<(), ServeError> {
    listener
        .set_nonblocking(true)
        .map_err(ServeError::Interrupt)?;
    let listener = tokio::net::TcpListener::from_std(listener).map_err(ServeError::Interrupt)?;
    let (sender, mut receiver) = mpsc::channel(32);
    let permits = std::sync::Arc::new(Semaphore::new(HANDSHAKE_LIMIT));
    let mut clients: [Option<mpsc::Sender<Outbound>>; 2] = [None, None];
    let mut revision = 0_u64;
    let mut requests = [0_u64; 2];
    loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => { interrupted.map_err(ServeError::Interrupt)?; stop_all(&clients, "interrupted").await?; return Ok(()); }
            accepted = listener.accept() => {
                let (mut stream, _) = accepted.map_err(ServeError::Interrupt)?;
                if let Ok(permit) = permits.clone().try_acquire_owned() {
                    let sender = sender.clone();
                    tokio::spawn(async move { handshake(stream, sender, permit).await; });
                } else {
                    reject_stream(&mut stream, None, "server busy").await;
                }
            }
            event = receiver.recv() => match event {
                Some(SessionEvent::Joined(seat, client, admitted)) => {
                    let index = seat_index(seat);
                    if clients[index].is_some() || clients.iter().all(Option::is_some) {
                        let _ = deliver(&client, ServerEnvelope::Rejected { request_id: None, reason: "seat is occupied".to_string() }).await;
                        let _ = admitted.send(Admission::Rejected);
                    } else {
                        if let Err(error) = deliver(&client, ServerEnvelope::Waiting { seat }).await {
                            let _ = stop_all(&clients, "connection closed").await;
                            return Err(error);
                        }
                        clients[index] = Some(client);
                        let _ = admitted.send(Admission::Accepted);
                        if clients.iter().all(Option::is_some) {
                            broadcast(&clients, Broadcast { recorder: &recorder, descriptions, revision, reply: None, result: None }).await?;
                        }
                    }
                }
                Some(SessionEvent::Gone(seat)) => {
                    if clients[seat_index(seat)].is_some() {
                        stop_all(&clients, "connection closed").await?;
                        return Ok(());
                    }
                }
                Some(SessionEvent::UnsupportedVersion(seat, request_id)) => {
                    if let Some(client) = &clients[seat_index(seat)] {
                        deliver(client, rejection(request_id, "unsupported version")).await?;
                    }
                    stop_all(&clients, "protocol error").await?;
                    return Ok(());
                }
                Some(SessionEvent::Submit(seat, request_id, based_on_revision, action)) => {
                    let Some(client) = &clients[seat_index(seat)] else { continue; };
                    if !request_is_next(requests[seat_index(seat)], request_id) {
                        deliver(client, rejection(request_id, "request id must increase")).await?;
                        continue;
                    }
                    requests[seat_index(seat)] = request_id;
                    if !clients.iter().all(Option::is_some) {
                        deliver(client, rejection(request_id, "waiting for opponent")).await?;
                        continue;
                    }
                    let action = match normalize_action(seat.into(), action) {
                        Ok(action) => action,
                        Err(ProtocolError::Actor) => { deliver(client, rejection(request_id, "actor does not own this connection")).await?; continue; }
                        Err(ProtocolError::Conversion(reason)) => { deliver(client, rejection(request_id, &reason)).await?; continue; }
                    };
                    if !accepts_revision(revision, based_on_revision, &action, recorder.state().status) {
                        deliver(client, rejection(request_id, "stale revision")).await?;
                        continue;
                    }
                    let result = match recorder.submit(&action).map_err(ServeError::Recording)? {
                        RecordedStep::Accepted { .. } => SubmissionResult::Accepted,
                        RecordedStep::Rejected { error } => SubmissionResult::Rejected { reason: rejection_reason(error) },
                    };
                    revision += 1;
                    let terminal = matches!(recorder.state().status, GameStatus::Ended(_));
                    broadcast(&clients, Broadcast { recorder: &recorder, descriptions, revision, reply: Some(request_id), result: Some(result) }).await?;
                    if terminal { return Ok(()); }
                }
                None => return Ok(()),
            }
        }
    }
}

async fn handshake(
    mut stream: TcpStream,
    sender: mpsc::Sender<SessionEvent>,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    let frame =
        tokio::time::timeout(std::time::Duration::from_secs(5), read_frame(&mut stream)).await;
    let Ok(Ok(frame)) = frame else {
        return;
    };
    let join = serde_json::from_slice(&frame);
    let seat = match join_decision(&join) {
        Ok(seat) => seat,
        Err(reason) => {
            reject_stream(&mut stream, None, reason).await;
            return;
        }
    };
    let (outbound, mut output) = mpsc::channel::<Outbound>(CONNECTION_QUEUE);
    let (mut reader, mut writer) = stream.into_split();
    tokio::spawn(async move {
        while let Some(message) = output.recv().await {
            let result = write_server_frame(&mut writer, &message.envelope).await;
            let failed = result.is_err();
            let _ = message.completed.send(result);
            if failed {
                return;
            }
        }
    });
    let (admission, admitted) = oneshot::channel();
    if sender
        .send(SessionEvent::Joined(seat, outbound, admission))
        .await
        .is_err()
    {
        return;
    }
    if !matches!(admitted.await, Ok(Admission::Accepted)) {
        return;
    }
    drop(permit);
    loop {
        match read_frame(&mut reader).await {
            Ok(frame) => match serde_json::from_slice::<ClientEnvelope>(&frame) {
                Ok(ClientEnvelope::Submit {
                    version: VERSION,
                    request_id,
                    based_on_revision,
                    action,
                }) => {
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
                Ok(ClientEnvelope::Submit { request_id, .. }) => {
                    let _ = sender
                        .send(SessionEvent::UnsupportedVersion(seat, request_id))
                        .await;
                    return;
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
async fn write_server_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    envelope: &ServerEnvelope,
) -> io::Result<()> {
    write_server_frame_with_deadline(writer, envelope, CONNECTION_DEADLINE).await
}
async fn write_server_frame_with_deadline(
    writer: &mut (impl AsyncWrite + Unpin),
    envelope: &ServerEnvelope,
    deadline: std::time::Duration,
) -> io::Result<()> {
    let bytes = server_frame(envelope)?;
    tokio::time::timeout(deadline, writer.write_all(&bytes))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "write timeout"))?
}
fn server_frame(envelope: &ServerEnvelope) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(envelope).map_err(io::Error::other)?;
    if bytes.len() >= MAX_SERVER_LINE {
        return Err(io::Error::other("frame limit"));
    }
    bytes.push(b'\n');
    Ok(bytes)
}
async fn reject_stream(stream: &mut TcpStream, request_id: Option<u64>, reason: &str) {
    let envelope = ServerEnvelope::Rejected {
        request_id,
        reason: reason.to_string(),
    };
    if let Ok(mut bytes) = serde_json::to_vec(&envelope) {
        bytes.push(b'\n');
        let _ = tokio::time::timeout(CONNECTION_DEADLINE, stream.write_all(&bytes)).await;
        let _ = stream.shutdown().await;
    }
}
fn seat_index(seat: Seat) -> usize {
    match seat {
        Seat::One => 0,
        Seat::Two => 1,
    }
}
struct Broadcast<'a> {
    recorder: &'a RecordedMatch<File>,
    descriptions: &'a crate::protocol::CardDescriptions,
    revision: u64,
    reply: Option<u64>,
    result: Option<SubmissionResult>,
}

async fn broadcast(
    clients: &[Option<mpsc::Sender<Outbound>>; 2],
    input: Broadcast<'_>,
) -> Result<(), ServeError> {
    let mut failure = None;
    for seat in [Seat::One, Seat::Two] {
        if let Some(client) = &clients[seat_index(seat)] {
            let view = player_view(input.recorder.state(), seat.into(), input.descriptions);
            let envelope = if let Some(outcome) = view.outcome.clone() {
                ServerEnvelope::Finished {
                    outcome,
                    view,
                    notices: vec!["resignation".to_string()],
                    reply: input.reply,
                }
            } else {
                ServerEnvelope::Update {
                    revision: input.revision,
                    view,
                    notices: Vec::new(),
                    reply: input.reply,
                    result: input.result.clone(),
                }
            };
            if let Err(error) = deliver(client, envelope).await {
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}
async fn stop_all(
    clients: &[Option<mpsc::Sender<Outbound>>; 2],
    reason: &str,
) -> Result<(), ServeError> {
    let mut failure = None;
    for client in clients.iter().flatten() {
        if let Err(error) = deliver(
            client,
            ServerEnvelope::Stopped {
                reason: reason.to_string(),
            },
        )
        .await
        {
            failure.get_or_insert(error);
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn deliver(
    client: &mpsc::Sender<Outbound>,
    envelope: ServerEnvelope,
) -> Result<(), ServeError> {
    let (completed, received) = oneshot::channel();
    tokio::time::timeout(
        CONNECTION_DEADLINE,
        client.send(Outbound {
            envelope,
            completed,
        }),
    )
    .await
    .map_err(|_| ServeError::Delivery(io::Error::new(io::ErrorKind::TimedOut, "queue timeout")))?
    .map_err(|_| {
        ServeError::Delivery(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "connection closed",
        ))
    })?;
    received
        .await
        .map_err(|_| {
            ServeError::Delivery(io::Error::new(io::ErrorKind::BrokenPipe, "writer stopped"))
        })?
        .map_err(ServeError::Delivery)
}

fn rejection(request_id: u64, reason: &str) -> ServerEnvelope {
    ServerEnvelope::Rejected {
        request_id: Some(request_id),
        reason: reason.to_string(),
    }
}

fn rejection_reason(error: summoners_core::domain::errors::ActionError) -> String {
    use summoners_core::domain::errors::ActionError;
    match error {
        ActionError::NotYourDecision => "not your decision".to_string(),
        ActionError::GameAlreadyOver => "game already over".to_string(),
        ActionError::GameBroken => "game is broken".to_string(),
        ActionError::WrongPhase => "wrong phase".to_string(),
        ActionError::EmptyPosition => "empty position".to_string(),
        ActionError::UnknownCard => "unknown card".to_string(),
        ActionError::InsufficientMana { short } => format!(
            "insufficient mana: matter {} mind {} spirit {}",
            short.matter, short.mind, short.spirit
        ),
        ActionError::SummonExhausted => "summon exhausted".to_string(),
        ActionError::NormalAttackAlreadyUsed => "normal attack already used".to_string(),
        ActionError::NormalRetreatAlreadyUsed => "normal retreat already used".to_string(),
        ActionError::AlreadyUpgradedThisTurn => "already upgraded this turn".to_string(),
        ActionError::PlayedThisTurn => "played this turn".to_string(),
        ActionError::IllegalUpgradeTarget => "illegal upgrade target".to_string(),
        ActionError::InvalidTarget => "invalid target".to_string(),
        ActionError::PendingInputMismatch => "pending input mismatch".to_string(),
        ActionError::InvalidManaHint => "invalid mana hint".to_string(),
    }
}

fn request_is_next(last: u64, request: u64) -> bool {
    request > last
}

fn accepts_revision(
    revision: u64,
    based_on_revision: u64,
    action: &summoners_core::domain::actions::GameAction,
    status: GameStatus,
) -> bool {
    matches!(status, GameStatus::Playing)
        && (based_on_revision == revision
            || matches!(
                action,
                summoners_core::domain::actions::GameAction::Resign { .. }
            ))
}

fn join_decision(join: &Result<ClientEnvelope, serde_json::Error>) -> Result<Seat, &'static str> {
    match join {
        Ok(ClientEnvelope::Join {
            version: VERSION,
            seat,
        }) => Ok(*seat),
        Ok(ClientEnvelope::Join { .. }) => Err("unsupported version"),
        Ok(ClientEnvelope::Submit { version, .. }) if *version != VERSION => {
            Err("unsupported version")
        }
        Ok(ClientEnvelope::Submit { .. }) | Err(_) => Err("join required"),
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
    use summoners_core::domain::{
        actions::GameAction,
        ids::PlayerId,
        state::{GameOutcome, LossReason},
    };
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

    #[test]
    fn request_and_revision_rules_are_explicit() {
        let resign = GameAction::Resign {
            player: PlayerId::One,
        };
        let ordinary = GameAction::EndTurn {
            player: PlayerId::One,
        };
        assert!(request_is_next(3, 4));
        assert!(!request_is_next(3, 3));
        assert!(accepts_revision(7, 7, &ordinary, GameStatus::Playing));
        assert!(!accepts_revision(7, 6, &ordinary, GameStatus::Playing));
        assert!(accepts_revision(7, 6, &resign, GameStatus::Playing));
        assert!(!accepts_revision(
            7,
            7,
            &resign,
            GameStatus::Ended(GameOutcome {
                winner: PlayerId::Two,
                reason: LossReason::Resignation,
            })
        ));
        assert!(matches!(
            rejection(4, "no"),
            ServerEnvelope::Rejected {
                request_id: Some(4),
                reason
            } if reason == "no"
        ));
    }

    #[test]
    fn join_rules_distinguish_version_and_order() {
        assert_eq!(
            join_decision(&Ok(ClientEnvelope::Join {
                version: VERSION,
                seat: Seat::One
            })),
            Ok(Seat::One)
        );
        assert_eq!(
            join_decision(&Ok(ClientEnvelope::Join {
                version: VERSION + 1,
                seat: Seat::One
            })),
            Err("unsupported version")
        );
        let frame = serde_json::from_str(
            "{\"kind\":\"submit\",\"version\":1,\"request_id\":1,\"based_on_revision\":0,\"action\":{\"kind\":\"resign\",\"player\":\"one\"}}",
        );
        assert_eq!(join_decision(&frame), Err("join required"));
        assert_eq!(seat_index(Seat::One), 0);
        assert_eq!(seat_index(Seat::Two), 1);
    }

    #[tokio::test]
    async fn client_frame_limit_rejects_unterminated_input() {
        let (mut writer, mut reader) = tokio::io::duplex(MAX_CLIENT_LINE + 1);
        let writer = tokio::spawn(async move {
            writer
                .write_all(&vec![b'x'; MAX_CLIENT_LINE])
                .await
                .expect("frame writes");
        });
        assert_eq!(
            read_frame(&mut reader)
                .await
                .expect_err("frame limit")
                .kind(),
            io::ErrorKind::Other
        );
        writer.await.expect("writer joins");
    }

    #[tokio::test]
    async fn server_frame_limit_write_failure_and_timeout_propagate() {
        let large = ServerEnvelope::Rejected {
            request_id: None,
            reason: "x".repeat(MAX_SERVER_LINE),
        };
        let (mut bound_writer, _) = tokio::io::duplex(1);
        assert_eq!(
            write_server_frame(&mut bound_writer, &large)
                .await
                .expect_err("frame limit")
                .to_string(),
            "frame limit"
        );
        let base = serde_json::to_vec(&ServerEnvelope::Rejected {
            request_id: None,
            reason: String::new(),
        })
        .expect("base frame");
        let exact = ServerEnvelope::Rejected {
            request_id: None,
            reason: "x".repeat(MAX_SERVER_LINE - base.len() - 1),
        };
        assert_eq!(
            server_frame(&exact).expect("exact frame").len(),
            MAX_SERVER_LINE
        );

        let failed = ServerEnvelope::Rejected {
            request_id: None,
            reason: "failed".to_string(),
        };
        let (mut closed_writer, closed_reader) = tokio::io::duplex(1);
        drop(closed_reader);
        assert!(
            write_server_frame(&mut closed_writer, &failed)
                .await
                .is_err()
        );

        let timeout = ServerEnvelope::Rejected {
            request_id: None,
            reason: "x".repeat(64),
        };
        let (mut blocked_writer, _blocked_reader) = tokio::io::duplex(1);
        assert_eq!(
            write_server_frame_with_deadline(
                &mut blocked_writer,
                &timeout,
                std::time::Duration::from_millis(1)
            )
            .await
            .expect_err("write timeout")
            .kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[tokio::test]
    async fn terminal_recording_completes_before_both_finished_deliveries_acknowledge() {
        let catalog = built_in_catalog().expect("catalog");
        let state = initial_state(
            catalog.library(),
            [catalog.set_paths(), catalog.barrow_herd()],
            1,
        )
        .expect("state");
        let descriptions = crate::protocol::CardDescriptions::from_card_set(&state.cards);
        let directory = TempDir::new().expect("directory");
        let transcript = directory.path().join("match.ndjson");
        let mut recorder = start_recording(
            File::create(&transcript).expect("transcript"),
            BTreeMap::new(),
            required_sets(catalog.library()).expect("requirements"),
            state,
        )
        .expect("recorder");
        recorder
            .submit(&GameAction::Resign {
                player: PlayerId::One,
            })
            .expect("resignation");
        let (one, mut one_messages) = mpsc::channel::<Outbound>(1);
        let (two, mut two_messages) = mpsc::channel::<Outbound>(1);
        let clients = [Some(one), Some(two)];
        let observe = async {
            let first = one_messages.recv().await.expect("first finished");
            assert!(matches!(first.envelope, ServerEnvelope::Finished { .. }));
            first.completed.send(Ok(())).expect("first acknowledges");
            let second = two_messages.recv().await.expect("second finished");
            assert!(matches!(second.envelope, ServerEnvelope::Finished { .. }));
            assert!(
                std::fs::read_to_string(&transcript)
                    .expect("transcript reads")
                    .contains("\"record\":\"match_completed\"")
            );
            second
                .completed
                .send(Err(io::Error::other("write failed")))
                .expect("second fails");
        };
        let (delivery, ()) = tokio::join!(
            broadcast(
                &clients,
                Broadcast {
                    recorder: &recorder,
                    descriptions: &descriptions,
                    revision: 1,
                    reply: Some(1),
                    result: Some(SubmissionResult::Accepted),
                }
            ),
            observe
        );
        assert!(matches!(delivery, Err(ServeError::Delivery(_))));
    }
}
