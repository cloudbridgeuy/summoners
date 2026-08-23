# Summoners context

## Status

A deterministic, pure game engine exists in `crates/core`. It enforces turn
structure, Mana, the board, Combat, the Stack, Spells, Skills, Triggered
Abilities, destruction, Prize recovery, promotion, and loss conditions for the
rules described below. The engine reads no file, calls no network, uses no
clock, and uses no random source. Every entry point takes one state value and
one action, and returns a new state value; it never mutates anything the caller
still holds. A strict authored-card boundary parses caller-held Set bytes into
core definitions without file I/O. A separate CLI serves a local museum image
search form. It connects to the Art Institute of Chicago; the other four
provider connections are not available yet. There is no game client or runtime
file loader yet. Detail pages include a trusted self-contained JPEG download
path for provider image responses. This file is an index of stable product
language, not an API contract.

## Behavior

### Requirement: Local museum image search

The `cceroby search` command parses a non-empty query, one or more of five
museum sources, an optional culture or region, an output directory, and local
server options before it starts a loopback-only server on an operating-system
assigned port. The local page keeps valid query and filter values in its form.
The Art Institute of Chicago search keeps only records that the response marks
as public domain, and it shows normalized result cards. Metadata responses stay
in the user cache for 24 hours. Card and preview image bytes stay in a separate
user cache for 30 days. Cards use local image and detail routes that accept only
a known source and object ID. The detail route reconstructs trusted provider
metadata and shows a local-proxy preview, normalized metadata, the accepted
license, and the ready-to-print attribution. Provider slots that do not have a
connection return an unavailable notice, and a connected-provider failure
returns one failure notice without stopping the page. A corrupt metadata or
thumbnail cache entry degrades to a cache miss instead of stopping the search.
Startup removes expired and corrupt cache entries on a best-effort basis. The
cache commands report the resolved cache path and separate fresh and expired
file counts and byte totals for metadata and thumbnails, or remove only the
Cceroby cache root. Cache inspection does not follow symbolic links.
The detail page accepts an editable safe file name and free-form tags. A
download requires the Host and Origin to match the exact authority assigned to
the loopback listener. It strictly parses the complete form before provider I/O
and reconstructs all policy and remote-request data from the provider, keeps
native JPEG scan data, converts TIFF input once, embeds attribution and tags as
standard XMP, and atomically writes one JPEG in the selected output directory.
Each complete search or detail page holds one local event stream while its tab
is open. Without `--serve`, the server stops after a same-authority POST quit
request or after the last connected tab stays disconnected for 10 seconds. It
does not stop before a page has connected, and a reconnect restarts the grace
period. With `--serve`, browser quit and disconnect events do not stop the
server. Ctrl-C stops either mode and closes open event streams cleanly.

#### Scenario: A valid local search starts

- **WHEN** a user starts a search with a valid query, source set, and output
  directory
- **THEN** the CLI reports a `127.0.0.1` URL with an assigned port
- **AND** the root page contains the initial query, sources, culture or region,
  and search control
- **AND** the page contains the initial search results and provider notices

#### Scenario: An Art Institute search succeeds

- **WHEN** the Art Institute of Chicago returns matching public-domain records
- **THEN** the page shows the loaded result count
- **AND** each accepted record has a locally proxied thumbnail, detail link,
  title, institution, and public-domain license badge
- **AND** a record that is not public domain does not appear

#### Scenario: A user opens one artwork

- **WHEN** the user follows a result card's detail link
- **THEN** the server reconstructs the artwork from its known source and object
  ID through the provider and metadata cache
- **AND** the page shows a full local-proxy preview, title, institution, source
  ID, accepted license, source object, and ready-to-print attribution
- **AND** creator, date, and culture or region appear when available
- **AND** the Back link returns to the accumulated search results

#### Scenario: An artwork route receives untrusted input

- **WHEN** a thumbnail, preview, or detail request has an unknown source,
  malformed object ID, duplicate field, unrecognized field, or remote URL
- **THEN** the server rejects the request with a short error
- **AND** it does not fetch the browser-provided remote URL

#### Scenario: A trusted artwork download succeeds

- **WHEN** the user submits the source, object ID, safe file name, and optional
  comma- or newline-separated tags from a detail page
- **THEN** the server reconstructs the artwork, license, exact attribution, and
  best image request from trusted provider data
