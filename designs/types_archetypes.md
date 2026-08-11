# Summoners — Mana Types & Archetypes
**Draft v1**

This document defines the three Mana Types of Summoners, their mechanical and thematic identities, the design philosophy governing mono-type and dual-type cards, and the six resulting deck archetypes. It is a design document, not a rules document: the rules engine defines what is possible; this document defines the identities and balance intentions expressed through the card file.

---

## 1. The Three Realms

The world of Summoners is layered across three realms of being. Each realm is a Mana Type.

| Type | Concept | Domain |
|---|---|---|
| **Matter** | The realm of bodies and force | Everything physical: stone, flesh, metal, weight, permanence |
| **Mind** | The realm born from thought | Ideas, stories, memory, information — reality produced by minds |
| **Spirit** | The realm of essence | Life-force, emotion, birth and death, love and hate |

The names are deliberately plain. They read as fundamental forces rather than factions, and each name immediately telegraphs its mechanical identity. Iconography and color carry the flavor:

| Type | Icon direction | Palette |
|---|---|---|
| Matter | Anvil / hexagon | Bronze, stone grey, ochre |
| Mind | Eye / spiral | Silver, violet, pale blue |
| Spirit | Flame / twin crescent | Gold, crimson, ember green |

---

## 2. Mana Rule Reference

Defined in the core rules (see rules §11–12); restated here because the type system depends on it:

- **All Mana is typed.** There is no Generic Mana resource.
- During Upkeep, the player naturally generates **one Mana of a Type they choose from the Mana Types of the Summons they control** in Main or on the Bench.
- Summons generate **typed Mana** matching their Mana Type(s); a multi-type Summon's controller chooses which one type it produces.
- Costs may include **typed components** and **Generic components**. Typed components require the named Type; Generic components accept Mana of any Type.

Consequences:

- Natural production is anchored to the board: the player chooses among the Mana Types of the Summons they control, so a player cannot fake a realm identity they did not bring into play. The floor resource remains, because Main is never empty and at least one Type is therefore always available.
- The Bench is the mana base twice over: Summons produce typed Mana, and their Types define what the player's natural Mana may become. Bench composition is a strategic resource decision.
- The second player's Coin converts into one Mana whose Type must come from their in-play Summons' Mana Types: a tempo tool, not a color fixer.

---

## 3. Mono-Type Identities

### Matter — *the realm of force*

**Mechanical home:** High Life values, raw attack damage, Retreat-cost manipulation, passives that scale with board presence. Matter owns the biggest bodies and the hardest hits in the file.

**Plays like:** Aggressive, board-centric, honest. Stats matter.

**Benchmark card — Colossus of the Quarry (Elite):**
Life 180. Attack (Matter + Matter + 1): 90 Damage. No text.
The Colossus defines the rate ceiling: it out-hits every dual card in the file, by design.

### Mind — *the realm of thought*

**Mechanical home:** Attack Spells, responses, Priority tricks, card draw, information (looking at Prize Cards, hands, hidden zones), retargeting and redirecting attacks between positions. An attack is a kind of story, and Mind rewrites stories.

**Plays like:** Reactive, tricky, Stack-native. The Priority system is Mind's home terrain.

### Spirit — *the realm of essence*

**Mechanical home:** Healing, destruction triggers, discard-pile recursion, Mana bursts when Summons fall or are promoted. Spirit cares about the passage between life and death — which the Main-destruction and Prize mechanics make central to the game.

**Plays like:** Resilient, cyclical, attrition-oriented. Spirit turns losses into fuel.

---

## 4. Mono vs. Dual — Design Philosophy

**Mono-type cards buy rate. Dual-type cards buy reach.**

These are card-design and balance guidelines, not rules:

1. A dual-type Summon should cost more, or carry slightly lower Life/damage, than a mono-type equivalent at the same level.
2. A dual-type Summon's **strongest Skill should require both of its Mana Types** ("dual-gated"), so a deck merely splashing one realm cannot access it.
3. Passives and baseline attacks may remain ungated — they are what the card does for showing up.
4. Mono lines are the consistency option: under the upgrade-typing rule, a mono Base keeps every upgrade path open, while a dual Base locks its line in. Deckbuilders trade flexibility for rate — or rate for reach — at the moment they pick their Bases.

