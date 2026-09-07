pub fn terminal_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            '\x1b' => "\\x1b".chars().collect(),
            character if character.is_control() => {
                format!("\\u{{{:04x}}}", character as u32).chars().collect()
            }
            character => vec![character],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::terminal_text;

    #[test]
    fn terminal_text_escapes_every_terminal_control() {
        assert_eq!(
            terminal_text("a\n\r\t\u{1b}\u{0007}b"),
            "a\\n\\r\\t\\x1b\\u{0007}b"
        );
    }
}
