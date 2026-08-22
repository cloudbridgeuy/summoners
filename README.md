# Summoners

Home of Summoners, a two-player card game.

This repository contains the deterministic game rules, authored card data, a
local museum image search shell, and development tools. The search shell
connects to the Art Institute of Chicago. Four additional source slots remain
unavailable.

## Workspace

- `crates/core`: the dependency-free deterministic rules and state-transition library.
- `crates/cards`: strict authored Set and Deck loading with built-in content.
- `crates/cceroby`: local museum image search CLI and form with public-domain
  Art Institute of Chicago results and a 24-hour metadata cache.
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
