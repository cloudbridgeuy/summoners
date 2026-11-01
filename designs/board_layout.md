# Board layout for the text client

Design input. Nothing here is implemented until `crates/cli` and its tests make
it true.

## Problem

The client prints three flat lines per update (`Hand`, `Board`, `Phase`) and a
ten-item menu. A player cannot see at a glance: turn owner, priority holder,
Mana pools, Prizes, Deck counts, Main losses, the Coin, the Stack, persistent
Enchantments, or remaining Life. Every one of these values already travels in
`PlayerView`.

## Constraints

- Plain text, 80 columns, monospace. No colour required; colour is optional.
- One full redraw per snapshot. Notices stay available through `history`.
- Orientation mirrors a table: opponent at the top, you at the bottom, Hand
  under your side. The Stack sits on the centre line.
- Card names in `foundations` are at most 20 characters. Life is 50, 100, or
  150. Damage is a multiple of 10.
- Show remaining Life as `💗 remaining/max` instead of raw Damage.

## Symbols

| Meaning  | Glyph | Code point | Cells | ASCII fallback |
| -------- | ----- | ---------- | ----- | -------------- |
| Life     | 💗    | U+1F497    | 2     | `L`            |
| Matter   | 🔨    | U+1F528    | 2     | `M`            |
| Mind     | 🧠    | U+1F9E0    | 2     | `N`            |
| Spirit   | ✨    | U+2728     | 2     | `S`            |
| Generic  | ⚪    | U+26AA     | 2     | `G`            |
| Coin     | 🪙    | U+1FA99    | 2     | `C`            |
| Chain    | ▲     | U+25B2     | 1     | `^`            |
| Priority | ▶     | U+25B6     | 1     | `>`            |

Spirit alternatives considered: 👻 reads as Halloween, 🔮 reads as
divination. ✨ reads as arcane force and is the recommendation.

Every emoji here is a single code point with `Emoji_Presentation=Yes`. Each
one occupies two terminal cells on every modern terminal. Sequences with
variation selectors (`❤️`, `🕊️`) are excluded: their width differs between
terminals. `▲`, `▶`, and the box-drawing characters share the ambiguous-width
class with `─` and `│`; a terminal that draws boxes correctly draws them at one
cell.

Width rules for the renderer:

- Pad by display width, never by character count. Count an emoji as two
  cells.
- Column alignment therefore works out the same on the screen even though the
  source lines below have unequal character counts.

Notation rules:

- A Mana pool reads `🔨 2  🧠 0  ✨ 1`: glyph, space, count. Zero counts stay
  visible so the three columns never move.
- A cost reads one glyph per Mana, typed first, generic last: `🔨⚪` is Matter
  1 plus Generic 1. `⚪` alone is Generic 1.
- A Summon's Mana Types read as a glyph run with no counts: `🔨🧠`.
- Life reads `💗 70/100`.
- A one-line legend sits under the header so a new player never has to guess.

## Layout A: boxed board (recommended)

```
 Turn 4 · Main phase · Active: Player Two · Priority: You
 💗 Life   🔨 Matter   🧠 Mind   ✨ Spirit   ⚪ Generic   🪙 Coin
────────────────────────────────────────────────────────────────────────────────
   Player Two (opponent)          Hand 4 · Deck 11 · Prizes 2 · Losses 0/3
   Mana  🔨 2  🧠 0  ✨ 1                                                🪙
   Enchant  Standing Ward

 Bench  ┌ 1 ──────────────────┐ ┌ 2 ──────────────────┐ ┌ 3 ──────────────────┐
        │ Quarry Scout     🔨 │ │                     │ │                     │
        │ 💗 40/50  ready     │ │        empty        │ │        empty        │
        └─────────────────────┘ └─────────────────────┘ └─────────────────────┘
 Main                     ┌ Main ───────────────────────┐
                          │ Warden Pathkeeper      🔨🧠 │
                          │  ▲ Warden Initiate          │
                          │ 💗 70/100  EXHAUSTED        │
                          └─────────────────────────────┘
──────────────────────────────── Stack: empty ──────────────────────────────────
 Main                     ┌ Main ───────────────────────┐
                          │ Warden Initiate        🔨   │
                          │ 💗 50/50  ready             │
                          └─────────────────────────────┘
 Bench  ┌ 1 ──────────────────┐ ┌ 2 ──────────────────┐ ┌ 3 ──────────────────┐
        │ Set Path Adept   🧠 │ │                     │ │                     │
        │ 💗 50/50  EXHAUSTED │ │        empty        │ │        empty        │
        └─────────────────────┘ └─────────────────────┘ └─────────────────────┘

 ▶ Player One (you)               Hand 5 · Deck 12 · Prizes 2 · Losses 0/3
   Mana  🔨 1  🧠 1  ✨ 0
────────────────────────────────────────────────────────────────────────────────
 Hand
   1  Warden Pathkeeper   Summon   Enhanced  💗100  🔨🧠
   2  Ember Lance         Spell    attack    cost 🔨⚪
   3  Scrying Glass       Spell    support   cost ⚪
   4  Quarry Scout        Summon   Base      💗50   🔨
   5  Standing Ward       Enchant            cost 🧠
────────────────────────────────────────────────────────────────────────────────
   1 Play Summon   2 Upgrade   3 Retreat   4 Attack   5 End Turn
   6 Pass          7 Coin      9 Cast      10 Skill   8 Resign
 >
```

