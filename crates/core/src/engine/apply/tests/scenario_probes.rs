//! Integration probes that drive a `Scenario`-built board through the real
//! `apply` entry point, action by action, the way a caller outside this
//! crate would. Unlike `demo` and `demo_triggers` (which mostly start from
//! `base_state()` and hand-build a `GameState` directly), every test here
//! starts from `scenario::from_scenario`, so each one also doubles as a
//! lineage check for the registry chain it uses.
//!
//! Four scenarios were named explicitly for this coverage: rules §42's
//! simultaneous Main destruction, the Warden of Set Paths' Rearrange
//! against the Old Sow of the Barrow's Root and Renew, the Griefsinger's
//! destruction Trigger reopening a Priority window mid-resolution, and an
//! Enchantment's persistence past the turn it resolved on. A fifth test
//! walks every signature chain this registry prints through
//! `from_scenario` together, since nothing before this file exercised all
//! four back to back.

use super::*;
use crate::domain::cards::{EffectLeaf, TriggerEvent};
use crate::scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario};

/// Every signature chain this registry prints, Base through Elite, so
/// `from_scenario`'s lineage rules (rules §20 — a chain must climb Base,
/// Enhanced, Elite in order) get exercised against all four real fixtures
/// together, not only against ad hoc single-card boards.
#[test]
fn every_signature_chains_elite_is_reachable_through_from_scenario() {
    let chains: [[&str; 3]; 4] = [
        ["quarry-whelp", "quarry-brute", "colossus-of-the-quarry"],
        [
            "warden-initiate",
            "warden-pathkeeper",
            "warden-of-set-paths",
        ],
        ["griefsinger-wisp", "griefsinger-mourner", "griefsinger"],
        ["sow-piglet", "sow-matriarch", "old-sow-of-the-barrow"],
    ];

    for [base, enhanced, elite] in chains {
        let scenario = Scenario {
            players: PerPlayer::new(
                ScenarioPlayer {
                    deck: vec![],
                    hand: vec![],
                    prizes: vec![],
                    discard: vec![],
                    mana: ManaBank::default(),
                    main_losses: 0,
                    has_coin: false,
                    main: Some(ScenarioSummon {
                        chain: vec![card_ref(1, base), card_ref(2, enhanced), card_ref(3, elite)],
                        damage: 0,
                        ready: true,
                    }),
                    bench: [None, None, None],
                },
                ScenarioPlayer {
                    deck: vec![],
                    hand: vec![],
                    prizes: vec![],
                    discard: vec![],
                    mana: ManaBank::default(),
                    main_losses: 0,
                    has_coin: false,
                    main: Some(ScenarioSummon {
                        chain: vec![card_ref(101, "quarry-whelp")],
                        damage: 0,
                        ready: true,
                    }),
                    bench: [None, None, None],
                },
            ),
            active_player: PlayerId::One,
        };

        let state = from_scenario(fixtures::card_set(), &scenario).unwrap_or_else(|error| {
            panic!("{base}/{enhanced}/{elite} is a legal Base/Enhanced/Elite lineage: {error:?}")
        });

        assert_eq!(
            state
                .players
                .get(PlayerId::One)
                .main
                .as_ref()
                .map(|summon| summon.chain.top().def),
            Some(fixtures::id(elite)),
            "{base}/{enhanced}/{elite}'s printed Elite must sit on top of the parsed chain"
        );
    }
}

