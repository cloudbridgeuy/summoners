# Summoners context

## Status

The repository is at bootstrap. No game rule is implemented yet. This file is
an index of stable product language from the design inputs, not an API contract.

## Sources

- `designs/core_rules.md` defines the current prototype rules and their open areas.
- `designs/core_rules.docx` is the source document retained with the Markdown rules.
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
- **Mana Pool:** persistent public resources owned by a player. Typed Mana and Generic Mana have different payment rules.
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
