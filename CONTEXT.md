# Summoners context

## Status

The repository is at bootstrap. No game rule is implemented yet. This file is
an index of stable product language from the design inputs, not an API contract.

## Behavior

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

## Sources

- `designs/core_rules.md` defines the current prototype rules and their open areas.
- `designs/core_rules.docx` is the source document retained with the Markdown
  rules. The Markdown file supersedes the `.docx` where they differ; the
  2026-08-10 Mana rewrite exists only in the Markdown.
- `designs/types_archetypes.md` defines card-design identities and balance intent. It is not a rules document.

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
- **Skill:** an ability a Ready Summon can voluntarily activate by paying its cost and becoming Exhausted.
- **Passive Ability:** a continuous effect that normally does not use the Stack.
- **Triggered Ability:** an automatic response to a stated event. It can resolve immediately or create a respondable Stack effect.
- **Priority:** the exclusive right to add one legal Spell to the Stack or pass.
- **Stack:** the last-in, first-out sequence of attacks and respondable effects.
- **Prize Card:** one of two face-down comeback resources recovered after the first two Main losses.
- **Vault:** seven match-play cards outside the 20-card Deck.

## Important relationships

- One player controls one Main Summon and up to three Benched Summons.
- A Summon upgrade chain is one Summon; only the top card defines current characteristics, while Damage remains.
- Attacks target battlefield positions, not a specific Summon that can move away.
- Two consecutive passes start Stack resolution. Ordinary voluntary actions stop until resolution ends or a triggered respondable effect opens a new Priority window.
- Destruction, forced promotion, movement triggers, other automatic triggers, and loss checks occur during resolution.
- Losing is immediate. Unresolved effects stop, and games do not end in a draw.
- The fixed movement-trigger order is Leaving Main, Entering Bench, Leaving Bench, Entering Main.

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