/// Rules §42: a single event can put both players' Mains over their Life
/// at once. This does not build an "explosion" effect (this registry has
/// none) — it pre-loads One's own Main already at lethal Damage, then lets
/// One's own attack finish Two's Main in the same resolution, so the one
/// `WorkItem::DestructionCheck(Position::Main)` that damage enqueues finds
/// both players destroyed at that position together, the same shape the
/// rules example describes. Rules §41 then requires the opponent of the
/// active player (Two, since One is attacking) to resolve their whole
/// destruction chain before One's own begins.
#[test]
fn a_single_destruction_check_can_end_both_players_mains_at_once() {
    let scenario = Scenario {
        players: PerPlayer::new(
            ScenarioPlayer {
                deck: vec![],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                // Already two Main losses; a third one, still queued behind
                // Two's own full destruction chain, must end the game.
                main_losses: 2,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(1, "quarry-whelp")],
                    // Life(40): already at the destruction threshold before
                    // this action runs at all.
                    damage: 40,
                    ready: true,
                }),
                bench: [None, None, None],
            },
            ScenarioPlayer {
                deck: vec![],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(101, "quarry-whelp")],
                    // Life(40): One's free Attack deals 10, finishing Two's
                    // Main in the same resolution that also finds One's own
                    // Main already over its Life.
                    damage: 30,
                    ready: true,
                }),
                bench: [
                    Some(ScenarioSummon {
                        chain: vec![card_ref(102, "quarry-whelp")],
                        damage: 0,
                        ready: true,
                    }),
                    None,
                    None,
                ],
            },
        ),
        active_player: PlayerId::One,
    };
    let state = from_scenario(fixtures::card_set(), &scenario).expect("both boards are legal");

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("One's Main is Ready and its Attack is free");
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority first");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the second consecutive pass resolves the Attack and drains its consequences");

    // Rules §41: Two (the opponent of the active player, One) resolves
    // their whole destruction chain first. Two's Main was destroyed and
    // survived by promoting; One's Main was destroyed and, on its third
    // loss, ended the game — both traceable in the same ordered batch.
    let two_destroyed = resolved
        .events
        .iter()
        .position(|event| {
            matches!(
                event,
                GameEvent::SummonDestroyed {
                    position: Position::Main,
                    owner: PlayerId::Two,
                }
            )
        })
        .expect("Two's Main destruction is recorded");
    let two_promoted = resolved
        .events
        .iter()
        .position(|event| {
            matches!(
                event,
                GameEvent::SummonPromoted {
                    player: PlayerId::Two,
                    ..
                }
            )
        })
        .expect("Two's Bench Summon is promoted into the vacated Main");
    let one_destroyed = resolved
        .events
        .iter()
        .position(|event| {
            matches!(
                event,
                GameEvent::SummonDestroyed {
                    position: Position::Main,
                    owner: PlayerId::One,
                }
            )
        })
        .expect("One's Main destruction is recorded");
    assert!(
        two_destroyed < two_promoted && two_promoted < one_destroyed,
        "Two's whole destruction chain must finish before One's own begins"
    );

    assert_eq!(
        resolved.events.last(),
        Some(&GameEvent::GameEnded {
            winner: PlayerId::Two,
            reason: LossReason::ThirdMainLoss,
        }),
        "One's third Main loss ends the game outright"
    );
    assert_eq!(
        resolved.state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::Two,
            reason: LossReason::ThirdMainLoss,
        })
    );

    let two = resolved.state.players.get(PlayerId::Two);
    assert_eq!(two.main_losses, 1);
    assert_eq!(
        two.main.as_ref().map(|summon| summon.chain.top().def),
        Some(fixtures::id("quarry-whelp")),
        "Two's promoted Bench Summon now occupies Main"
    );
    assert_eq!(two.bench, [None, None, None]);

    let one = resolved.state.players.get(PlayerId::One);
    assert_eq!(one.main_losses, 3);
}

