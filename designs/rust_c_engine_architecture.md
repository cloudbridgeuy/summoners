# Summoners Rust Core and C/Raylib Application Architecture

> [!IMPORTANT]
> This document describes architectural ideas and design direction. All code, type names, function signatures, event names, memory layouts, and C interfaces are illustrative examples intended to make the concepts concrete. They are not final decisions about the underlying interfaces or implementation.

## 1. Purpose

Summoners can be built as a C application powered by Raylib, with its authoritative game engine implemented as a Rust library.

The intent is to combine the strengths of both languages:

- Rust expresses the game's rules through enums, structs, traits, exhaustive matching, invariants, and strongly typed state transitions.
- Rust's compiler and unit-testing ecosystem help validate complicated rule interactions.
- C remains in control of the application, Raylib integration, rendering, input, animation, audio, and platform-specific behavior.
- The C layer does not need to recreate the game's rule structures or algorithms.

Rust is compiled into a static or dynamic native library and linked into the C application. It is not translated into C. From the outside, the Rust library exposes a deliberately small C-compatible API.

The result should feel like a game built in C and Raylib that delegates authoritative game decisions to a substantial Rust engine.

## 2. Primary Boundary

A useful rule for deciding where behavior belongs is:

> If changing the behavior could change the legal outcome of a match, it belongs in the Rust engine. If it only changes how players see, hear, or communicate with the game, it belongs in the C/Raylib application.

| Rust engine | C/Raylib application |
| --- | --- |
| Authoritative game state | Window and application loop |
| Rules and action validation | Keyboard, mouse, and controller input |
| Turns, phases, and priority | Translation of UI gestures into player actions |
| Stack and trigger resolution | Rendering cards, boards, and menus |
| Damage, defeat, promotion, and victory | Animation and particle effects |
| Legal choices and targets | Audio and music |
| Card behavior and effect interpretation | Textures, fonts, cameras, and other Raylib resources |
| Deck and setup validation | Navigation and screen management |
| Production of ordered game events | Playback of those events in the UI |
| Save and replay representation | Platform-specific storage mechanisms |
| Validation of external random results | Entropy generation and external shuffling |

This is a division between domain policy and application mechanism, not simply between code that performs I/O and code that does not.

## 3. Suggested High-Level Shape

The overall application can be thought of as three layers:

```text
C/Raylib application
    Input, presentation, audio, animation, platform integration
                         |
                         | C ABI
                         v
Rust FFI facade
    Opaque handles, C-compatible values, ownership rules
                         |
                         v
Rust game engine
    State, actions, rules, stack, triggers, events, validation
```

The Rust implementation might internally contain modules such as:

```text
Rust library
├── domain
│   ├── state
│   ├── actions
│   ├── events
│   ├── cards
│   └── rules
├── engine
│   ├── priority
│   ├── stack
│   ├── triggers
│   └── resolution
├── services
│   ├── serialization
│   ├── card-data loading
│   └── replay encoding
└── ffi
    └── C-compatible facade
```

These boundaries do not require separate crates at the beginning. They can be modules within one Rust crate and separated later if that becomes useful.

## 4. The Authoritative State-Transition Engine

At the conceptual center of the Rust library is a deterministic state transition:

```rust
pub fn apply(
    state: &GameState,
    action: PlayerAction,
) -> Result<ActionOutcome, ActionError>;
```

The important property is not this particular signature. The important property is that the same valid state and the same action produce the same next state and the same ordered game events.

The engine should:

1. Validate whether the action is legal in the current state.
2. Leave the state unchanged if the action is rejected.
3. Apply the accepted action.
4. Resolve every consequence that requires no further decision.
5. Produce an ordered series of game events describing what happened.
6. Stop when it reaches the next meaningful decision boundary or the end of the game.

An implementation may create immutable states directly, use ownership to transform one state into the next, or use controlled internal mutation while constructing an immutable result. That implementation choice does not change the external model.

## 5. Actions, Events, and Continuations

Three concepts should remain distinct.

### 5.1 Player actions

A player action is an attempted decision. It may be accepted or rejected.

Conceptual examples include:

```rust
pub enum PlayerAction {
    DeclareAttack {
        player: PlayerId,
        attacker: SummonId,
        target: SummonId,
    },
    PlayInstant {
        player: PlayerId,
        card: CardId,
        targets: Vec<TargetId>,
    },
    PassPriority {
        player: PlayerId,
    },
    PromoteSummon {
        player: PlayerId,
        summon: SummonId,
    },
    MakeChoice {
        player: PlayerId,
        request_id: RequestId,
        choice: Choice,
    },
}
```