- **AND** the full image request uses the provider rate limit and headers
  without writing the response to the metadata or thumbnail cache
- **AND** the server writes `<output>/<file-name>.jpg` with the attribution and
  ordered, de-duplicated tags in standard XMP
- **AND** the detail page reports the created path

#### Scenario: A download target exists

- **WHEN** the user downloads the same safe file name again
- **THEN** a durable same-directory temporary write atomically replaces the
  existing JPEG
- **AND** the detail page reports that it replaced the path
- **AND** no metadata sidecar remains
- **AND** two concurrent writes for the same new name report one creation and
  one replacement and leave one complete JPEG

#### Scenario: A downloaded image is JPEG or TIFF

- **WHEN** the provider returns a native JPEG
- **THEN** XMP embedding preserves its scan data and all non-XMP marker data
- **WHEN** the provider returns TIFF data
- **THEN** the server converts it once to JPEG at quality 100 before it uses the
  same XMP and atomic-write path
- **WHEN** any metadata or tag contains a code point that XML 1.0 does not
  permit
- **THEN** XMP construction returns a typed error before a JPEG is written

#### Scenario: A download form contains untrusted fields

- **WHEN** a download form contains an unknown, duplicate, missing, malformed,
  path, remote URL, license, or attribution value
- **THEN** the server rejects the untrusted input or shows a short typed error
- **AND** browser data cannot select a remote request or output path
- **WHEN** the form has malformed percent or UTF-8 encoding, or its Host and
  Origin do not exactly match the authority assigned to the loopback listener
- **OR WHEN** a hostile Host and Origin match each other but not that assigned
  authority
- **THEN** the server rejects it before provider or storage I/O

#### Scenario: A thumbnail is still fresh

- **WHEN** the local proxy requests the same provider-derived image less than
  30 days after a successful image response
- **THEN** it uses the cached image bytes without a second image request

#### Scenario: A thumbnail cache entry is corrupt or expired

- **WHEN** a thumbnail cache entry is malformed or reaches its 30-day boundary
- **THEN** the proxy treats the entry as a cache miss and removes it on a
  best-effort basis
- **AND** the provider image request can continue

#### Scenario: Search form values change

- **WHEN** a user submits a non-empty query and source set from the local page
- **THEN** the page keeps the submitted query and filters
- **AND** each selected source without a connection has an unavailable notice

#### Scenario: A metadata response is still fresh

- **WHEN** the user repeats the same Art Institute of Chicago search less than
  24 hours after a successful metadata response
- **THEN** the page uses the cached metadata without a second provider request

#### Scenario: A metadata cache entry is corrupt

- **WHEN** a metadata cache entry is malformed or has a time that the system
  cannot represent
- **THEN** the search treats the entry as a cache miss and removes it on a
  best-effort basis
- **AND** the provider request can continue

#### Scenario: A user inspects or clears the cache

- **WHEN** the user runs `cceroby cache info`
- **THEN** the command reports the resolved cache path
- **AND** it reports fresh and expired file counts and byte totals separately
  for metadata and thumbnails
- **WHEN** the user runs `cceroby cache clear`
- **THEN** the command reports the number of removed files
- **AND** on Apple platforms, Linux, and Android it atomically detaches and
  removes only the Cceroby cache root without following symbolic links
- **AND** a cache root that a writer creates after the detach operation remains
- **AND** a permission, inspection, count, or removal error makes the command
  fail without a success report
- **AND** a missing or disabled cache is a safe no-op
- **AND** on a platform without the required atomic no-replace rename, the
  command refuses removal

#### Scenario: Startup prunes old cache entries

- **WHEN** a search starts with expired or corrupt metadata or thumbnails
- **THEN** on Unix startup removes those entries on a best-effort basis with
  handle-relative operations that do not follow symbolic links
- **AND** a cache I/O error does not stop normal search work
- **AND** on a platform without safe handle-relative cache operations, normal
  search work continues without cache reads, writes, or pruning

#### Scenario: A connected provider fails

- **WHEN** a selected connected provider cannot complete its search
- **THEN** the page shows one failure notice for that provider
- **AND** the page remains available with an empty result set

#### Scenario: Command input is invalid

- **WHEN** a user supplies an empty source set or an output path that is not an
  available directory
- **THEN** the command reports the input error before it starts the server

