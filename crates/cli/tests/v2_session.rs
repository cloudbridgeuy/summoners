#![allow(clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(8);

struct Process {
    child: Child,
}

impl Process {
    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(status) = self.child.try_wait().expect("process state is readable") {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "process did not exit within {WAIT:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Host {
    process: Process,
    port: u16,
    transcript: PathBuf,
}

struct RawClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

struct Client {
    process: Process,
    input: ChildStdin,
    output: PathBuf,
}

impl RawClient {
    fn connect(port: u16) -> Self {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&address, WAIT).expect("server accepts connection");
        stream
            .set_read_timeout(Some(WAIT))
            .expect("read timeout sets");
        let reader = BufReader::new(stream.try_clone().expect("stream clones"));
        Self {
            writer: stream,
            reader,
        }
    }

    fn send(&mut self, message: &Value) {
        serde_json::to_writer(&mut self.writer, &message).expect("message encodes");
        self.writer.write_all(b"\n").expect("message writes");
        self.writer.flush().expect("message flushes");
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("bytes write");
        self.writer.flush().expect("bytes flush");
    }

    fn receive(&mut self) -> Value {
        let mut line = String::new();
        let size = self
            .reader
            .read_line(&mut line)
            .expect("server response reads");
        assert_ne!(size, 0, "server closed before response");
        serde_json::from_str(&line).expect("server response is JSON")
    }

    fn closes(&mut self) {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(size) => assert_eq!(size, 0, "server closes rejected connection"),
            Err(error) => assert!(
                matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ),
                "rejected connection closes: {error}"
            ),
        }
    }
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_summoners")
}

fn repository_deck() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cards/data/set-paths.toml")
}

fn start_host(directory: &TempDir) -> Host {
    let transcript = directory.path().join("match.ndjson");
    let deck = repository_deck();
    let child = Command::new(binary())
        .args(["serve", "--port", "0", "--seed", "17", "--decks"])
        .arg(&deck)
        .arg(&deck)
        .args(["--output"])
        .arg(&transcript)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("host starts");
    let mut process = Process { child };
    let stdout = process.child.stdout.take().expect("host stdout is piped");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let line = receiver
        .recv_timeout(WAIT)
        .expect("host announced its assigned port")
        .expect("host announcement reads");
    let address = line
        .split_whitespace()
        .nth(2)
        .and_then(|text| text.strip_suffix(';'))
        .expect("host announcement has an address")
        .parse::<SocketAddr>()
        .expect("host announced a socket address");
    Host {
        process,
        port: address.port(),
        transcript,
    }
}

fn join(client: &mut RawClient, seat: &str) -> Value {
    client.send(&json!({"kind":"join", "version":1, "seat":seat}));
    client.receive()
}

fn join_pair(host: &Host) -> (RawClient, RawClient) {
    let mut one = RawClient::connect(host.port);
    assert_eq!(join(&mut one, "one")["kind"], "waiting");
    let mut two = RawClient::connect(host.port);
    assert_eq!(join(&mut two, "two")["kind"], "waiting");
    assert_eq!(one.receive()["kind"], "update");
    assert_eq!(two.receive()["kind"], "update");
    (one, two)
}

fn resign(client: &mut RawClient, player: &str, request_id: u64, revision: u64) {
    client.send(&json!({
        "kind":"submit",
        "version":1,
        "request_id":request_id,
        "based_on_revision":revision,
        "action":{"kind":"resign", "player":player}
    }));
}

fn end_turn(client: &mut RawClient, player: &str, request_id: u64, revision: u64) {
    client.send(&json!({
        "kind":"submit",
        "version":1,
        "request_id":request_id,
        "based_on_revision":revision,
        "action":{"kind":"end_turn", "player":player}
    }));
}

fn choose_prize(client: &mut RawClient, player: &str, request_id: u64, revision: u64) {
    client.send(&json!({
        "kind":"submit",
        "version":1,
        "request_id":request_id,
        "based_on_revision":revision,
        "action":{"kind":"choose_prize", "player":player, "prize_index":0}
    }));
}