/// Rules §44: the Old Sow of the Barrow's Root and Renew keeps its own
/// Main from being moved by the opponent "until the controller's next
/// turn" — the whole of the opponent's next turn, not just the moment it
/// is cast. The Warden of Set Paths' Rearrange is the only Skill in this
/// registry that can move an opposing Main at all, so this scenario uses
/// it to observe both halves: rejected outright while the marker holds,
/// legal again once it has expired.
#[test]
fn a_rooted_main_blocks_rearrange_for_the_opponents_whole_turn_then_allows_it() {
    let scenario = Scenario {
        players: PerPlayer::new(
            ScenarioPlayer {
                deck: vec![card_ref(50, "quarry-whelp"), card_ref(51, "quarry-whelp")],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank {
                    matter: 2,
                    mind: 2,
                    spirit: 0,
                },
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![
                        card_ref(1, "warden-initiate"),
                        card_ref(2, "warden-pathkeeper"),
                        card_ref(3, "warden-of-set-paths"),
                    ],
                    damage: 0,
                    ready: true,
                }),
                bench: [None, None, None],
            },
            ScenarioPlayer {
                deck: vec![card_ref(150, "quarry-whelp"), card_ref(151, "quarry-whelp")],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank {
                    matter: 1,
                    mind: 0,
                    spirit: 1,
                },
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![
                        card_ref(101, "sow-piglet"),
                        card_ref(102, "sow-matriarch"),
                        card_ref(103, "old-sow-of-the-barrow"),
                    ],
                    damage: 0,
                    ready: true,
                }),
                bench: [
                    Some(ScenarioSummon {
                        chain: vec![card_ref(201, "quarry-whelp")],
                        damage: 0,
                        ready: true,
                    }),
                    None,
                    None,
                ],
            },
        ),
        active_player: PlayerId::Two,
    };
    let state = from_scenario(fixtures::card_set(), &scenario).expect("both boards are legal");

    // Two roots their own Main: Root and Renew, cost Matter + Spirit.
    let rooted = apply(
        &state,
        &GameAction::ActivateSkill {
            player: PlayerId::Two,
            position: Position::Main,
            ability: fixtures::skill_id("old-sow-of-the-barrow"),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("Two's Main is Ready and Root and Renew's cost is covered");
    assert!(
        !rooted
            .state
            .players
            .get(PlayerId::Two)
            .main
            .as_ref()
            .expect("Two still has a Main")
            .ready,
        "activating a Skill exhausts the Summon (rules §15)"
    );

    // Two's turn ends; One becomes active. The marker only ever clears on
    // its own holder's own next turn, so it must still hold here.
    let one_active = advance_turn(&rooted.state, PlayerId::Two);
    assert_eq!(one_active.state.turn.active_player, PlayerId::One);

    let first_rearrange = apply(
        &one_active.state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("warden-of-set-paths"),
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    );
    assert_eq!(
        first_rearrange,
        Err(ActionError::InvalidTarget),
        "the marker still holds through the whole of One's turn"
    );

    // One's turn ends; Two becomes active a second time — the handover
    // where the marker finally clears from Two's own board.
    let two_active_again = advance_turn(&one_active.state, PlayerId::One);
    assert_eq!(two_active_again.state.turn.active_player, PlayerId::Two);

    // Two's turn ends; One becomes active a second time.
    let one_active_again = advance_turn(&two_active_again.state, PlayerId::Two);
    assert_eq!(one_active_again.state.turn.active_player, PlayerId::One);

    let second_rearrange = apply(
        &one_active_again.state,
        &GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: fixtures::skill_id("warden-of-set-paths"),
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    )
    .expect("the marker has expired, so Rearrange is legal again");

    let two = second_rearrange.state.players.get(PlayerId::Two);
    assert_eq!(
        two.main.as_ref().map(|summon| summon.chain.top().def),
        Some(fixtures::id("quarry-whelp")),
        "Two's former Bench Summon now occupies Main"
    );
    assert_eq!(
        two.bench[0].as_ref().map(|summon| summon.chain.top().def),
        Some(fixtures::id("old-sow-of-the-barrow")),
        "the Old Sow itself was swapped onto the vacated Bench slot"
    );
}

/// Run a full `EndTurn` handoff the way `full_end_turn` already does, then
/// answer a `ManaProduction` pause automatically with Matter — every
/// dual-type fixture this file hands off with (the Warden of Set Paths,
/// the Old Sow of the Barrow) anchors Matter alongside its second Type, so
/// this is always a legal answer here. `full_end_turn` and `apply` both
/// drain the resolution loop after every step, so the returned state has
/// already reached the new turn's own Main Phase on its own (rules §9) by
/// the time this returns — nothing here needs to touch `state.turn.phase`.
fn advance_turn(state: &GameState, player: PlayerId) -> ActionOutcome {
    let mut outcome = full_end_turn(state, player).expect("legal from a resting Main Phase");
    if let Some(PendingInput::ManaProduction {
        player: waiting, ..
    }) = outcome.state.pending
    {
        let answered = apply(
            &outcome.state,
            &GameAction::ChooseManaType {
                player: waiting,
                mana_type: ManaType::Matter,
            },
        )
        .expect("Matter is anchored on every dual-type fixture this file hands off with");
        outcome.events.extend(answered.events);
        outcome.state = answered.state;
    }
    outcome
}

/// Rules §36-41: a respondable Trigger opens a brand-new Priority window
/// and a new Stack segment even when it fires in the middle of an
/// already-in-progress destruction chain — the remaining steps of that
/// chain (recording the loss, recovering a Prize, promoting) stay queued
/// behind it until the nested exchange settles, the same way any other
/// respondable trigger already pauses `engine::resolution::drain` mid-work.
/// The Griefsinger's own destruction Trigger, still on the Bench when its
/// controller's Main is destroyed, demonstrates it.
#[test]
fn a_destruction_trigger_opens_a_nested_window_before_its_own_chain_finishes() {
    let scenario = Scenario {
        players: PerPlayer::new(
            ScenarioPlayer {
                deck: vec![],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(1, "quarry-whelp")],
                    damage: 0,
                    ready: true,
                }),
                bench: [None, None, None],
            },
            ScenarioPlayer {
                deck: vec![],
                hand: vec![],
                prizes: vec![],
                discard: vec![card_ref(210, "ember-lance")],
                mana: ManaBank::default(),
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(101, "quarry-whelp")],
                    // Life(40): One's free Attack (10 Damage) finishes it.
                    damage: 30,
                    ready: true,
                }),
                bench: [
                    Some(ScenarioSummon {
                        chain: vec![
                            card_ref(201, "griefsinger-wisp"),
                            card_ref(202, "griefsinger-mourner"),
                            card_ref(203, "griefsinger"),
                        ],
                        damage: 0,
                        ready: true,
                    }),
                    None,
                    None,
                ],
            },
        ),
        active_player: PlayerId::One,
    };
    let state = from_scenario(fixtures::card_set(), &scenario).expect("both boards are legal");

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("One's Main is Ready and its Attack is free");
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority first");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the Attack resolves, Two's Main is destroyed, and the Griefsinger's Trigger fires");

    assert_eq!(resolved.state.pending, None);
    assert_eq!(
        resolved.state.stack,
        vec![StackItem::Trigger {
            controller: PlayerId::Two,
            source: Position::Bench(BenchSlot::First),
            ability: fixtures::trigger_id("griefsinger"),
            event: TriggerEvent::AnySummonDestroyed,
            targets: vec![Position::Bench(BenchSlot::First)],
            effects: vec![EffectLeaf::ReturnSpellFromDiscard],
        }],
        "the destruction Trigger opened its own Stack segment"
    );
    assert_eq!(resolved.state.stack_segment_bases, vec![0]);
    assert!(
        resolved.state.turn.window.is_some(),
        "a respondable Trigger opens a new Priority window (rules §38)"
    );
    assert!(
        !resolved.state.work.is_empty(),
        "the rest of Two's destruction chain is still queued behind the nested window"
    );
    assert_eq!(
        resolved.events.last(),
        Some(&GameEvent::TriggerFired {
            controller: PlayerId::Two,
            position: Position::Bench(BenchSlot::First),
            event: TriggerEvent::AnySummonDestroyed,
            ability: fixtures::trigger_id("griefsinger"),
        }),
        "the nested window opening is the last thing this action produced"
    );

    // The nested window: One (the opponent of the Trigger's controller,
    // Two) holds Priority first.
    let one_passed = apply(
        &resolved.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("One holds the nested window first");
    assert!(
        one_passed.state.turn.window.is_some(),
        "a single pass never closes a window by itself"
    );

    let two_passed = apply(
        &one_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Two holds the nested window second, closing it and resuming the interrupted chain");

    assert_eq!(two_passed.state.turn.window, None);
    assert!(two_passed.state.stack.is_empty());
    assert!(two_passed.state.stack_segment_bases.is_empty());

    let two = two_passed.state.players.get(PlayerId::Two);
    assert_eq!(
        two.discard,
        vec![card_ref(101, "quarry-whelp")],
        "the destroyed chain lands in discard; the Trigger already returned the Spell out of it"
    );
    assert_eq!(two.hand, vec![card_ref(210, "ember-lance")]);
    assert_eq!(
        two.main.as_ref().map(|summon| summon.chain.top().def),
        Some(fixtures::id("griefsinger")),
        "the interrupted destruction chain resumed and completed once the nested window closed"
    );
    assert_eq!(two.bench, [None, None, None]);
    assert_eq!(two.main_losses, 1);
    assert_eq!(two_passed.state.status, GameStatus::Playing);
}

/// Rules §44: "Enchantments ... remain in play after resolving ... until
/// an effect removes it" — unlike a Spell, which discards the instant it
/// resolves. This casts Standing Ward (the vanilla Enchantment fixture),
/// resolves it, and confirms it neither discards nor disappears across a
/// full turn handover to the opponent.
#[test]
fn a_resolved_enchantment_stays_in_play_through_a_full_turn_handover() {
    let scenario = Scenario {
        players: PerPlayer::new(
            ScenarioPlayer {
                deck: vec![],
                hand: vec![card_ref(5, "standing-ward")],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank {
                    matter: 1,
                    mind: 0,
                    spirit: 0,
                },
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(1, "quarry-whelp")],
                    damage: 0,
                    ready: true,
                }),
                bench: [None, None, None],
            },
            ScenarioPlayer {
                deck: vec![card_ref(150, "quarry-whelp")],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(101, "quarry-whelp")],
                    damage: 0,
                    ready: true,
                }),
                bench: [None, None, None],
            },
        ),
        active_player: PlayerId::One,
    };
    let state = from_scenario(fixtures::card_set(), &scenario).expect("both boards are legal");

    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(5),
            targets: vec![],
            mana_hint: None,
        },
    )
    .expect("Standing Ward is affordable and legal in One's own resting Main");
    let defender_passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority first");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster holds Priority second, resolving the cast Enchantment");

    let one = resolved.state.players.get(PlayerId::One);
    assert_eq!(
        one.enchantments,
        vec![card_ref(5, "standing-ward")],
        "the Enchantment stays in play rather than discarding (rules §44)"
    );
    assert!(one.discard.is_empty());

    let handed_over =
        full_end_turn(&resolved.state, PlayerId::One).expect("legal from a resting Main Phase");
    assert_eq!(handed_over.state.turn.active_player, PlayerId::Two);
    assert_eq!(
        handed_over.state.players.get(PlayerId::One).enchantments,
        vec![card_ref(5, "standing-ward")],
        "the Enchantment survives a full turn handover unchanged"
    );
}

