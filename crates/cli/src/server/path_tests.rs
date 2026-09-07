#![allow(clippy::expect_used)]
use super::*;
use tempfile::TempDir;

const CONTROL_NAME: &str = "bad\n\r\t\u{1b}\u{0007}\u{007f}name";
const ESCAPED_FORMS: [&str; 6] = ["\\n", "\\r", "\\t", "\\x1b", "\\u{0007}", "\\u{007f}"];

fn assert_escaped(text: &str) {
    for form in ESCAPED_FORMS {
        assert!(text.contains(form), "missing {form:?} in {text:?}");
    }
    assert!(
        !text.chars().any(char::is_control),
        "raw control character in {text:?}"
    );
}

#[test]
fn listening_line_escapes_control_characters_in_the_output_path() {
    let address: SocketAddr = "127.0.0.1:4000".parse().expect("address");
    let line = listening_line(address, Path::new(CONTROL_NAME));
    assert_escaped(&line);
    assert!(line.starts_with("Listening on 127.0.0.1:4000; recording to"));
    let tokens: Vec<&str> = line.split_whitespace().collect();
    assert_eq!(tokens[2], "127.0.0.1:4000;");
}

#[test]
fn read_deck_error_escapes_a_missing_control_character_path() {
    let directory = TempDir::new().expect("directory");
    let path = directory.path().join(CONTROL_NAME);
    let error = read_deck(&path).expect_err("missing deck cannot read");
    assert!(matches!(error, ServeError::ReadDeck { .. }));
    assert_escaped(&error.to_string());
}

#[test]
fn deck_too_large_error_escapes_a_control_character_path() {
    let directory = TempDir::new().expect("directory");
    let path = directory.path().join(CONTROL_NAME);
    fs::write(&path, vec![0; MAX_DECK_BYTES as usize + 1]).expect("deck");
    let error = read_deck(&path).expect_err("oversize deck cannot read");
    assert!(matches!(error, ServeError::DeckTooLarge { .. }));
    assert_escaped(&error.to_string());
}

#[test]
fn parse_deck_error_escapes_a_control_character_path() {
    let directory = TempDir::new().expect("directory");
    let path = directory.path().join(CONTROL_NAME);
    fs::write(&path, "not a deck").expect("invalid deck");
    let catalog = built_in_catalog().expect("catalog");
    let error = load_deck(&path, catalog.library()).expect_err("invalid deck cannot parse");
    assert!(matches!(error, ServeError::ParseDeck { .. }));
    assert_escaped(&error.to_string());
}

#[test]
fn output_error_escapes_an_existing_control_character_path() {
    let directory = TempDir::new().expect("directory");
    let path = directory.path().join(CONTROL_NAME);
    fs::write(&path, "existing").expect("output");
    let error = create_explicit_output(&path).expect_err("existing output cannot be recreated");
    assert!(matches!(error, ServeError::Output { .. }));
    assert_escaped(&error.to_string());
}