fn start_player(directory: &TempDir, player: u8, port: u16) -> Client {
    let output = directory.path().join(format!("player-{player}.log"));
    let stdout = std::fs::File::create(&output).expect("client output file creates");
    let stderr = stdout.try_clone().expect("client output file clones");
    let mut child = Command::new(binary())
        .args([
            "play",
            "--player",
            &player.to_string(),
            "--port",
            &port.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("client starts");
    let input = child.stdin.take().expect("client stdin is piped");
    Client {
        process: Process { child },
        input,
        output,
    }
}

fn wait_for_text(path: &Path, required: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.contains(required) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{required:?} did not appear in {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn duplicate_seat_and_third_connection_are_rejected_without_stopping_players() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let mut one = RawClient::connect(host.port);
    assert_eq!(join(&mut one, "one")["kind"], "waiting");
    let mut duplicate = RawClient::connect(host.port);
    assert_eq!(join(&mut duplicate, "one")["reason"], "seat is occupied");
    duplicate.closes();
    let mut two = RawClient::connect(host.port);
    assert_eq!(join(&mut two, "two")["kind"], "waiting");
    assert_eq!(one.receive()["kind"], "update");
    assert_eq!(two.receive()["kind"], "update");
    let mut third = RawClient::connect(host.port);
    assert_eq!(join(&mut third, "two")["reason"], "seat is occupied");
    third.closes();
    resign(&mut two, "two", 1, 0);
    assert_eq!(one.receive()["kind"], "finished");
    assert_eq!(two.receive()["kind"], "finished");
}

#[test]
fn full_handshake_set_rejects_busy_connections_then_admits_players_after_release() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let first = RawClient::connect(host.port);
    let second = RawClient::connect(host.port);
    let third = RawClient::connect(host.port);
    let fourth = RawClient::connect(host.port);
    let mut busy = RawClient::connect(host.port);
    busy.send(&json!({"kind":"join", "version":1, "seat":"one"}));
    assert_eq!(busy.receive()["reason"], "server busy");
    busy.closes();
    drop((first, second, third, fourth));
    thread::sleep(Duration::from_millis(100));
    let (mut one, mut two) = join_pair(&host);
    resign(&mut one, "one", 1, 0);
    assert_eq!(one.receive()["kind"], "finished");
    assert_eq!(two.receive()["kind"], "finished");
}

#[test]
fn version_and_submit_before_join_are_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let mut unsupported = RawClient::connect(host.port);
    unsupported.send(&json!({"kind":"join", "version":99, "seat":"one"}));
    assert_eq!(unsupported.receive()["reason"], "unsupported version");
    let mut submitter = RawClient::connect(host.port);
    resign(&mut submitter, "one", 1, 0);
    assert_eq!(submitter.receive()["reason"], "join required");
}

#[test]
fn spoofed_actor_is_rejected_without_a_recorder_submission() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let (mut one, mut two) = join_pair(&host);
    resign(&mut one, "two", 1, 0);
    assert_eq!(
        one.receive()["reason"],
        "actor does not own this connection"
    );
    let transcript = wait_for_text(&host.transcript, "\"record\":\"header\"");
    assert!(!transcript.contains("\"resign\""));
    resign(&mut two, "two", 2, 0);
    assert_eq!(one.receive()["kind"], "finished");
    assert_eq!(two.receive()["kind"], "finished");
}

#[test]
fn request_ids_increase_and_stale_resign_can_finish_the_session() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let (mut one, mut two) = join_pair(&host);
    resign(&mut one, "two", 2, 0);
    assert_eq!(
        one.receive()["reason"],
        "actor does not own this connection"
    );
    resign(&mut one, "one", 1, 0);
    assert_eq!(one.receive()["reason"], "request id must increase");
    resign(&mut two, "two", 1, 999);
    assert_eq!(one.receive()["kind"], "finished");
    assert_eq!(two.receive()["kind"], "finished");
}

#[test]
fn recorder_rejection_advances_revision_before_the_next_action() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let (mut one, mut two) = join_pair(&host);
    choose_prize(&mut one, "one", 1, 0);
    let update = one.receive();
    assert_eq!(update["kind"], "update");
    assert_eq!(update["revision"], 1);
    assert_eq!(two.receive()["revision"], 1);
    end_turn(&mut one, "one", 2, 0);
    assert_eq!(one.receive()["reason"], "stale revision");
    resign(&mut two, "two", 1, 0);
    assert_eq!(one.receive()["kind"], "finished");
    assert_eq!(two.receive()["kind"], "finished");
    assert!(host.process.wait().success());
    let transcript = std::fs::read_to_string(&host.transcript).expect("transcript reads");
    assert!(transcript.contains("\"record\":\"step_rejected\""));
}

#[test]
fn unsupported_submit_after_admission_stops_both_players() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let (mut one, mut two) = join_pair(&host);
    one.send(&json!({
        "kind":"submit",
        "version":99,
        "request_id":1,
        "based_on_revision":0,
        "action":{"kind":"resign", "player":"one"}
    }));
    assert_eq!(one.receive()["reason"], "unsupported version");
    assert_eq!(two.receive()["kind"], "stopped");
    assert!(host.process.wait().success());
}

#[test]
fn stale_ordinary_action_is_rejected_and_either_seat_can_resign_stale() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let (mut one, mut two) = join_pair(&host);
    end_turn(&mut one, "one", 1, 99);
    assert_eq!(one.receive()["reason"], "stale revision");
    resign(&mut one, "one", 2, 99);
    assert_eq!(one.receive()["kind"], "finished");
    assert_eq!(two.receive()["kind"], "finished");
}