#### Scenario: A browser controls a transient server

- **WHEN** the first search or detail page connects to the local event stream
  and all connected pages then stay disconnected for 10 seconds
- **THEN** the server completes graceful shutdown
- **WHEN** a page reconnects during the 10-second grace period
- **THEN** the server stays available and restarts the full grace period after
  the next disconnect
- **WHEN** no page has connected
- **THEN** the disconnect timer does not stop the server
- **WHEN** the user presses Esc on a search or detail page
- **THEN** the page sends a POST quit request with the same exact Host and
  Origin authority required by trusted downloads
- **AND** a missing, hostile, or mismatched authority cannot request shutdown

#### Scenario: The user keeps or stops the local server

- **WHEN** the local server receives Ctrl-C
- **THEN** it completes graceful shutdown and the command exits
- **WHEN** the user starts the search with `--serve`
- **THEN** tab disconnects and browser quit requests do not stop the server
- **AND** Ctrl-C remains available

### Requirement: Complete local quality gate

The `cargo xtask lint` command runs formatting, Cargo check, Clippy, tests,
file-length checks, and banned-pattern checks in that order. It stops after the
first failed check and returns a failure status.

#### Scenario: All checks pass

- **WHEN** a developer runs `cargo xtask lint` and all six checks pass
- **THEN** the command reports all six checks as successful and returns a
  success status

#### Scenario: One check fails

- **WHEN** a check fails during `cargo xtask lint`
- **THEN** the command returns a failure status without running later checks

### Requirement: Safe staged repair and hook ownership

Staged repair refuses a Rust path that has both staged and unstaged changes
before it changes the index or worktree. Hook removal removes only the exact
managed pre-commit hook and leaves any unmanaged hook unchanged.

#### Scenario: A Rust path is partially staged

- **WHEN** staged repair finds a Rust path with staged and unstaged changes
- **THEN** it reports the conflict and leaves the index and worktree unchanged

#### Scenario: A pre-commit hook is unmanaged

- **WHEN** hook removal finds a pre-commit hook that is not the exact managed
  hook
- **THEN** it reports that the hook is unmanaged and leaves it unchanged

### Requirement: Strict authored Set loading

A **Set** byte buffer uses one exact schema version and converts to core card
definitions only after strict document and semantic parsing. Stable Set, card,
and ability codes determine UUID-v5 identities; revision, display text, and
document order do not determine them.

#### Scenario: A valid Set is parsed

- **WHEN** a caller passes the Foundations Set bytes to the Set parser
- **THEN** it receives 20 core card definitions with their printed statistics,
  costs, abilities, effects, modifiers, timing, persistence, and response modes

#### Scenario: A Set is malformed

- **WHEN** a Set contains invalid UTF-8, a missing or unsupported schema
  version, an unknown field, an invalid stable code, an invalid semantic
  combination, or a duplicate code
- **THEN** parsing fails before caller-visible core conversion with a typed
  phase, schema version when known, stable path, and cause

#### Scenario: A positional effect keeps its authored selector

- **WHEN** a Set effect selects an own Summon or refers to its battlefield
  source
- **THEN** its core effect leaf keeps the matching Selected or Source target
- **AND** a Spell or Enchantment cannot use Source because it has no
  battlefield position

### Requirement: Strict Deck loading and one built-in catalog

A Deck byte buffer declares exact Set revisions and qualified stable card
references. The card boundary resolves one separate Base Starter and an
expanded 20-card body. It rejects unknown schema data, unavailable or wrong
Set revisions, unresolved references, duplicate Set requirements, a Starter
in the body, a non-Base Starter, a wrong body total, and more than two copies
of one definition. The embedded Foundations Set and both Decks use this same
public byte path once, and all callers share the cached catalog and core card
pool.

#### Scenario: A valid Deck is resolved

- **WHEN** a caller passes either built-in Deck byte buffer and the Foundations
  library to the Deck parser
- **THEN** it receives the correct separate Base Starter and 20 body cards
- **AND** the body contains ten definitions with two copies of each

#### Scenario: A Deck is malformed or cannot resolve

- **WHEN** a Deck has malformed schema data, an invalid stable reference, a
  missing or wrong Set revision, an unresolved card, or an illegal construction
- **THEN** parsing fails with a typed document kind, phase, schema version,
  stable path, and cause

