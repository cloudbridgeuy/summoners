#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(20);
const ACTIONS: &str = "Actions: 1. Play Summon 2. Upgrade Summon 3. Retreat 4. Declare Attack 5. End Turn 6. Pass Priority 7. Convert Coin 8. Resign 9. Cast Spell 10. Activate Skill";
const SEED: u64 = 0;

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

struct PlayerClient {
    process: Process,
    input: ChildStdin,
    output: PathBuf,
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_summoners")
}

fn repository_deck(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../cards/data")
        .join(name)
}

fn start_host(directory: &TempDir) -> Host {
    let transcript = directory.path().join("match.ndjson");
    let child = Command::new(binary())
        .args(["serve", "--bind", "127.0.0.1", "--port", "0", "--seed"])
        .arg(SEED.to_string())
        .args(["--decks"])
        .arg(repository_deck("set-paths.toml"))
        .arg(repository_deck("barrow-herd.toml"))
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

fn start_player(directory: &TempDir, player: u8, host: &str, port: u16) -> PlayerClient {
    let output = directory.path().join(format!("player-{player}.log"));
    let stdout = std::fs::File::create(&output).expect("client output file creates");
    let stderr = stdout.try_clone().expect("client output file clones");
    let mut child = Command::new(binary())
        .args([
            "play",
            "--player",
            &player.to_string(),
            "--host",
            host,
            "--port",
            &port.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("client starts");
    let input = child.stdin.take().expect("client stdin is piped");
    PlayerClient {
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
            "{required:?} did not appear in {} within {WAIT:?}\n{text}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_after(path: &Path, offset: usize, required: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text[offset.min(text.len())..].contains(required) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{required:?} did not appear after {offset} in {} within {WAIT:?}\n{text}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_turn_or_finish(path: &Path, offset: usize) -> bool {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let recent = &text[offset.min(text.len())..];
        if recent.contains("Finished:") {
            return true;
        }
        if recent.contains(ACTIONS) {
            return false;
        }
        assert!(
            Instant::now() < deadline,
            "turn result did not appear after {offset} in {} within {WAIT:?}\n{text}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn send_line(client: &mut PlayerClient, line: &str) {
    client
        .input
        .write_all(line.as_bytes())
        .expect("client input writes");
    client
        .input
        .write_all(b"\n")
        .expect("client input writes a newline");
    client.input.flush().expect("client input flushes");
}

fn end_turn(active: &mut PlayerClient, priority: &mut PlayerClient) -> bool {
    let active_before = std::fs::read_to_string(&active.output)
        .expect("active output reads")
        .len();
    send_line(active, "5");
    wait_for_after(
        &active.output,
        active_before,
        "Enter 1 to confirm, or cancel",
    );
    let priority_before = std::fs::read_to_string(&priority.output)
        .expect("priority output reads")
        .len();
    send_line(active, "1");
    wait_for_after(&priority.output, priority_before, "Priority: Some(");
    let active_priority_before = std::fs::read_to_string(&active.output)
        .expect("active output reads")
        .len();
    send_line(priority, "6");
    wait_for_after(&active.output, active_priority_before, "passed Priority");
    let final_before = std::fs::read_to_string(&active.output)
        .expect("active output reads")
        .len();
    send_line(active, "6");
    wait_for_turn_or_finish(&active.output, final_before)
}

#[test]
fn real_clients_complete_an_empty_deck_loss_and_replay() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let mut one = start_player(&directory, 1, "localhost", host.port);
    let mut two = start_player(&directory, 2, "127.0.0.1", host.port);
    wait_for_text(&one.output, ACTIONS);
    wait_for_text(&two.output, ACTIONS);

    let mut finished = false;
    for turn in 0..40 {
        finished = if turn % 2 == 0 {
            end_turn(&mut one, &mut two)
        } else {
            end_turn(&mut two, &mut one)
        };
        if finished {
            break;
        }
    }
    assert!(finished, "the scripted ordinary turns did not reach a loss");
    let one_output = wait_for_text(&one.output, "Finished:");
    let two_output = wait_for_text(&two.output, "Finished:");
    assert!(one_output.contains("empty deck draw"));
    assert!(two_output.contains("empty deck draw"));
    for output in [&one_output, &two_output] {
        assert!(!output.contains("seed"));
        assert!(!output.contains("GameState"));
    }

    assert!(one.process.wait().success(), "player one exits normally");
    assert!(two.process.wait().success(), "player two exits normally");
    assert!(host.process.wait().success(), "host exits normally");

    let transcript = std::fs::read_to_string(&host.transcript).expect("transcript reads");
    assert!(transcript.contains("\"record\":\"match_completed\""));
    assert!(transcript.contains("\"reason\":\"empty_deck_draw\""));
    assert!(!transcript.contains("\"kind\":\"resign\""));
    assert!(transcript.matches("\"kind\":\"end_turn\"").count() >= 20);

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
    assert!(String::from_utf8_lossy(&replay.stdout).contains("transcript verified"));
}