Vertical budget: 35 lines with an empty Stack and a five-card Hand.

Rules of the picture:

- `▶` marks the priority holder. The header names the active player.
- A Summon box shows the top of the chain, then each lower layer with `▲`.
  The top line's right edge shows the current form's Mana Types.
- `EXHAUSTED` is upper case, `ready` is lower case, so state reads without
  colour.
- `🪙` sits at the right of the second player's Mana line while the Coin
  exists.
- `Enchant` lists persistent cards. The line is absent when the list is empty.
- Discard is a count only: `Discard 3` on the player line when non-zero.

## Layout A with a live Stack and a pending choice

```
──────────────────────────────── Stack (top resolves first) ────────────────────
   2  Trigger  Player One  Standing Ward · enters Bench · -10 attack damage
   1  Attack   Player Two  Warden Pathkeeper → Player One Main
────────────────────────────────────────────────────────────────────────────────
```

```
 Awaiting you: choose Mana type   1 🔨 Matter   2 🧠 Mind   3 ✨ Spirit
 >
```

```
 Awaiting Player Two: Promotion
 >
```

The pending line replaces the action menu. It always names the owner.

## Layout B: compact rows

For terminals under 60 columns, or as a `--compact` flag.

```
 T4 Main · active P2 · priority YOU
 P2 opp   H4 D11 P2 L0/3   🔨2 🧠0 ✨1   🪙
   Main   Warden Pathkeeper ▲1  🔨🧠  💗70/100  EXH
   B1     Quarry Scout          🔨    💗40/50   rdy
   B2     —
   B3     —
 ── stack: empty ──
 P1 you   H5 D12 P2 L0/3   🔨1 🧠1 ✨0
   Main   Warden Initiate       🔨    💗50/50   rdy
   B1     Set Path Adept        🧠    💗50/50   EXH
   B2     —
   B3     —
 Hand
   1 Warden Pathkeeper  Enh 💗100 🔨🧠
   2 Ember Lance        Spell atk 🔨⚪
   3 Scrying Glass      Spell sup ⚪
   4 Quarry Scout       Base 💗50  🔨
   5 Standing Ward      Ench      🧠
 1 Play 2 Upgr 3 Retreat 4 Attack 5 End 6 Pass 7 Coin 8 Resign 9 Cast 10 Skill
 >
```

Fifteen lines for the board. `▲1` is the number of lower chain layers.
`H D P L` are Hand, Deck, Prizes, Losses.

## Glyph tiers

The client picks one tier at start and never mixes tiers on one screen.

| Tier    | Life | Matter | Mind | Spirit | Generic | Coin | Chain | Priority |
| ------- | ---- | ------ | ---- | ------ | ------- | ---- | ----- | -------- |
| emoji   | 💗   | 🔨     | 🧠   | ✨     | ⚪      | 🪙   | ▲     | ▶        |
| symbols | ♥    | ■      | ◆    | ✦      | ○       | ●    | ▲     | ▶        |
| ascii   | L    | M      | N    | S      | G       | C    | ^     | >        |

`--glyphs auto|emoji|symbols|ascii`, default `auto`. `auto` selects `emoji`
when `LC_ALL`, `LC_CTYPE`, or `LANG` names UTF-8 and `TERM` is not `linux` or
`dumb`; otherwise `ascii`. No terminal reports emoji font support, so
`symbols` is never auto-selected. It is the manual answer for a UTF-8
terminal without an emoji font. The `ascii` tier also replaces box-drawing
with `+-|`.

## Client becomes a TUI

The line prompt cannot show a board, a card detail, and an input line at the
same time. The client moves to `ratatui` with the `crossterm` backend.

What stays:

- `prompt.rs` is a pure reducer from typed line to `PromptState` plus
  effects. The TUI feeds it one submitted line at a time. Its tests stay.
- The receive thread, `Snapshot`, revision tagging, stale-input discard, and
  the `history` store stay. `history` becomes the Log pane.
- Every text command stays: numbers, `inspect`, `history`, `cancel`,
  `give up`, `yes`. A new `cards` command opens the Cards screen.

What changes:

- Input arrives as `crossterm` key events on a second channel. The main loop
  waits on the snapshot channel and the key channel and redraws after either.
- `Screen` is a closed enum: `Board`, `Cards { focus: CardRef }`.
  `CardRef` is a closed enum: `Board { seat, position }` or `Hand { index }`.
