//! User-visible command-line behavior.
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

#[test]
fn invalid_output_path_has_one_short_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_cceroby"))
        .args(["search", "mask", "--out", "Cargo.toml"])
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => panic!("cannot run cceroby: {error}"),
    };
    let stderr = match String::from_utf8(output.stderr) {
        Ok(stderr) => stderr,
        Err(error) => panic!("stderr is not UTF-8: {error}"),
    };

    assert!(!output.status.success());
    assert_eq!(stderr, "output path is not a directory: Cargo.toml\n");
}