The C application constructs these actions from player input. Rust remains responsible for validating them, even when the UI has already tried to limit the player to legal options.

### 5.2 Game events

A game event is an accepted fact that has already occurred. Events describe the transition for presentation, diagnostics, replays, and testing.

Conceptual examples include:

```rust
pub enum GameEvent {
    AttackDeclared {
        attacker: SummonId,
        target: SummonId,
    },
    PriorityPassed {
        player: PlayerId,
    },
    PriorityGranted {
        player: PlayerId,
    },
    StackItemAdded {
        item: StackItemId,
    },
    StackItemResolved {
        item: StackItemId,
    },
    DamageApplied {
        target: SummonId,
        amount: u32,
        damage_before: u32,
        damage_after: u32,
    },
    SummonDefeated {
        summon: SummonId,
    },
    SummonMoved {
        summon: SummonId,
        from: Zone,
        to: Zone,
    },
    GameEnded {
        winner: PlayerId,
        reason: VictoryReason,
    },
}
```

Event names should normally describe facts in the past tense. The core should emit domain events such as damage being applied, not presentation commands such as playing an explosion animation. C decides how each domain event looks and sounds.

### 5.3 Continuations

A continuation describes why automatic processing stopped and what kind of input may continue the game.

```rust
pub enum Continuation {
    AwaitingPlayer {
        player: PlayerId,
        request: PlayerRequest,
    },
    AwaitingExternalResolution {
        request: ExternalRequest,
    },
    Finished {
        winner: PlayerId,
        reason: VictoryReason,
    },
}
```

A continuation is different from a historical event. Events say what happened; the continuation says what the current state requires.

## 6. Event Batches Rather Than a Long-Lived Stream

The word *stream* captures the idea that the UI receives a sequence of events, but it can imply a long-lived asynchronous connection. A clearer term for the core interface is **event batch** or **transition trace**.

Conceptually, one accepted action returns:

```rust
pub struct ActionOutcome {
    pub next_state: GameState,
    pub events: Vec<GameEvent>,
    pub continuation: Continuation,
}
```

The engine produces a finite, ordered batch and then returns control to C. It does not remain blocked inside a library call while waiting for another player.

The complete rhythm is:

```text
Player action
    → validation
    → state transition
    → automatic rule resolution
    → ordered event batch
    → next decision boundary
```

Across the lifetime of a match, these batches form an event stream or event log. At the individual library-call level, each result is finite.

## 7. Priority and Stack Example

Consider an attack declared by Player 1.

The attack action is first validated. Declaring it does not immediately apply damage because the opponent must receive an opportunity to respond. The engine may produce events conceptually equivalent to:

```text
AttackDeclared
StackItemAdded(Attack)
PriorityGranted(Player 2)
```

The resulting continuation indicates that Player 2 may respond or pass.

If Player 2 passes, the next batch might contain:

```text
PriorityPassed(Player 2)
PriorityGranted(Player 1)
```

If Player 1 then passes, both players have passed consecutively. The engine can resolve the top stack item and continue processing deterministic consequences until it reaches another decision:

```text
PriorityPassed(Player 1)
StackItemResolved(Attack)
DamageApplied(Target, Amount)
TriggerDetected(...)
StackItemAdded(Trigger)
PriorityGranted(...)
```

The precise events and priority rules remain game-design decisions. The architectural point is that every pass is a player action, while stack resolution and other deterministic consequences are performed by the engine without requiring artificial UI actions.

Useful priority invariants include:

- Only the player holding priority may submit a response or pass.
- Playing a response resets the relevant consecutive-pass state.
- Passing transfers priority according to the game's priority rules.
- The required number of consecutive passes resolves the top stack item.
- Passing with an empty stack may close the response window or advance the phase.
- A mandatory pending choice blocks unrelated actions.
- Invalid or stale actions do not partially modify state.

The exact rule for who receives priority after an item resolves should be specified explicitly as part of the game rules.

## 8. Defeat and Mandatory Promotion Example

Suppose an attack resolves, damages the opposing Main Summon, and defeats it.

If the opponent has eligible Benched Summons, the event batch could conceptually contain:

```text
StackItemResolved(Attack)
DamageApplied(Main Summon, 40)
SummonDefeated(Main Summon)
SummonMoved(Main → Discard)
```

The engine then stops at a mandatory decision:

```rust
Continuation::AwaitingPlayer {
    player: defending_player,
    request: PlayerRequest::PromoteSummon {
        candidates: eligible_benched_summons,
    },
}
```

At this point, unrelated actions are illegal. The defending player must submit a promotion action. A valid promotion may produce another batch:

```text
SummonMoved(Bench → Main)
LeavingBenchTriggered(...)
EnteringMainTriggered(...)
Further deterministic consequences...
```

If no eligible Benched Summon exists, or if another immediate defeat condition has been reached, no promotion action is requested. The batch instead concludes with a game-ending event, and the continuation is `Finished`.

The precise timing of defeat triggers, movement, promotion, and enter/leave triggers must be established by the rules. The engine architecture can support the chosen order without assigning it prematurely.

## 9. Automatic Work and Decision Boundaries

Internally, submitting an action can be understood as validating the action and then draining deterministic work:

```rust
pub fn submit(
    state: &GameState,
    action: PlayerAction,
) -> Result<ActionOutcome, ActionError> {
    validate_action(state, &action)?;

    let mut next = state.clone();
    let mut events = Vec::new();

    apply_action(&mut next, action, &mut events);

    let continuation = loop {
        match next_required_step(&next) {
            RequiredStep::Automatic(work) => {
                execute_work(&mut next, work, &mut events);
            }
            RequiredStep::PlayerInput { player, request } => {
                break Continuation::AwaitingPlayer { player, request };
            }
            RequiredStep::ExternalResolution(request) => {
                break Continuation::AwaitingExternalResolution { request };
            }
            RequiredStep::Finished { winner, reason } => {
                break Continuation::Finished { winner, reason };
            }
        }
    };

    Ok(ActionOutcome {
        next_state: next,
        events,
        continuation,
    })
}
```

Again, this is explanatory pseudocode. It does not determine whether the final implementation clones states, consumes them by value, uses a separate reducer, or names these concepts in this way.

The engine may need three distinct collections:

| Structure | Purpose |
| --- | --- |
| Game stack | Rules-visible LIFO attacks, spells, and responses |
| Pending-work or trigger queue | Deterministic internal effects that still require processing |
| Event batch/log | Ordered facts exposed to the application |

These collections should not be conflated merely because they all contain ordered items.

## 10. Suspended Effects Must Be Data

When an effect requires player input or external resolution, the engine must preserve where resolution stopped. That suspended state should be represented as serializable data rather than closures, callbacks, or opaque executable continuations.

For example:

```rust
pub enum PendingEffect {
    ResolveAttack {
        attacker: SummonId,
        target: SummonId,
    },
    ChooseCard {
        request_id: RequestId,
        player: PlayerId,
        candidates: Vec<CardId>,
        continuation: EffectId,
    },
    ResolveDamage {
        source: SourceId,
        target: SummonId,
        amount: u32,
    },
}
```

This makes intermediate states reproducible, testable, serializable, and safe to expose through a C-compatible facade.

## 11. Randomness Is an External Fact

The Rust engine does not need to own a random-number generator. Initial shuffling, coin tosses, and future random effects can be resolved externally.

An initial-game description can provide already resolved facts such as:

- The complete order of both decks.
- Each player's chosen starter.
- Initial hands and prizes.
- Initial discard piles or other zones, if applicable.
- The coin-toss result and starting-player selection.
- Any other setup result that would otherwise require randomness.

Rust should still validate the supplied setup. For example, it can verify deck restrictions, starter legality, card identities, zone consistency, hand sizes, and whether the starting-player choice follows the setup rules.

For randomness occurring during play, Rust should describe the required external result without generating it. One possible conceptual model is:

```rust
pub enum ExternalRequest {
    ChooseUniformly {
        request_id: RequestId,
        candidates: Vec<CardId>,
    },
    Roll {
        request_id: RequestId,
        sides: u32,
        count: u32,
    },
    Shuffle {
        request_id: RequestId,
        cards: Vec<CardId>,
    },
}
```

C, a server, or another host resolves the request and submits the result as an explicit input. Rust verifies that the response matches the outstanding request and is within the legal result space before applying it.

This preserves a valuable distinction:

- The host supplies the random fact.
- Rust owns what may be randomized, which outcomes are valid, and what each outcome means under the rules.

For a local game, the C application may be the source of entropy. In a networked game, an authoritative server could assume that responsibility without requiring a different rules engine.

## 12. Authoritative State and Presentation State

The Rust engine's state and the state currently displayed by the UI do not need to advance at the same speed.

After an action returns, the authoritative Rust state may already be waiting for a promotion while the UI is still animating:

```text
Damage animation
    → defeat animation
    → movement to discard
    → promotion prompt
```

The C application can maintain a presentation model and a presentation-event queue. It consumes the returned events over multiple frames. Once those events have been displayed, the presentation model should agree with the authoritative snapshot and the UI can expose the next continuation.