- Keys outside the input line: `Tab` toggles Board and Cards. In Cards,
  `Up`/`Down` or `j`/`k` move focus, digits jump, `Esc` returns.
  `Ctrl-C` stops the client, which is not a resignation.
- Under 80 by 35 cells the Board screen draws Layout B. Under 60 columns it
  draws the Log and the prompt only, with a `too small` notice.
- Rendering pads by display width through `unicode-width`, which `ratatui`
  already depends on.
- Golden tests use `ratatui::backend::TestBackend` and compare buffers.

New dependencies: `ratatui`, `crossterm`, `unicode-width`.

## Board screen as ratatui blocks

Vertical `Layout`, top to bottom, on an 80 by 35 terminal:

| Block    | Rows | Content                                              |
| -------- | ---- | ---------------------------------------------------- |
| Header   | 2    | turn, phase, active, priority; legend                |
| Opponent | 3    | counts line, Mana line with Coin, Enchant line       |
| Bench    | 4    | three boxes                                          |
| Main     | 5    | one box                                              |
| Stack    | 1+n  | centre rule; one row per entry                       |
| Main     | 5    | one box                                              |
| Bench    | 4    | three boxes                                          |
| You      | 2    | counts line, Mana line                               |
| Hand     | 1+n  | one row per card                                     |
| Prompt   | 2    | menu or awaiting line; input line with cursor        |
| Log      | rest | newest notice last; `Rejected` and `Stopped` here    |

The Board picture in Layout A above is the target buffer for that size.

## Cards screen

Opened by `cards`, `Tab`, or `inspect`. It shows every visible card: both
boards and your Hand. Master list on the left, one full card on the right.
The list is the same order as the Board picture, top to bottom.

```
 Cards                                     Tab Board   j/k move   Esc back
────────────────────────────────────────────────────────────────────────────────
 Player Two            ┌ Warden of Set Paths ───────────────────────────────┐
   Main  Warden of Se… │ Summon · Elite · 🔨🧠                              │
   B1    Quarry Scout  │ 💗 150   Retreat ⚪⚪⚪                            │
   B2    —             │                                                    │
   B3    —             │ Rearrange the opposing formation, increase         │
 Player One            │ opposing Retreat costs, and punish a defender      │
 ▶ Main  Warden Initi… │ that entered Main this turn.                       │
   B1    Set Path Adept│                                                    │
   B2    —             │ Skill    Rearrange           🔨🧠                  │
   B3    —             │          swap positions                            │
 Hand                  │ Passive  Bar the Way                               │
   1  Warden Pathkeeper│          opposing retreat cost +1                  │
   2  Ember Lance      │ Attack   Closed Path Strike  🔨⚪⚪                │
   3  Scrying Glass    │          deal 50 damage, +30 if defender entered   │
   4  Quarry Scout     │          Main this turn                            │
   5  Standing Ward    │                                                    │
                       │ In play  💗 120/150 · EXHAUSTED · owner Player Two │
                       │ Chain    ▲ Warden Pathkeeper  ▲ Warden Initiate    │
                       └────────────────────────────────────────────────────┘
 >
```

Rules of the picture:

- The list column is 22 cells. A longer name truncates with `…`.
- `▶` marks the focused row. The right panel always shows the focused card.
- The panel wraps the authored rules text at the panel width.
- Each ability row shows kind, name, and cost. Its effect lines sit under it.
- `In play` appears for Board cards only. It shows remaining Life, readiness,
  and the owner when the owner is not the controller.
- `Chain` lists lower layers, newest first, for Board cards only.
- A Spell or Enchantment panel shows `Spell · attack · cost 🔨⚪` on line
  one, then the rules text, then its effects.
- The input line stays live. A submitted action from this screen returns to
  Board.

## Wire additions

`CardDescription` today has no card kind, form, authored rules text, or
ability names. The Cards screen needs all four.

| Field     | Type            | Source                                    |
| --------- | --------------- | ----------------------------------------- |
| `kind`    | enum            | authored `kind`: summon, spell, enchantment |
| `form`    | optional enum   | authored `form`: base, enhanced, elite    |
| `text`    | string          | authored `text`                           |
| ability `name` | string     | authored ability `code`, title-cased      |

`crates/cards/src/v1/convert.rs` drops `text` today. The Set conversion must
keep it as a core `Text` component or a side table on `CardDescriptions`.
Mana Types and cost already travel as strings; the client maps them to
glyphs.

## Decisions taken

1. Life shows as `remaining/max`. Inspection shows raw Damage as well.
2. Full-screen redraw is the TUI. No scrolling separator mode.
3. Menu numbering stays stable. The two on-screen rows are a display order.
4. Actions stay all visible. The engine is the only legality judge.

## Open

1. Whether a phase-illegal action dims. Needs a client-side legality hint
   that the engine does not export today.
2. Whether `text` lives in core as a component or only in the cards crate.