#### Scenario: The built-in catalog is requested more than once

- **WHEN** callers request the built-in catalog and its core card pool again
- **THEN** each caller receives the same cached catalog allocation and core
  card-pool allocation

### Requirement: Turn structure and phase order

A turn moves through Upkeep, Main Phase, and Combat, and phases only move
forward (rules §9). Ending a turn opens one last Combat Priority window with
the defender first; once both players have passed and the Stack is empty, the
turn hands off completely in one step: every Summon the new active player
controls becomes Ready, they draw one card, Mana production runs for the
player and every Summon they control, and their own Main Phase begins before
either player can act again (rules §9–10, §47–48).

#### Scenario: A turn hands off after both players pass

- **WHEN** the active player ends their turn and both players pass Priority
  once each with the Stack empty
- **THEN** the turn hands to the opponent, who is Readied, draws one card,
  produces Mana, and reaches their own Main Phase before either player can
  act again

#### Scenario: A new turn reaches a Main Phase a player can act in

- **WHEN** a second turn's handover finishes
- **THEN** the new active player can play a Base Summon from hand, because
  their Main Phase — not just their Upkeep — has genuinely begun

### Requirement: Mana, the Coin, and paying costs

Every Mana is typed Matter, Mind, or Spirit; there is no separate Generic
pool (rules §11–12). During Upkeep, the player's own natural production and
every Summon they control each generate Mana, choosing among the Types
anchored to the player's board when more than one is available, and pausing
for that choice when it is. The second player's one-use Coin converts to one
Mana of a Type their board already produces, and is then removed from the
game (rules §7). Paying a cost spends typed components from their matching
pool first; a Generic component is paid from a named pool or, without one,
from the largest remaining pool (rules §11–12, §50). Mana produced by an
effect belongs to that effect's controller, even when another player is
active, and a required Mana Type choice remains assigned to that controller.

#### Scenario: A dual-type board pauses for a Mana Type choice

- **WHEN** a player's board anchors more than one Mana Type during Upkeep
- **THEN** natural production pauses until the player chooses which Type to
  add, and no Mana is banked until they answer

#### Scenario: The Coin converts to one Mana and cannot be reused

- **WHEN** the second player converts their Coin for a Type their board
  produces
- **THEN** the Coin leaves the game, and converting again is rejected

#### Scenario: A non-active player's effect produces Mana

- **WHEN** a Trigger controlled by the non-active player produces Mana
- **THEN** the Mana is banked for that Trigger's controller, and any Mana
  Type choice waits for that same player

### Requirement: Playing, upgrading, and retreating Summons

Playing a Base Summon fills an empty Bench slot and it enters Exhausted
(rules §14, §17). Upgrading a Summon must strictly climb Base, Enhanced,
Elite, and the new top must still print every Mana Type the current top
prints (rules §18–20). Retreating pays the printed Retreat Cost — raised by
any opposing card that says opposing Retreats cost more — and exchanges Main
with a chosen Bench Summon, firing all four movement triggers in a fixed
order (rules §26, §28).

#### Scenario: An upgrade must climb Form and keep every printed Type

- **WHEN** a proposed upgrade would drop a Mana Type the current top already
  prints
- **THEN** the upgrade is rejected as an illegal upgrade target

#### Scenario: Retreating fires movement triggers in a fixed order

- **WHEN** a player retreats, exchanging Main with a Bench Summon
- **THEN** Leaving Main, Entering Bench, Leaving Bench, and Entering Main
  fire in that order, and any immediate Triggered Ability on the arriving
  Summon resolves as part of the same action

### Requirement: Combat, the Stack, and Priority

Declaring the turn's one normal attack pays the printed Attack cost, puts the
attack on the Stack, and gives the defending player Priority first (rules
§29–31, §46). Priority moves to the other player on every Spell played or
every pass; two consecutive passes close the current Priority window, and if
the Stack is then completely empty the turn hands off immediately (rules
§32–33, §47). Whatever sits on top of the Stack resolves first, so a response
played after an attack resolves before that attack does (rules §35).

#### Scenario: A declared attack resolves after both players pass once

- **WHEN** a player declares their normal attack and both players pass
  Priority once each
- **THEN** the attack resolves, applying Damage to the defending Main

#### Scenario: A response resolves before the attack it answered