Events should contain enough information to represent their transition. For example, a damage event may include the amount and the before/after damage values instead of forcing the UI to query an already-advanced state to reconstruct the past.

C will therefore contain meaningful data structures, but they will be presentation structures rather than duplicate game-rule structures. A presented card might contain its texture, screen position, rotation, opacity, and animation state without deciding whether the card is legally playable.

## 13. Legal-Action Discovery

The C UI may need to highlight playable cards, valid targets, available responses, or eligible promotions. Those answers should come from Rust rather than from a duplicate implementation of the rules in C.

Conceptually, the engine may expose queries such as:

```rust
pub fn legal_actions(
    state: &GameState,
    player: PlayerId,
) -> Vec<LegalAction>;

pub fn legal_targets(
    state: &GameState,
    action: &ActionPrototype,
) -> Vec<TargetId>;
```

These signatures are only examples. The interface could instead expose narrower queries, iterators, result handles, or precomputed choices attached to a continuation.

Even when the UI uses legal-action discovery, every submitted action must be validated again. The UI may be stale, contain a bug, or eventually be separated from the authoritative engine by a network.

## 14. C-Compatible Facade

The C boundary should be high-level and narrow. C should not receive pointers into Rust collections or reproduce the full internal object graph.

An illustrative API might use opaque handles:

```c
typedef struct SummonersGame SummonersGame;
typedef struct SummonersOutcome SummonersOutcome;

SummonersGame *summoners_create(
    const SummonersInitialState *initial
);

SummonersStatus summoners_submit(
    SummonersGame *game,
    const SummonersAction *action,
    SummonersOutcome **outcome
);

size_t summoners_outcome_event_count(
    const SummonersOutcome *outcome
);

SummonersStatus summoners_outcome_event(
    const SummonersOutcome *outcome,
    size_t index,
    SummonersEvent *event
);

SummonersContinuation summoners_outcome_continuation(
    const SummonersOutcome *outcome
);

void summoners_outcome_destroy(SummonersOutcome *outcome);
void summoners_destroy(SummonersGame *game);
```

This example illustrates the ownership and interaction model; it does not prescribe the actual API. Other valid approaches include caller-owned buffers, two-call size-and-copy functions, serialized messages, or result cursors.

Useful FFI principles include:

- Keep Rust's authoritative state behind an opaque handle.
- Pass fixed-width primitives, stable identifiers, and explicitly C-compatible structures.
- Do not expose Rust `Vec`, `String`, references, trait objects, or internal enums directly.
- Represent cross-boundary variants through deliberate tags and payloads.
- Keep allocation and deallocation on the same side of the boundary.
- Provide Rust destruction functions for objects allocated by Rust.
- Validate pointers and lengths received from C.
- Never allow a Rust panic to unwind through C.
- Consider an explicit API version as the interface evolves.
- Generate the C header from the Rust-facing declarations when the API becomes large enough to justify it.

Rust may internally use traits and rich enums extensively. The C ABI is only a translation layer; it does not constrain the internal domain model to C's type system.

## 15. C/Raylib Application Loop

At a conceptual level, the C application loop remains in control:

```c
while (!WindowShouldClose()) {
    collect_input();

    if (player_completed_action()) {
        SummonersAction action = build_action_from_ui();
        SummonersOutcome *outcome = NULL;

        if (summoners_submit(game, &action, &outcome) == SUMMONERS_OK) {
            enqueue_outcome_events(outcome);
            remember_continuation(outcome);
        } else {
            present_action_error();
        }
    }

    update_presentations(GetFrameTime());

    BeginDrawing();
    render_game();
    EndDrawing();
}
```

This code is deliberately incomplete. Its purpose is to show that Raylib controls timing and presentation while Rust controls whether an action is legal and what it causes.

Animation completion normally should not be sent back to Rust because it has no effect on game legality. C can temporarily disable input while presenting an event batch, then expose the action described by the continuation.

## 16. I/O, Serialization, and Persistence

The architecture does not prohibit I/O in Rust. The more useful constraint is that the authoritative transition should not contain hidden dependencies that make the same state and action behave differently.

Rust can reasonably own:

- Serialization and deserialization formats.
- Validation of loaded games.
- Parsing of gameplay card definitions.
- Replay encoding and decoding.
- Structured diagnostic information.

For maximum platform flexibility, C can own access to the device while Rust owns the bytes and their meaning. A conceptual interface might serialize a game into a byte buffer, after which C stores it through the platform-appropriate mechanism.

