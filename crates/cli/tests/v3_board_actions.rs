#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(10);
const ACTIONS: &str = "Actions: 1. Play Summon 2. Upgrade Summon 3. Retreat 4. Declare Attack 5. End Turn 6. Pass Priority 7. Convert Coin 8. Resign";

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

fn start_player(directory: &TempDir, player: u8, port: u16) -> PlayerClient {
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
            "{required:?} did not appear in {} within {WAIT:?}\n--- log ---\n{text}",
            path.display()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_count(path: &Path, required: &str, count: usize) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.matches(required).count() >= count {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{required:?} did not reach {count} occurrences in {} within {WAIT:?}\n--- log ---\n{text}",
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

fn nth_view_block(log: &str, occurrence: usize) -> String {
    let lines: Vec<&str> = log.lines().collect();
    let mut seen = 0;
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with("Hand: ") {
            seen += 1;
            if seen == occurrence {
                let end = (index + 3).min(lines.len());
                return lines[index..end].join("\n");
            }
        }
    }
    panic!("view block {occurrence} is not present in log:\n{log}");
}

#[test]
fn two_clients_play_board_actions_over_the_real_host() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory);
    let mut one = start_player(&directory, 1, host.port);
    let mut two = start_player(&directory, 2, host.port);

    wait_for_text(&one.output, ACTIONS);
    let two_initial_log = wait_for_text(&two.output, ACTIONS);
    let two_first_view = nth_view_block(&two_initial_log, 1);

    send_line(&mut two, "5");
    wait_for_text(&two.output, "Enter 1 to confirm, or cancel");
    send_line(&mut two, "1");
    wait_for_text(&two.output, "Rejected not your decision");

    let one_after_rejection = wait_for_count(&one.output, ACTIONS, 2);
    assert!(
        !one_after_rejection.contains("Rejected"),
        "player one observed a rejection meant for player two:\n{one_after_rejection}"
    );
    let two_after_rejection = wait_for_count(&two.output, ACTIONS, 2);
    let two_second_view = nth_view_block(&two_after_rejection, 2);
    assert_eq!(
        two_first_view, two_second_view,
        "the rejected end turn changed player two's rendered view"
    );
    let transcript_after_rejection =
        wait_for_text(&host.transcript, "\"record\":\"step_rejected\"");
    assert!(
        !transcript_after_rejection.contains("\"record\":\"step_completed\""),
        "a step completed before the rejection was recorded:\n{transcript_after_rejection}"
    );

    send_line(&mut one, "1");
    wait_for_text(&one.output, "1. Quarry Warden-Guard");
    send_line(&mut one, "1");
    wait_for_text(&one.output, "1. Bench 1");
    send_line(&mut one, "1");
    wait_for_text(
        &one.output,
        "Player One played Quarry Warden-Guard to Bench 1",
    );
    wait_for_text(
        &two.output,
        "Player One played Quarry Warden-Guard to Bench 1",
    );

    send_line(&mut one, "4");
    wait_for_text(&one.output, "1. Main");
    send_line(&mut one, "1");
    wait_for_text(&one.output, "4. No hint");
    send_line(&mut one, "4");
    wait_for_text(&one.output, "Player One declared an attack at Main");
    wait_for_text(&two.output, "Player One declared an attack at Main");

    send_line(&mut two, "6");
    wait_for_text(&two.output, "Player Two passed Priority");
    wait_for_text(&one.output, "Player Two passed Priority");
    send_line(&mut one, "6");
    wait_for_text(&one.output, "Player One passed Priority");
    wait_for_text(&one.output, "Damage 10 applied");
    wait_for_text(&two.output, "Player One passed Priority");
    wait_for_text(&two.output, "Damage 10 applied");

    send_line(&mut one, "5");
    wait_for_text(&one.output, "Enter 1 to confirm, or cancel");
    send_line(&mut one, "1");
    wait_for_count(&two.output, ACTIONS, 7);
    send_line(&mut two, "6");
    wait_for_count(&two.output, "Player Two passed Priority", 2);
    wait_for_count(&one.output, "Player Two passed Priority", 2);
    send_line(&mut one, "6");
    wait_for_count(&one.output, "Player One passed Priority", 2);
    wait_for_text(&two.output, "Player Two drew Second Wind");
    let one_after_first_draw = wait_for_text(&one.output, "Player Two drew a card");
    assert!(
        !one_after_first_draw.contains("Second Wind"),
        "player one saw player two's private card identity:\n{one_after_first_draw}"
    );

    send_line(&mut two, "1");
    wait_for_text(&two.output, "3. Set-Path Adept");
    send_line(&mut two, "3");
    wait_for_text(&two.output, "1. Bench 1");
    send_line(&mut two, "1");
    wait_for_text(&one.output, "Player Two played Set-Path Adept to Bench 1");
    wait_for_text(&two.output, "Player Two played Set-Path Adept to Bench 1");

    send_line(&mut two, "5");
    wait_for_count(&two.output, "Enter 1 to confirm, or cancel", 2);
    send_line(&mut two, "1");
    wait_for_count(&one.output, ACTIONS, 11);
    send_line(&mut one, "6");
    wait_for_count(&one.output, "Player One passed Priority", 3);
    wait_for_count(&two.output, "Player One passed Priority", 3);
    send_line(&mut two, "6");
    wait_for_count(&two.output, "Player Two passed Priority", 3);
    wait_for_text(&one.output, "Player One drew Standing Ward");
    let two_after_second_draw = wait_for_text(&two.output, "Player One drew a card");
    assert!(
        !two_after_second_draw.contains("Standing Ward"),
        "player two saw player one's private card identity:\n{two_after_second_draw}"
    );

    send_line(&mut one, "5");
    wait_for_count(&one.output, "Enter 1 to confirm, or cancel", 2);
    send_line(&mut one, "1");
    wait_for_count(&two.output, ACTIONS, 14);
    send_line(&mut two, "6");
    wait_for_count(&two.output, "Player Two passed Priority", 4);
    wait_for_count(&one.output, "Player Two passed Priority", 4);
    send_line(&mut one, "6");
    wait_for_text(&two.output, "Awaiting ManaProduction from Two");
    send_line(&mut two, "2");
    wait_for_text(&one.output, "Player Two produced Mind Mana");
    wait_for_text(&two.output, "Player Two produced Mind Mana");

    send_line(&mut one, "8");
    wait_for_text(&one.output, "Confirm Give up with yes");
    send_line(&mut one, "yes");
    wait_for_text(&one.output, "Finished: Player Two wins by resignation");
    wait_for_text(&two.output, "Finished: Player Two wins by resignation");

    let one_final_log = std::fs::read_to_string(&one.output).expect("player one log reads");
    assert!(
        !one_final_log.contains("Rejected"),
        "player one observed a rejection meant for player two:\n{one_final_log}"
    );

    assert!(one.process.wait().success(), "player one exits normally");
    assert!(two.process.wait().success(), "player two exits normally");
    assert!(host.process.wait().success(), "host exits normally");

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
    let replay_stdout = String::from_utf8_lossy(&replay.stdout);
    assert!(
        replay_stdout.contains("transcript verified"),
        "replay did not print the verification confirmation line: {replay_stdout}"
    );

    let transcript = std::fs::read_to_string(&host.transcript).expect("transcript reads");
    let rejected_index = transcript
        .find("\"record\":\"step_rejected\"")
        .expect("the transcript records the rejected step");
    let completed_index = transcript
        .find("\"record\":\"step_completed\"")
        .expect("the transcript records at least one completed step");
    assert!(
        rejected_index < completed_index,
        "the rejected step was not recorded before the first completed step"
    );
}