- **WHEN** the defender casts a legal Attack Spell while holding Priority
- **THEN** the Spell resolves first, and only afterward does the original
  attack resolve

### Requirement: Spells and Enchantments

A Support Spell may be cast proactively in its own caster's resting Main
Phase, or as a response while its caster holds Priority; an Attack Spell may
only be cast as a response (rules §34, §45–46). Casting pays the printed
cost, puts the card on the Stack, and opens a Priority window. A resolved
Spell moves to its caster's discard pile; a resolved Enchantment instead
remains in play until something removes it (rules §44, §56). A card's own
printed Attack effect can block Attack Spell responses to it under a stated
condition. Each player has independent Spell-play history for the current
turn. A response block reads the nearest unresolved Attack in the current
Stack segment, even when Support Spells sit above it, and never reads an
outer or completed Attack.

#### Scenario: A Support Spell heals through the Stack

- **WHEN** a Support Spell that heals is cast in its caster's own Main Phase
  and both players pass
- **THEN** it resolves, healing the named target, and it moves to the
  caster's discard pile

#### Scenario: An Enchantment persists through a turn handover

- **WHEN** an Enchantment resolves
- **THEN** it stays in play rather than discarding, and it is still in play
  after a full turn hands off to the opponent

#### Scenario: Spell history and response blocks stay local

- **WHEN** one player casts a Support Spell above a protected Attack
- **THEN** only that caster's Spell-play condition becomes true, and the
  protected Attack remains visible within its current Stack segment

### Requirement: Source-aware Damage resolution

Each Damage effect groups its base amount, conditional additions, and semantic
constraints. The engine evaluates one immutable Damage intent in the fixed
order Addition, Persistent Reduction, Clamp, and Commit. It then changes the
target's Damage once and queues destruction work. Every calculation emits an
ordered, flat event trace whose lines repeat the exact source and target.
Sources distinguish an Attack, Spell card instance, Skill ability, and Trigger
ability.

Standing Ward reduces an opposing Attack's combined Damage by 10 for each Ward
in play. It does not reduce Spell, Skill, or Trigger Damage. `Unpreventable`
skips each Ward reduction, and `Unincreasable` skips each conditional addition.
A reduction cannot make the running total less than zero.

#### Scenario: Conditional Damage passes through two Wards

- **WHEN** a true conditional addition combines with an Attack's base Damage
  while the defender controls two Standing Wards
- **THEN** the addition applies first, each Ward reduces the combined total in
  play order, the total clamps at zero or more, and one final Damage amount is
  committed to the target

#### Scenario: A constraint skips each blocked adjustment

- **WHEN** an unpreventable Attack meets two Standing Wards
- **THEN** the event trace contains one skipped persistent-reduction line for
  each Ward and the full Attack Damage is committed

- **WHEN** an unincreasable Damage effect has a true conditional addition
- **THEN** the event trace contains a skipped addition line and commits the
  base Damage without that addition

#### Scenario: Damage events identify their complete calculation

- **WHEN** Damage resolves from an Attack, Spell, Skill, or Trigger
- **THEN** its calculation events identify the exact source, the target, each
  applied or skipped operation, its origin and stage, the running input and
  output or skip constraint, and the final before-and-after values

### Requirement: Skills

Activating a Skill requires the Summon to be Ready. Activation checks
Readiness, pays the cost, exhausts the Summon, then resolves the Skill's
effect, always in that order (rules §15). No currently printed Skill uses the
Stack, so every Skill resolves immediately (rules §43). An effect authored
to act on its source uses the activating or triggering Summon's position and
cannot be redirected through an action's selected target.

#### Scenario: Exhaustion happens before the Skill's effect

- **WHEN** a Summon activates a Skill
- **THEN** the Summon becomes Exhausted before the effect resolves, even when
  the effect itself would otherwise re-Ready it

#### Scenario: A protective Skill blocks an opposing Skill until it expires

- **WHEN** a Skill protects a Summon from being moved by the opponent until
  its controller's next turn
- **THEN** an opposing Skill that would move it is rejected for the whole of
  the opponent's next turn, and becomes legal again only once the
  controller's following turn begins

#### Scenario: A self effect ignores a redirected selected target

- **WHEN** a source-targeted heal and movement protection resolves with a
  different friendly Summon in the selected-target list