#[test]
fn malformed_admitted_frame_stops_the_session() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let mut client = RawClient::connect(host.port);
    assert_eq!(join(&mut client, "one")["kind"], "waiting");
    client.send_bytes(b"not-json\n");
    assert!(host.process.wait().success());
    let transcript =
        std::fs::read_to_string(&host.transcript).expect("incomplete transcript reads");
    assert!(!transcript.contains("\"record\":\"match_completed\""));
}

#[test]
fn unterminated_over_limit_client_input_is_closed_within_the_frame_limit() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let host = start_host(&directory);
    let mut oversized = RawClient::connect(host.port);
    oversized.send_bytes(&vec![b'x'; 64 * 1024]);
    let mut line = String::new();
    let size = oversized
        .reader
        .read_line(&mut line)
        .expect("server close reads");
    assert_eq!(size, 0, "over-limit unadmitted input closes the connection");
}

#[test]
fn admitted_disconnect_stops_without_a_completed_transcript() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let (one, mut two) = join_pair(&host);
    drop(one);
    assert_eq!(two.receive()["kind"], "stopped");
    assert!(host.process.wait().success());
    let transcript =
        std::fs::read_to_string(&host.transcript).expect("incomplete transcript reads");
    assert!(!transcript.contains("\"record\":\"match_completed\""));
}

#[test]
fn ctrl_c_keeps_the_transcript_incomplete() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let (_one, _two) = join_pair(&host);
    let status = Command::new("kill")
        .args(["-INT", &host.process.child.id().to_string()])
        .status()
        .expect("interrupt command runs");
    assert!(status.success());
    assert!(host.process.wait().success());
    let transcript =
        std::fs::read_to_string(&host.transcript).expect("incomplete transcript reads");
    assert!(!transcript.contains("\"record\":\"match_completed\""));
}

fn hand_ids(output: &str) -> Vec<&str> {
    output
        .lines()
        .flat_map(str::split_whitespace)
        .filter_map(|word| word.strip_prefix('#'))
        .collect()
}

#[test]
fn client_exits_on_finished_while_waiting_for_give_up_confirmation() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let mut one = start_player(&directory, 1, host.port);
    let mut two = RawClient::connect(host.port);
    assert_eq!(join(&mut two, "two")["kind"], "waiting");
    assert_eq!(two.receive()["kind"], "update");
    wait_for_text(&one.output, "Hand:");
    one.input
        .write_all(b"give up\n")
        .expect("confirmation input writes");
    one.input.flush().expect("confirmation input flushes");
    wait_for_text(&one.output, "Confirm Give up with yes");
    resign(&mut two, "two", 1, 0);
    assert_eq!(two.receive()["kind"], "finished");
    assert!(
        one.process.wait().success(),
        "pending confirmation client exits normally"
    );
    assert!(host.process.wait().success(), "host exits normally");
}

#[test]
fn actual_two_client_resignation_keeps_stdin_open_preserves_privacy_and_replays() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let mut one = start_player(&directory, 1, host.port);
    let mut two = start_player(&directory, 2, host.port);
    wait_for_text(&one.output, "Hand:");
    wait_for_text(&two.output, "Hand:");
    two.input
        .write_all(b"give up\nyes\n")
        .expect("resignation input writes");
    two.input.flush().expect("resignation input flushes");
    drop(two.input);
    assert!(two.process.wait().success(), "player two exits normally");
    assert!(
        one.process.wait().success(),
        "player one exits normally while stdin remains open"
    );
    drop(one.input);
    assert!(host.process.wait().success(), "host exits normally");
    let one_output = std::fs::read_to_string(&one.output).expect("player one output reads");
    let two_output = std::fs::read_to_string(&two.output).expect("player two output reads");
    assert!(one_output.contains("Finished: Player One wins by resignation"));
    assert!(two_output.contains("Finished: Player One wins by resignation"));
    assert!(two_output.contains("Hand:"));
    let one_ids = hand_ids(&one_output);
    let two_ids = hand_ids(&two_output);
    assert!(
        !one_ids.is_empty(),
        "player one has readable private hand IDs"
    );
    assert!(
        !two_ids.is_empty(),
        "player two has readable private hand IDs"
    );
    assert!(
        one_ids.iter().all(|id| !two_ids.contains(id)),
        "same deck seats have unique physical IDs"
    );
    let two_tokens: Vec<_> = two_output.split_whitespace().collect();
    assert!(
        one_ids.iter().all(|id| !two_tokens.contains(id)),
        "player two output hides player one private IDs"
    );
    let replay = Command::new(binary())
        .arg("replay")
        .arg(&host.transcript)
        .output()
        .expect("replay starts");
    assert!(
        replay.status.success(),
        "replay failed: {}",
        String::from_utf8_lossy(&replay.stderr)
    );
}
