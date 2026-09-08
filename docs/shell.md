# Game shell

Use the game shell to host one local or trusted-network match. The host loads
both Deck files and writes the transcript. Each player runs one client.

## Prerequisites

Install Rust with Cargo. Run all commands from the repository root. Build the
shell with:

```sh
cargo build -p summoners-cli
```

`cargo run -p summoners-cli -- ...` also builds it when needed.

## Start a local match

Use three terminals. The explicit output parent must already exist. The
`matches/local.ndjson` path must be unused before you start. These commands
use two Deck files in this repository, a fixed seed, and explicit seats.

In terminal one, start the host:

```sh
mkdir -p matches
cargo run -p summoners-cli -- serve --bind 127.0.0.1 --port 4000 --seed 0 --decks crates/cards/data/set-paths.toml crates/cards/data/barrow-herd.toml --output matches/local.ndjson
```

In terminal two, join as Player One:

```sh
cargo run -p summoners-cli -- play --player 1 --host localhost --port 4000
```

In terminal three, join as Player Two:

```sh
cargo run -p summoners-cli -- play --player 2 --host 127.0.0.1 --port 4000
```

After the match finishes, verify its transcript:

```sh
cargo run -p summoners-cli -- replay matches/local.ndjson
```

## Commands and files

`serve` starts the host:

```text
summoners serve [--bind IP] [--port PORT] --decks DECK_A DECK_B [--output PATH] [--seed SEED]
```

`--bind` defaults to `127.0.0.1`. `--port` is optional. Without it, the host
asks the system for a port and prints the assigned address. `--decks` requires
exactly two paths. The host reads these paths. `--seed` is optional. Without
it, the host creates a random seed.

`--output` is optional. The host owns this file. An explicit output path must
have an existing parent. It must not already exist. The host never overwrites
it. Without `--output`, the host creates `matches/` and creates a unique
`matches/<unix-seconds>-<suffix>.ndjson` file.

`play` starts one client:

```text
summoners play --player 1|2 [--host HOST] --port PORT
```

`--host` defaults to `127.0.0.1`. It is the address to join. `--port` is
required. `--player` is required and accepts only `1` or `2`. Clients do
not read Deck files or write transcripts.

`replay` verifies one completed transcript:

```text
summoners replay TRANSCRIPT
```

The command reads the transcript path. It checks the transcript against the
built-in card catalog and prints `OK ...: transcript verified ...` on success.

## Network use

`--bind` selects the host listener. `--host` selects the host that a client
joins. Do not use `0.0.0.0` for `--host`; it is a listener wildcard, not a
destination. A host can bind `0.0.0.0` to accept network connections. Clients
must then use a real host address.

The protocol is unencrypted. A client declares its own seat. The host trusts
that explicit seat after it accepts the connection. Use loopback or a trusted
network.

When `--port 0` is used with `serve`, read the `Listening on ADDRESS; ...`
line and use its assigned port in both `play` commands. `play --port 0` tries
to join port zero and does not select a host port.

## Read the screen

Each update prints three lines: `Hand`, `Board`, and `Phase`. The hand lists
your visible cards. The board lists Player One and Player Two. `Main` is the
main position. `Bench` lists three bench positions. A summon shows its card
chain, damage, and readiness.

The client prints the action menu after each update. It can also print an
awaiting-pending line and the current priority holder. A newer game revision
cancels any guided form. The client prints `Prompt cancelled`. Enter the next
choice from the new menu.

The engine can reject a submitted action. The requesting client prints
`Rejected REASON`. Read the reason, then use the next menu. A local bad menu
or form choice prints `Invalid selection`.

If the host is interrupted, connected clients print `Stopped interrupted`.
A lost client connection prints `Stopped connection closed`. If client input
closes, it prints `Stopped input closed`.

The current opening observation is Main phase with Player One active. Player
Two has the Coin. Player One has no opening Upkeep.

## Actions

Type the action number. All displayed card, skill, and menu numbers start at
one.

1. **Play Summon.** Choose a hand card. Then choose Bench 1, 2, or 3.
2. **Upgrade Summon.** Choose a hand card. Then choose Main or Bench 1, 2,
   or 3.
3. **Retreat.** Choose Bench 1, 2, or 3. Then choose Matter, Mind, Spirit, or
   No hint.
4. **Declare Attack.** Choose Main or Bench 1, 2, or 3. Then choose Matter,
   Mind, Spirit, or No hint.
5. **End Turn.** Enter `1` to confirm.
6. **Pass Priority.** The client sends this action at once.
7. **Convert Coin.** Choose Matter, Mind, or Spirit.
8. **Resign.** Type `yes` to confirm.
9. **Cast Spell.** Choose a hand card. Add zero to four ordered targets: Main
   is 1; Bench 1, 2, and 3 are 2, 3, and 4. Repeated targets stay in the
   list. Enter 5 when done, 6 to undo the last target, or 7 to clear targets.
   Then choose Matter, Mind, Spirit, or No hint.
10. **Activate Skill.** Choose your Main or Bench position. Choose a numbered
    Skill. Add zero to four ordered targets with the same target form as Cast
    Spell. Repeated targets stay in the list. Then choose Matter, Mind,
    Spirit, or No hint.

When a pending choice belongs to you, the client shows `Awaiting ...`. It does
not print option lines for this forced form. Enter 1 for Matter, 2 for Mind,
or 3 for Spirit for Mana production. Enter 1, 2, or 3 for Bench 1, 2, or 3
for Promotion. For Prize selection, enter a one-based Prize number. The engine
checks each submitted action.

## Local commands

These commands do not submit a game action:

```text
inspect <hand index>
inspect board <player> <position>
history
cancel
```

Hand indexes start at one. In `inspect board`, `<player>` is `1` or `2`.
Accepted `<position>` numbers are 1 for Main, 2 for Bench 1, 3 for Bench 2,
and 4 for Bench 3. Board inspection returns the top card of the visible
summon chain. An invalid inspection prints `Invalid inspection`.

`history` prints notices this client already received, in delivery order. It
is local history. It does not query the host. Notices preserve private card
information: a player can receive a card name for that player's own draw,
prize view, or prize recovery, while the opponent receives a redacted notice.

`cancel` leaves the current guided form and prints the action menu. It does
not undo a submitted action.

To resign deliberately at any prompt, type `give up`, then type `yes` on the
next line. Any other next line cancels that confirmation.

## Transcript and replay

The host creates the transcript before clients join. A completed match writes
the terminal records and can be verified by `replay`. A graceful interruption
retains recorded bytes but writes no completion record. A crash or a changed
file can also be partial or invalid. Replay is authoritative: it accepts only
a valid completed transcript. Start a new host with a new output file to play
again.

## Common failures

- `cannot read deck` or `deck ... is invalid`: check both Deck paths and Deck
  content. `serve` requires two Deck paths.
- `cannot create output`: create the output parent first. Pick a new explicit
  filename when the old file exists.
- `cannot bind`: select an unused port, or omit `--port` and use the printed
  assigned port.
- `cannot connect`: start the host first. Check the join host and port. Use a
  real host address when the host binds `0.0.0.0`.
- `Waiting`: the client joined. Start the other client with the other seat.
- `seat is occupied`: that seat already has a client. Use the other seat, or
  stop the existing client before starting a replacement.
- `Prompt cancelled`, `Discarded stale input`, or `Rejected ...`: read the
  newest board and action menu, then enter a new choice.
- `transcript is invalid`: the file may be partial or changed. Finish a match
  and replay its completed output file.