- **THEN** both effects apply to the source Summon

### Requirement: Triggered abilities

A Triggered Ability fires on its stated event, discovered opponent-of-active-
player first and Main before Bench within a player (rules §28, §36–41). A
respondable Trigger opens a new Priority window for the opponent of its
controller before anything already queued behind it continues; a
non-respondable Trigger applies immediately, with no window opening for it
(rules §37–39).

#### Scenario: A respondable trigger interrupts a chain already in progress

- **WHEN** a respondable Triggered Ability fires in the middle of an
  already-queued destruction chain
- **THEN** a new Priority window opens before the rest of that chain runs,
  and the chain resumes only once the window closes

#### Scenario: An immediate trigger resolves without opening a window

- **WHEN** a non-respondable Triggered Ability, such as a Summon healing on
  entering Main, fires
- **THEN** it resolves as part of the same action, and no Priority window
  opens for it

### Requirement: Destruction, Prize recovery, and promotion

A Summon is destroyed once its Damage reaches its Life (rules §21, §23). A
destroyed Main Summon's whole upgrade chain discards, its owner records one
Main loss, they recover one Prize Card if any remain (the opponent chooses
which face-down card), a Bench Summon promotes into the empty Main if any
remain (the owner chooses which when more than one is available), and only
then do the remaining movement consequences and a losing-condition check run
— always in that order (rules §24–25, §28). A destroyed Bench Summon skips
straight to discarding and a losing-condition check.

#### Scenario: A lethal attack destroys, recovers a Prize, and promotes

- **WHEN** an attack finishes a Main Summon's Life and its owner has both a
  Prize Card and a Bench Summon available
- **THEN** the chain discards, one Main loss is recorded, the opponent picks
  which Prize Card is recovered, the owner picks which Bench Summon is
  promoted, and the promoted Summon occupies Main once the chain settles

#### Scenario: One event can destroy both players' Mains at once

- **WHEN** a single action's Damage finishes both players' Main Summons
  together
- **THEN** the opponent of the active player's whole destruction chain
  resolves before the active player's own chain begins (rules §41–42)

### Requirement: Loss conditions and immediate ending

The game ends the moment any losing condition is met: a third Main loss, no
Bench Summon available to promote into an empty Main, or an attempted draw
from an empty Deck (rules §2, §10, §24, §58). A third Main loss ends the game
even if another losing condition is pending at the very same moment, and once
the game has ended every later action is rejected. When an effect requires
more cards than remain in the Deck, the player draws every available card in
order, then loses before any later effect leaf resolves. Drawing exactly the
final available card succeeds and does not cause an early loss.

#### Scenario: A third Main loss ends the game outright

- **WHEN** a player's third Main Summon is destroyed, even with a Bench
  promotion still queued behind it
- **THEN** the game ends immediately in the opponent's favor, and the queued
  promotion never resolves

#### Scenario: An empty-deck draw ends the game before Mana production

- **WHEN** a player must draw from an empty Deck during Upkeep
- **THEN** the game ends immediately, and that Upkeep's Mana production never
  runs

#### Scenario: A required multi-card effect draw cannot complete

- **WHEN** an effect requires more cards than remain in the player's Deck
- **THEN** every available card is drawn and reported first, the player then
  loses for draw failure, and no later effect leaf resolves

#### Scenario: A required draw takes the final available card

- **WHEN** an effect requires exactly the number of cards left in the Deck
- **THEN** the draw succeeds and emptying the Deck alone does not end the game

## Sources

- `designs/core_rules.md` defines the current prototype rules and their open areas.
- `designs/core_rules.docx` is the source document retained with the Markdown
  rules. The Markdown file supersedes the `.docx` where they differ; the
  2026-08-10 Mana rewrite exists only in the Markdown.
- `designs/types_archetypes.md` defines card-design identities and balance intent. It is not a rules document.
- `designs/rust_c_engine_architecture.md` describes the intended boundary
  between an authoritative Rust engine and a future C/Raylib application. No
  such application exists yet; `crates/core` is the Rust half this document
  describes.

When the documents overlap, use the core rules for engine behavior and the
types/archetypes document for content-design intent.

## Core language