---

## 5. Dual-Type Archetypes

Each pair's identity comes from what its two realms *share*.

### Matter / Mind — Control through the board

**Shared ground:** Structure — form, pattern, engineering. Thought imposed on matter.

**Mechanical home:** Attacks that target Bench positions, forced movement of opposing Summons (exploiting the rule that attacks target positions, not creatures), Ready/Exhaust manipulation, Skills that scale with board state.

**Plays like:** A chess player. Slower, wins by dictating which Summon is forced to fight.

**Flavor space:** Constructs, golem-scribes, living architecture, automata.

**Signature Elite — The Warden of Set Paths:**
Life 150. Produces Matter or Mind. Retreat 3.
- Skill (Matter + Mind): *Rearrange* — Exchange the opposing Main with a Benched Summon of your choice, or move one opposing Benched Summon to another Bench position.
- Passive: Opposing Retreats cost 1 more.
- Attack (Matter + 2 Generic): 50 Damage. +30 if the defending Summon entered Main this turn.

### Mind / Spirit — The immaterial pair

**Shared ground:** Interiority — everything that exists without a body.

**Mechanical home:** The Stack. Attack Spells and responses-to-responses, destruction triggers that draw or recur, Spell recursion from the discard, Prize Card information and manipulation.

**Plays like:** A duelist of reactions. Fragile Summons; every attack against them costs the attacker something.

**Flavor space:** Ghosts, oracles, memories given will — things once alive or once thought.

**Signature Elite — The Griefsinger:**
Life 110. Produces Mind or Spirit. Retreat 1.
- Trigger (Stack): When any Summon is destroyed, you may return one Spell from your discard pile to your hand.
- Skill (Mind + Spirit): *Foresee* — Look at either player's Prize Cards, then draw a card, then you may return one Spell from your hand to the top of your Deck.
- Attack (Mind + Spirit + 1): 40 Damage. If you played a Spell this turn, +40 and this attack cannot be responded to by Attack Spells.

### Matter / Spirit — Life itself

**Shared ground:** Vitality — bodies animated by essence; the pair that bypasses Mind entirely.

**Mechanical home:** Big Life plus healing and death payoffs. Summons that heal when promoted, generate Mana when destroyed, or return from the discard as Base forms. Compounds with the persistent-damage rule in long games.

**Plays like:** The unkillable herd. It doesn't outplay the opponent on the Stack; it simply doesn't die on schedule.

**Flavor space:** Beasts, primal spirits, blood-and-bone shamanism — life without cleverness.

**Signature Elite — The Old Sow of the Barrow:**
Life 170. Produces Matter or Spirit. Retreat 4.
- Passive: During your Upkeep, heal 10 Damage from this Summon.
- Skill (Matter + Spirit): *Root and Renew* — Heal 30 Damage from this Summon; it cannot be moved out of Main by opposing effects until your next turn.
- Attack (Matter + Spirit + 2): 70 Damage. This damage cannot be increased and cannot be prevented.

---

## 6. The Archetype Triangle

- **Matter/Mind** controls the **board**.
- **Mind/Spirit** controls the **Stack**.
- **Matter/Spirit** ignores both and controls **time**.

Each pair is strong in the arena one of the others neglects — a soft rock-paper-scissors without hard counters. Cross-answers are deliberate and should be dual-gated (e.g., the Sow's *Root and Renew* answering the Warden's *Rearrange*).

Mono decks sit around the triangle as the purest version of one axis: mono-Matter hits harder than Matter/Spirit, mono-Mind is more reactive than Mind/Spirit, mono-Spirit out-heals both of its pairs — trading versatility for consistency.

---

## 7. Open Questions (v1)

- Exact rate gap between mono and dual cards at each level (to be set by simulation and playtest, not by rule).
- Whether Base-level dual Summons should gate Skills on a single type, with dual gates arriving at Enhanced/Elite — a pacing question for turns 1–3.
- Adjective conventions in card text: noun-everywhere ("Matter Mana", "Mind Skill") is the current working standard.
- Whether typed-Mana stranding (unspendable typed Mana at game end) differs meaningfully by realm — a simulation metric.

---

*Draft v1 — types and archetypes only. Card file, test plan, and art manifest are maintained separately.*
