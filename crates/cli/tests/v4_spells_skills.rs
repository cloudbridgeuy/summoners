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
const ACTIONS: &str = "Actions: 1. Play Summon 2. Upgrade Summon 3. Retreat 4. Declare Attack 5. End Turn 6. Pass Priority 7. Convert Coin 8. Resign 9. Cast Spell 10. Activate Skill";
const SEED: u64 = 0;
const SCRYING_GLASS_HAND_INDEX: &str = "2";

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

fn fixture_deck() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/barrow-seer-lead.toml")
}

fn start_host(directory: &TempDir, seed: u64, deck_one: &Path, deck_two: &Path) -> Host {
    let transcript = directory.path().join("match.ndjson");
    let child = Command::new(binary())
        .args([
            "serve",
            "--port",
            "0",
            "--seed",
            &seed.to_string(),
            "--decks",
        ])
        .arg(deck_one)
        .arg(deck_two)
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

fn wait_for_text_from(path: &Path, from: usize, required: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let start = from.min(text.len());
        if text[start..].contains(required) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "{required:?} did not appear after offset {from} in {} within {WAIT:?}\n--- log ---\n{text}",
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

fn read_log(path: &Path) -> String {
    std::fs::read_to_string(path).expect("client log reads")
}

#[test]
fn two_clients_cast_a_spell_and_view_prizes_over_the_real_host() {
    let directory = tempfile::tempdir().expect("temporary directory exists");
    let mut host = start_host(&directory, SEED, &repository_deck(), &fixture_deck());
    let mut one = start_player(&directory, 1, host.port);
    let mut two = start_player(&directory, 2, host.port);

    wait_for_text(&one.output, ACTIONS);
    wait_for_text(&two.output, ACTIONS);

    let one_before_inspect = read_log(&one.output).len();
    send_line(&mut one, "inspect 1");
    let one_after_inspect = wait_for_text_from(
        &one.output,
        one_before_inspect,
        "cost matter 0 mind 0 spirit 0 generic 1",
    );
    let inspected_hand_line = one_after_inspect[one_before_inspect..]
        .lines()
        .find(|line| line.contains("cost matter 0 mind 0 spirit 0 generic 1"))
        .expect("the hand inspection line is present");
    assert!(
        inspected_hand_line.starts_with("Second Wind"),
        "hand card 1 was not named Second Wind: {inspected_hand_line}"
    );

    send_line(&mut one, "inspect 0");
    wait_for_text(&one.output, "Invalid inspection");

    let two_before_board_inspect = read_log(&two.output).len();
    send_line(&mut two, "inspect board 2 1");
    let two_after_board_inspect =
        wait_for_text_from(&two.output, two_before_board_inspect, "look at Prizes");
    let inspected_board_line = two_after_board_inspect[two_before_board_inspect..]
        .lines()
        .find(|line| line.contains("look at Prizes"))
        .expect("the board inspection line is present");
    assert!(
        inspected_board_line.contains("Barrow Seer"),
        "the inspected board card was not named Barrow Seer: {inspected_board_line}"
    );
    assert!(
        inspected_board_line.contains("Skill"),
        "the inspected board card did not describe a Skill: {inspected_board_line}"
    );

    send_line(&mut one, "5");
    wait_for_text(&one.output, "Enter 1 to confirm, or cancel");
    send_line(&mut one, "1");
    wait_for_text(&two.output, "Priority: Some(Two)");
    send_line(&mut two, "6");
    wait_for_text(&two.output, "Player Two passed Priority");
    wait_for_text(&one.output, "Player Two passed Priority");
    send_line(&mut one, "6");
    wait_for_text(&one.output, "Player One passed Priority");
    wait_for_text(&two.output, "Player One passed Priority");
    wait_for_text(&two.output, "Player Two drew");
    wait_for_text(&one.output, "Player Two produced Spirit Mana");
    wait_for_text(&two.output, "Player Two produced Spirit Mana");

    send_line(&mut two, "7");
    wait_for_text(&two.output, "3. Spirit");
    send_line(&mut two, "3");
    wait_for_text(&one.output, "Player Two converted Coin to Spirit Mana");
    wait_for_text(&two.output, "Player Two converted Coin to Spirit Mana");

    let one_before_skill = read_log(&one.output).len();
    let two_before_skill = read_log(&two.output).len();
    send_line(&mut two, "10");
    wait_for_text(&two.output, "1. Main");
    send_line(&mut two, "1");
    send_line(&mut two, "1");
    send_line(&mut two, "5");
    send_line(&mut two, "4");

    let two_after_skill = wait_for_text_from(
        &two.output,
        two_before_skill,
        "Player Two activated a Skill at Main",
    );
    let two_skill_window = &two_after_skill[two_before_skill..];
    let prizes_line = two_skill_window
        .lines()
        .find(|line| line.starts_with("Player Two viewed Prizes: "))
        .expect("player two's log names the viewed Prizes");
    let viewed_names: Vec<&str> = prizes_line
        .trim_start_matches("Player Two viewed Prizes: ")
        .split(", ")
        .collect();
    assert_eq!(
        viewed_names.len(),
        2,
        "expected exactly two viewed prizes: {prizes_line}"
    );
    assert!(
        viewed_names.iter().all(|name| !name.is_empty()),
        "a viewed Prize name was empty: {prizes_line}"
    );
    assert!(
        two_skill_window
            .lines()
            .any(|line| line.starts_with("Player Two drew ") && line != "Player Two drew a card"),
        "player two did not observe drawing a named card from the Skill: {two_skill_window}"
    );

    let one_after_skill = wait_for_text_from(
        &one.output,
        one_before_skill,
        "Player Two activated a Skill at Main",
    );
    let one_skill_window = &one_after_skill[one_before_skill..];
    assert!(
        one_skill_window
            .lines()
            .any(|line| line == "Player Two viewed 2 Prizes"),
        "player one did not observe the masked Prize count: {one_skill_window}"
    );
    assert!(
        one_skill_window
            .lines()
            .any(|line| line == "Player Two drew a card"),
        "player one did not observe the masked draw: {one_skill_window}"
    );
    assert!(
        !one_skill_window.contains("viewed Prizes:"),
        "player one's log leaked a Prize identity: {one_skill_window}"
    );

    send_line(&mut two, "5");
    wait_for_text(&two.output, "Enter 1 to confirm, or cancel");
    send_line(&mut two, "1");
    wait_for_count(&one.output, "Priority: Some(One)", 2);
    send_line(&mut one, "6");
    wait_for_count(&one.output, "Player One passed Priority", 2);
    wait_for_count(&two.output, "Player One passed Priority", 2);
    send_line(&mut two, "6");
    wait_for_count(&two.output, "Player Two passed Priority", 2);
    wait_for_count(&one.output, "Player Two passed Priority", 2);
    wait_for_text(&one.output, "Player One produced Matter Mana");
    wait_for_text(&two.output, "Player One produced Matter Mana");

    let one_before_cast = read_log(&one.output).len();
    let two_before_cast = read_log(&two.output).len();
    send_line(&mut one, "9");
    wait_for_text(&one.output, "2. Scrying Glass");
    send_line(&mut one, SCRYING_GLASS_HAND_INDEX);
    send_line(&mut one, "5");
    send_line(&mut one, "4");

    wait_for_text(&one.output, "Player One cast Scrying Glass");
    wait_for_text(&two.output, "Player One cast Scrying Glass");

    send_line(&mut two, "6");
    wait_for_count(&two.output, "Player Two passed Priority", 3);
    wait_for_count(&one.output, "Player Two passed Priority", 3);

    send_line(&mut one, "6");
    wait_for_count(&one.output, "Player One passed Priority", 3);
    wait_for_count(&two.output, "Player One passed Priority", 3);

    wait_for_text(&one.output, "A Stack item resolved");
    wait_for_text(&two.output, "A Stack item resolved");

    let one_after_cast = read_log(&one.output);
    let one_cast_window = &one_after_cast[one_before_cast..];
    let one_drew_line = one_cast_window
        .lines()
        .find(|line| line.starts_with("Player One drew "))
        .expect("player one observed their own draw from Scrying Glass");
    assert_ne!(
        one_drew_line, "Player One drew a card",
        "player one's own draw was unexpectedly masked"
    );

    let two_after_cast = read_log(&two.output);
    let two_cast_window = &two_after_cast[two_before_cast..];
    let two_drew_line = two_cast_window
        .lines()
        .find(|line| line.starts_with("Player One drew "))
        .expect("player two observed player one's draw");
    assert_eq!(
        two_drew_line, "Player One drew a card",
        "player two saw player one's private card identity: {two_drew_line}"
    );

    let one_before_history = read_log(&one.output);
    let two_before_history = read_log(&two.output);
    assert_eq!(
        two_before_history
            .matches("Player Two viewed Prizes: ")
            .count(),
        1
    );
    assert_eq!(
        one_before_history
            .matches("Player Two viewed 2 Prizes")
            .count(),
        1
    );
    assert_eq!(one_before_history.matches("viewed Prizes:").count(), 0);

    send_line(&mut one, "history");
    send_line(&mut two, "history");

    wait_for_count(&two.output, "Player Two viewed Prizes: ", 2);
    wait_for_count(&one.output, "Player Two viewed 2 Prizes", 2);

    let one_after_history = read_log(&one.output);
    assert_eq!(
        one_after_history
            .matches("Player Two viewed Prizes: ")
            .count(),
        0,
        "player one's history replay leaked a Prize identity: {one_after_history}"
    );
    assert_eq!(
        one_after_history.matches("viewed Prizes:").count(),
        0,
        "player one's log never names a viewed Prize: {one_after_history}"
    );
    let two_after_history = read_log(&two.output);
    assert_eq!(
        two_after_history
            .matches("Player Two viewed Prizes: ")
            .count(),
        2,
        "player two's history replay did not double the Prize identity line: {two_after_history}"
    );

    send_line(&mut one, "8");
    wait_for_text(&one.output, "Confirm Give up with yes");
    send_line(&mut one, "yes");
    wait_for_text(&one.output, "Finished: Player Two wins by resignation");
    wait_for_text(&two.output, "Finished: Player Two wins by resignation");

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

    let transcript = read_log(&host.transcript);
    assert!(
        transcript.contains("\"kind\":\"activate_skill\""),
        "the transcript did not record the activate_skill action: {transcript}"
    );
    assert!(
        transcript.contains("\"kind\":\"cast_spell\""),
        "the transcript did not record the cast_spell action: {transcript}"
    );
}
