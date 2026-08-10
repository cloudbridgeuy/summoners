# Summoners

Home of Summoners, a two-player card game.

This repository currently contains the Rust workspace and development tools.
It does not yet implement the game rules.

## Workspace

- `crates/core`: the dependency-free deterministic rules and state-transition library.
- `xtask`: repository lint and Git hook automation.

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
