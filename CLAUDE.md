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
cached runtime path. There is no product runtime shell yet. Add a CLI, server,
simulator, runtime file loader, client, protocol adapter, or FFI crate only
after its boundary and dependency set are explicit.

Keep gameplay concerns as modules in `summoners_core`. Do not create one crate
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

The canonical quality gate is:

```sh
cargo xtask lint
```

The no-wrapper fallback is:

```sh
cargo run -p xtask -- lint
```

Do not use skip flags for completion evidence.

## Domain documents

`CONTEXT.md` is the product-language index. The files under `designs/` contain
design inputs and open questions. Do not state that a design rule is
implemented until code and tests make it true.