This is especially useful when desktop, mobile, and web builds have different persistence APIs. Optional native filesystem conveniences can still be added without making them fundamental to the engine.

The same separation applies to networking:

- C or another host can own sockets and transport.
- Rust can own message validation and authoritative game transitions.

## 17. Replay and Reproducibility

Because randomness and decisions are explicit inputs, a game can be reproduced from:

```text
Engine and card-set version
    + validated initial state
    + ordered accepted player actions
    + ordered external resolutions
```

The accumulated game events can also form a useful event log. Whether actions, events, periodic state snapshots, or a combination become the persisted replay format is a later decision.

Version information matters because the same historical action could produce different events after rules or card definitions change.

Action envelopes may eventually include a state revision or sequence number so stale actions can be rejected explicitly in networked or asynchronous environments.

## 18. Testing Strategy

The Rust layer should receive most of the rules-focused testing.

Useful test categories include:

- Unit tests for individual rules and card effects.
- Transition tests that assert the next state and exact ordered event batch.
- Invalid-action tests proving that rejected actions leave state unchanged.
- Priority and consecutive-pass tests.
- Trigger-order tests.
- Mandatory-choice tests that reject unrelated actions.
- Setup-validation tests.
- External-resolution tests that reject stale or impossible results.
- Replay tests that reproduce the same final state and events.
- Property tests for invariants such as card-zone uniqueness and valid ownership.

The C/Raylib layer can focus on presentation behavior:

- Correct translation of player input into actions.
- Correct playback order for event batches.
- Synchronization of presentation state with authoritative snapshots.
- Rendering and animation behavior.
- Resource lifetime and platform integration.

Integration tests should exercise the C ABI to catch layout, ownership, and lifecycle problems that Rust-only tests cannot detect.

## 19. Static and Dynamic Linking

Rust can produce either a static C-compatible library or a dynamic C-compatible library.

Conceptually, a Rust package can select a static library with:

```toml
[lib]
crate-type = ["staticlib"]
```

Or a dynamic C-compatible library with:

```toml
[lib]
crate-type = ["cdylib"]
```

These snippets illustrate the relevant Rust output forms; the eventual build configuration may differ.

A static library is the likely default because it simplifies distribution and offers broad deployment compatibility. A dynamic library may be useful for independent engine updates or development-time hot reloading, at the cost of runtime library discovery and version-management concerns.

The Rust library must be built for each target architecture and platform. Raylib's support for a target does not by itself guarantee an equivalent Rust target and linking environment, so web, mobile, and specialized platforms should be tested early. Static linking is generally the most promising common strategy, but final feasibility depends on the target toolchains.

## 20. Design Principles

The architecture can be summarized by the following principles:

1. **C owns the application.** Raylib remains in its native environment and controls the main loop, input, rendering, audio, and platform integration.
2. **Rust owns the game.** Rules, authoritative state, validation, stack resolution, triggers, choices, and outcomes live in Rust.
3. **Actions are attempts.** The engine can reject them without changing state.
4. **Events are facts.** Accepted actions produce ordered batches describing everything that happened.
5. **Continuations are requirements.** They identify the player or external result needed to proceed.
6. **Automatic work stays automatic.** The engine resolves deterministic consequences until it encounters a real decision boundary.
7. **Randomness is explicit.** External systems provide random facts; Rust validates and interprets them.
8. **Presentation is independent.** C animates domain events without becoming authoritative about rules.
9. **The FFI is narrow.** Rich Rust types remain inside Rust and are translated into stable C-facing representations.
10. **Rules are not duplicated.** C can query legal choices, but Rust validates every submitted action.
11. **Intermediate state is data.** Pending effects and choices remain serializable and reproducible.
12. **Examples are not commitments.** Concrete interfaces should be designed only after the domain model and interaction requirements are better understood.

## 21. Questions Left Intentionally Open

This direction does not yet decide:

- The exact Rust crate and module organization.
- Whether the FFI facade owns one mutable handle or accepts and returns serialized immutable states.
- The exact representation of actions, events, continuations, and errors across C.
- Whether event results use copied arrays, opaque result handles, iterators, or serialized buffers.
- The precise priority rule after a stack item resolves.
- The exact timing of defeat, promotion, and enter/leave triggers.
- Whether cards are compiled Rust definitions, external data interpreted by Rust, or a mixture.
- The replay persistence format.
- The snapshot and presentation-synchronization strategy.
- The amount of optional I/O exposed directly by the Rust library.
- The final static and dynamic build targets.

Those decisions can be made incrementally. The important architectural commitment is that the C/Raylib application owns the experience while the Rust library remains the authoritative, strongly typed implementation of the game itself.
