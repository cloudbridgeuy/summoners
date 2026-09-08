# Summoners

Home of Summoners, a two-player card game.

This repository contains the deterministic game rules, authored card data, a
local museum image search shell, and development tools. The search shell
connects to the Art Institute of Chicago, the Cleveland Museum of Art, the
Metropolitan Museum of Art, and Wikimedia Commons. It connects to Smithsonian
Open Access when `SMITHSONIAN_API_KEY` contains an api.data.gov key.

## Workspace

- `crates/core`: the dependency-free deterministic rules and state-transition library.
- `crates/cards`: strict authored Set and Deck loading with built-in content.
- `crates/cceroby`: local museum image search CLI and form with public-domain
  Art Institute of Chicago, CC0 Cleveland Museum, Metropolitan Museum of Art,
  configured Smithsonian Open Access results, and Wikimedia Commons results;
  locally proxied artwork
  previews; compact URL-free card credit with separate license and source
  links; full attribution in downloaded JPEG XMP; a 24-hour metadata cache; a
  30-day image cache; and a trusted self-contained JPEG download path.
  Cleveland downloads use valid absolute HTTP(S) full TIFF originals when
  available and the best valid absolute HTTP(S) JPEG otherwise. The CLI can
  inspect or clear its cache. A transient browser session stops after its last
  tab closes; `--serve` keeps it available until Ctrl-C. Results load in fixed
  source order and can load the next independent source page without duplicate
  source objects. Wikimedia Commons keeps only CC0, Public Domain Mark, and CC
  BY files from trusted file metadata. The culture or region field accepts
  `MNAV` and `CdF` as Uruguay institution filters.
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

## Play a local match

See the [game shell guide](docs/shell.md) for command details, prompts, local
commands, transcripts, and recovery. The host owns both Deck paths and the
transcript. Start it with two repository Deck files and an explicit seed:

```sh
mkdir -p matches
cargo run -p summoners-cli -- serve --bind 127.0.0.1 --port 4000 --seed 0 --decks crates/cards/data/set-paths.toml crates/cards/data/barrow-herd.toml --output matches/local.ndjson
```

In two other terminals, join the host with explicit seats:

```sh
cargo run -p summoners-cli -- play --player 1 --host localhost --port 4000
cargo run -p summoners-cli -- play --player 2 --host 127.0.0.1 --port 4000
```

Replay the completed transcript with:

```sh
cargo run -p summoners-cli -- replay matches/local.ndjson
```

The current opening starts in Main phase with Player One active and Player Two
holding the Coin. It does not run Player One's opening Upkeep.

## Search museum images

To enable Smithsonian Open Access, get an api.data.gov key and set it only in
the process environment:

```sh
export SMITHSONIAN_API_KEY='<api.data.gov key>'
```

The Smithsonian source stays unavailable when this variable is missing, empty,
or not valid as an HTTP header. A syntactically valid key that the service
rejects produces one provider failure after the request. The search page does
not show the variable name or its value.

Inspect or clear Cceroby's user cache with:

```sh
cargo run -p cceroby -- cache info
cargo run -p cceroby -- cache clear
```

On Unix, cache access and pruning use no-follow file handles. On Apple
platforms, Linux, and Android, cache removal first detaches the exact cache
root. Thus, a new cache that a running search creates remains. A removal error
returns a failure and does not print a success report. If the detached root or
a nested entry changes during removal, Cceroby stops and does not delete the
replacement. A platform without the required atomic rename refuses cache
removal. On non-Unix platforms, Cceroby keeps search work available without
cache I/O.

## License

Summoners is available under the [MIT License](LICENSE).