- **Player / Summoner:** one of the two competitors.
- **Summon:** a creature card. A Summon has an Owner and can have a different Controller.
- **Main:** the required active battlefield position. It cannot remain empty during play.
- **Bench:** up to three reserve positions whose Summons can still produce Mana and provide abilities.
- **Base / Enhanced / Elite:** the ordered Summon forms in an upgrade chain.
- **Ready / Exhausted:** the physical state that normally controls voluntary Skill activation.
- **Mana Pool:** persistent public resources owned by a player. All Mana is typed. Generic cost components accept Mana of any Type, while typed components require the named Type.
- **Coin:** the second player's one-use resource. It converts into one Mana of a Type their board already produces, then leaves the game.
- **Skill:** an ability a Ready Summon can voluntarily activate by paying its cost and becoming Exhausted.
- **Passive Ability:** a continuous effect that normally does not use the Stack.
- **Triggered Ability:** an automatic response to a stated event. It can resolve immediately or create a respondable Stack effect.
- **Spell:** a card played from hand for one use. A Support Spell may be played proactively or as a response; an Attack Spell only as a response.
- **Enchantment:** a card played from hand like a Spell, but it remains in play after resolving instead of discarding.
- **Priority:** the exclusive right to add one legal Spell to the Stack or pass.
- **Stack:** the last-in, first-out sequence of attacks and respondable effects.
- **Damage:** a resolved amount added to one Summon's existing Damage. Its
  calculation retains the printed source and battlefield target.
- **Damage constraint:** semantic text such as `Unpreventable` or
  `Unincreasable` that skips the matching adjustment without changing the
  running Damage total.
- **Prize Card:** one of two face-down comeback resources recovered after the first two Main losses.
- **Vault:** seven match-play cards outside the 20-card Deck.
- **Set:** one versioned authored document that owns card definitions and their
  stable identities.

## Important relationships

- One player controls one Main Summon and up to three Benched Summons.
- A Summon upgrade chain is one Summon; only the top card defines current characteristics, while Damage remains.
- Attacks target battlefield positions, not a specific Summon that can move away.
- Two consecutive passes close the current Priority window. If the Stack is
  then empty, the turn hands off; otherwise whatever sits on top of the Stack
  resolves next. A triggered respondable effect discovered along the way can
  open a fresh Priority window before that resolution continues.
- Destruction, forced promotion, movement triggers, other automatic triggers, and loss checks occur during resolution.
- Losing is immediate. Unresolved effects stop, and games do not end in a draw.
- The fixed movement-trigger order is Leaving Main, Entering Bench, Leaving Bench, Entering Main.
- Mana production anchors to the board: a player only ever chooses among the
  Mana Types the Summons they control actually print, never a Type they have
  no Summon for.
- Recovering a Prize Card and promoting a Bench Summon have opposite
  choosers: the opponent of the player recovering the Prize chooses which
  face-down card it is, but that player's own Bench decides who is promoted.

## Content-design language

- **Matter:** bodies, force, high Life, raw attack damage, and board presence.
- **Mind:** information, Priority, responses, and attack redirection.
- **Spirit:** healing, destruction, recursion, and life/death transitions.
- Mono-type cards buy rate; dual-type cards buy reach. This is a design guide, not an engine rule.

## Deliberately unsettled

The core rules list open balance and tournament areas. The archetype document
also leaves rate gaps, early dual gates, wording conventions, and Mana
stranding for later simulation and playtest. Do not encode these as settled
rules without a new decision.

The engine itself leaves further ground open that a reader should not
mistake for settled:

- Damage supports conditional additions, persistent reductions, constraints,
  and a zero clamp. It does not yet support scaling, replacement, redirection,
  or consumable shields.
- The design's "you may" wording on a few printed effects — returning a
  Spell from the discard pile, and the look-then-draw-then-return sequence on
  one Skill — is currently played out as an unconditional action. The engine
  has no vocabulary yet for an optional sub-effect a player can decline.
- One Skill's printed text offers a choice between two different effects
  (exchange the opposing Main with a Bench Summon, or move one opposing
  Benched Summon to another Bench position). Only the first branch exists in
  the engine.
- No printed Skill currently uses the Stack, so the rule that lets a Skill's
  own text put it there remains unexercised.
- `GameState` is one fully visible value with no hidden-information layer:
  a Prize Card, a face-down Deck, and an opponent's hand are all readable by
  anything that can see the state. Whatever hides them from a real opponent
  is future shell work, not something this crate does.