/// Rules §9: "Phases only move forward" — once a turn hands off, the new
/// active player's Upkeep must itself progress into a Main Phase they can
/// actually act in, not rest in Upkeep forever. This drives the whole
/// exchange through `apply` and `scenario::from_scenario` only, with no
/// hand-editing of `state.turn.phase` anywhere: `full_end_turn` submits
/// `EndTurn` and both `PassPriority` answers as three real actions, each
/// one draining the resolution loop the way any outside caller's action
/// would, so by the time `advance_turn` returns, Two's own Upkeep — Ready,
/// draw, and natural production — has already run to completion and left
/// `state.turn.phase` at `Phase::Main` on its own. Two then plays a Base
/// Summon from hand, legal only because that Main Phase is real: before
/// this behavior existed, `PlaySummon` rejected every Main Phase action for
/// the rest of the game with `WrongPhase`, since nothing ever advanced past
/// the first Upkeep.
#[test]
fn a_second_turn_reaches_its_own_main_phase_and_can_act_in_it() {
    let scenario = Scenario {
        players: PerPlayer::new(
            ScenarioPlayer {
                deck: vec![card_ref(50, "quarry-whelp")],
                hand: vec![],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(1, "quarry-whelp")],
                    damage: 0,
                    ready: true,
                }),
                bench: [None, None, None],
            },
            ScenarioPlayer {
                deck: vec![card_ref(150, "quarry-whelp")],
                hand: vec![card_ref(151, "quarry-whelp")],
                prizes: vec![],
                discard: vec![],
                mana: ManaBank::default(),
                main_losses: 0,
                has_coin: false,
                main: Some(ScenarioSummon {
                    chain: vec![card_ref(101, "quarry-whelp")],
                    damage: 0,
                    ready: true,
                }),
                bench: [None, None, None],
            },
        ),
        active_player: PlayerId::One,
    };
    let state = from_scenario(fixtures::card_set(), &scenario).expect("both boards are legal");

    let handed_over = advance_turn(&state, PlayerId::One);
    assert_eq!(handed_over.state.turn.active_player, PlayerId::Two);
    assert_eq!(
        handed_over.state.turn.phase,
        Phase::Main,
        "Two's own Upkeep — Ready, draw, and production — drained all the \
         way through to the Main Phase advance on its own (rules §9)"
    );
    assert!(handed_over.state.work.is_empty());

    let played = apply(
        &handed_over.state,
        &GameAction::PlaySummon {
            player: PlayerId::Two,
            card: CardInstanceId(151),
            slot: BenchSlot::First,
        },
    )
    .expect("Two's Main Phase has genuinely begun, so playing a Base Summon is legal");

    assert_eq!(
        played.events,
        vec![GameEvent::SummonPlayed {
            player: PlayerId::Two,
            card: CardInstanceId(151),
            slot: BenchSlot::First,
        }]
    );
    assert_eq!(
        played.state.players.get(PlayerId::Two).bench[0]
            .as_ref()
            .map(|summon| summon.chain.top().def),
        Some(fixtures::id("quarry-whelp")),
        "the freshly played Base Summon now occupies Two's Bench"
    );
}
