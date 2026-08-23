# Summoners

Home of Summoners, a two-player card game.

This repository contains a deterministic Rust game engine, a strict
authored-card boundary with a built-in catalog, a versioned match-transcript
library, and a `summoners` command-line shell that verifies, replays, and
plays match transcripts.

## Workspace

- `crates/core`: the dependency-free deterministic rules and state-transition library.
- `crates/cards`: the strict authored-document boundary — parses Set and Deck bytes, applies content policy, and exposes one cached built-in catalog.
- `crates/match-log`: the versioned NDJSON match-transcript library — recording, parsing, comparison, and replay verification.
- `crates/shell`: the `summoners` command-line binary — argument mapping, exit-code classification, and file effects around the libraries above.
- `xtask`: repository lint and Git hook automation.

## Usage

Verify that a recorded transcript reproduces itself against the engine:

```sh
summoners verify match.ndjson
```

Replay a transcript's recorded actions into a fresh recording and compare
the result against the original:

```sh
summoners replay --from match.ndjson --output observed.ndjson
```

Play a scenario transcript interactively — only its header metadata,
required Set revisions, and initial state are read; its recorded actions
are not:

```sh
summoners play --from scenario.ndjson --output played.ndjson
```

Every command that writes an output file writes to `<output>.partial`
first and renames it into place only once the result is complete; add
`--force` to replace an output file that already exists.

## Design inputs

- [Core rules](designs/core_rules.md)
- [Core rules source document](designs/core_rules.docx)
- [Mana types and archetypes](designs/types_archetypes.md)

The design files are inputs for later implementation plans. They are not proof
that a rule exists in code.

## Development

Run the complete local quality gate:

```sh
cargo xtask lint
```

If the global `cargo-xtask` wrapper is not available, run:

```sh
cargo run -p xtask -- lint
```

Run the workspace tests directly with:

```sh
cargo test --workspace --all-targets
```

## License

Summoners is available under the [MIT License](LICENSE).
