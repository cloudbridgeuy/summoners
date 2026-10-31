# Summoners development guidance

## Current boundary

`crates/core` is the deterministic functional core. It owns game vocabulary,
legal choices, state transitions, resolution, triggers, destruction,
promotion, loss checks, and observable outcomes as those features are added.
It must not read files, call a network, use a clock, or own another I/O effect.

`crates/cards` is the strict authored-document boundary. It parses caller-held
bytes without file I/O, applies content policy, and converts valid definitions
into core entities. It also assembles loaded Sets into one shared card library,
resolves strict Deck documents, and exposes the embedded catalog through one
cached runtime path. It depends only on `summoners_core`.

`crates/match-log` (`summoners_match_log`) owns the version-1 NDJSON transcript
schema: record, parse, compare, replay, and the state digest. Wire conversion,
canonical state bytes, and digests are pure. It opens no file and no socket; the
recording adapter only writes to a caller-held writer. It depends on
`summoners_core` and `summoners_cards`.

`crates/cli` (`summoners-cli`, binary `summoners`) is the game shell with
`serve`, `play`, and `replay`. `serve` hosts a two-seat TCP match, reads the Deck
files, and writes the transcript. `play` is a TCP client with a terminal prompt.
`replay` reads a transcript and verifies it. It owns the files, sockets, terminal,
randomness, and async runtime (`tokio`). It depends on `summoners_core`,
`summoners_cards`, and `summoners_match_log`.

`crates/cceroby` is a separate museum image search CLI and local HTTP server for
card art, with a disk cache and image download. It owns network, file, and
browser effects. It does not depend on any other workspace crate.

`xtask` is repository automation: `lint`, `golden` (records the checked-in
replay transcripts), and the Git pre-commit hook. It may run processes and
touch the repository. It depends on `summoners_core`, `summoners_cards`, and
`summoners_match_log`.

Add a new shell or adapter crate (TUI client, server, simulator, FFI) only after
its boundary and dependency set are explicit. Keep gameplay concerns as modules in `summoners_core`. Do not create one crate
per rule or concept.

## Code policy

- Follow `~/.claude/patterns/functional-core-imperative-shell.md` for every implementation.
- Parse raw external input into domain types at its shell boundary; see `~/.claude/patterns/parse-dont-validate.md`.
- Model closed choices and states with enums and structs; see `~/.claude/patterns/algebraic-data-types.md` and `~/.claude/patterns/make-impossible-states-impossible.md`.
- Prefer precise signatures and private smart constructors; see `~/.claude/patterns/type-driven-development.md`.
- Use composition and pure functions instead of trait objects unless an open extension point exists; see `~/.claude/patterns/composition-over-inheritance.md`.
- Do not add CQRS or data-oriented layouts without a concrete read/write split or measured performance need.

Production crate roots deny `clippy::unwrap_used` and
`clippy::expect_used`. Tests can allow them locally when that keeps fixtures
clear.

## Tests and quality

Keep unit tests next to the pure rule they exercise. Test each enum variant and
state transition. Add integration tests only for real shell wiring.

`cargo xtask lint` is the canonical quality gate. It runs only from the
pre-commit hook (`cargo xtask lint --staged-only`). Install the hook once per
clone:

```sh
cargo xtask lint --install-hooks
```

Agents must not run `cargo xtask lint` themselves. While working, run targeted
checks instead:

```sh
cargo test -p <crate>
cargo clippy -p <crate> --all-targets
```

The no-wrapper fallback for the hook installer is
`cargo run -p xtask -- lint --install-hooks`. Do not use skip flags to get a
commit through.

## Domain documents

`CONTEXT.md` is the product-language index. The files under `designs/` contain
design inputs and open questions. Do not state that a design rule is
implemented until code and tests make it true.
