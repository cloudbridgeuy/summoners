use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngExt;
use serde_json::json;
use summoners_cards::{BuiltInError, DeckLoadError, built_in_catalog, parse_deck};
use summoners_match_log::{RecordedMatch, RecordingError, SetRequirementV1};

use crate::app::ServeArgs;
use crate::setup::{SetupError, initial_state};

const MAX_DECK_BYTES: u64 = 1024 * 1024;
const SHUFFLE_VERSION: &str = "std_rng_v1";

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
    wait_for_interrupt(recorder)
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

fn wait_for_interrupt(recorder: RecordedMatch<File>) -> Result<(), ServeError> {
    let runtime = tokio::runtime::Runtime::new().map_err(ServeError::Interrupt)?;
    runtime
        .block_on(tokio::signal::ctrl_c())
        .map_err(ServeError::Interrupt)?;
    drop(recorder);
    Ok(())
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
