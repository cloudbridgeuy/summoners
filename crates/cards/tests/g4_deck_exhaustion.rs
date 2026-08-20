#![allow(clippy::expect_used)]

mod support;

use std::collections::VecDeque;

use summoners_cards::{CardLibrary, built_in_catalog};
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::{EntityId, Skill},
        errors::ActionError,
        events::GameEvent,
        ids::{ManaType, PlayerId, Position},
        state::{
            CardRef, GameOutcome, GameState, GameStatus, LossReason, ManaBank, ManaSource, Phase,
            StackItem, StackWindow, WorkItem,
        },
    },
    engine::apply::apply,
};

use support::{PhysicalCards, player, state};

struct G4 {
    state: GameState,
    seer_skill: EntityId,
    scrying_glass: CardRef,
    one_upkeep_draw: CardRef,
    one_effect_draw: CardRef,
    two_skill_draw: CardRef,
    returned_spell: CardRef,
    retained_spell: CardRef,
    prizes: [CardRef; 2],
}

fn take(cards: &mut Vec<CardRef>, library: &CardLibrary, qualified_key: &str) -> CardRef {
    let definition = library
        .card_id(qualified_key)
        .unwrap_or_else(|| panic!("real definition must exist: {qualified_key}"));
    let index = cards
        .iter()
        .position(|card| card.def == definition)
        .unwrap_or_else(|| panic!("resolved Deck must contain: {qualified_key}"));
    cards.remove(index)
}

fn only_skill(state: &GameState, definition: EntityId) -> EntityId {
    let entity = state.cards.get(definition).expect("card must exist");
    let skills = entity.all::<Skill>();
    assert_eq!(skills.len(), 1, "Barrow Seer prints one Skill");
    skills[0].id
}

fn seeded_g4() -> G4 {
    let catalog = built_in_catalog().expect("built-in catalog is valid");
    let library = catalog.library();
    let mut physical = PhysicalCards::new(library, 4_000);
    let mut one_recipe = physical.deck(catalog.set_paths());
    let mut two_recipe = physical.deck(catalog.barrow_herd());

    let scrying_glass = take(&mut one_recipe, library, "foundations/scrying-glass");
    let one_upkeep_draw = one_recipe.remove(0);
    let one_effect_draw = one_recipe.remove(0);
    let one_starter = physical.one("foundations/warden-initiate");
    assert_eq!(one_starter.def, catalog.set_paths().starter());

    let seer = take(&mut two_recipe, library, "foundations/barrow-seer");
    let returned_spell = take(&mut two_recipe, library, "foundations/renewing-balm");
    let retained_spell = take(&mut two_recipe, library, "foundations/ember-lance");
    let two_skill_draw = two_recipe.remove(0);
    let prizes = [two_recipe.remove(0), two_recipe.remove(0)];

    let mut one = player(summoners_core::scenario::ScenarioSummon {
        chain: vec![one_starter],
        damage: 0,
        ready: true,
    });
    one.deck = vec![one_upkeep_draw, one_effect_draw];
    one.hand = vec![scrying_glass];
    one.mana.matter = 1;

    let mut two = player(summoners_core::scenario::ScenarioSummon {
        chain: vec![seer],
        damage: 0,
        ready: true,
    });
    two.deck = vec![two_skill_draw];
    two.hand = vec![returned_spell, retained_spell];
    two.prizes = prizes.to_vec();
    two.mana = ManaBank {
        matter: 1,
        mind: 0,
        spirit: 1,
    };

    let state = state(&catalog, one, two, PlayerId::Two).expect("G4 seed is valid");
    G4 {
        seer_skill: only_skill(&state, seer.def),
        state,
        scrying_glass,
        one_upkeep_draw,
        one_effect_draw,
        two_skill_draw,
        returned_spell,
        retained_spell,
        prizes,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn step(state: &mut GameState, action: GameAction) -> Vec<GameEvent> {
    let outcome = apply(state, &action).expect("G4 action must be legal");
    *state = outcome.state;
    outcome.events
}

fn pass(state: &mut GameState, player: PlayerId) -> Vec<GameEvent> {
    step(state, GameAction::PassPriority { player })
}

#[test]
fn g4_effect_draws_empty_the_deck_before_the_next_upkeep_draw_ends_the_game() {
    let mut g4 = seeded_g4();
    assert_eq!(
        g4.state.players.two.hand,
        vec![g4.returned_spell, g4.retained_spell],
        "the two physical Spells start in the order selected from the Deck recipe"
    );

    let seer = step(
        &mut g4.state,
        GameAction::ActivateSkill {
            player: PlayerId::Two,
            position: Position::Main,
            ability: g4.seer_skill,
            targets: vec![],
            mana_hint: Some(ManaType::Matter),
        },
    );
    assert_eq!(
        seer,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Spirit,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::Two,
                position: Position::Main,
                ability: g4.seer_skill,
            },
            GameEvent::PrizesViewed {
                player: PlayerId::Two,
                prizes: vec![g4.prizes[0].instance, g4.prizes[1].instance],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: g4.two_skill_draw.instance,
            },
        ],
        "Barrow Seer resolves its printed effects in order"
    );
    assert_eq!(g4.state.players.two.prizes, g4.prizes.to_vec());
    assert_eq!(
        g4.state.players.two.hand,
        vec![g4.retained_spell, g4.two_skill_draw],
        "only the first Spell leaves the ordered hand"
    );
    assert_eq!(g4.state.players.two.deck, vec![g4.returned_spell]);
    assert_eq!(
        g4.state.players.two.deck.last(),
        Some(&g4.returned_spell),
        "the first Spell becomes the Deck top"
    );
    assert_eq!(g4.state.status, GameStatus::Playing);
    assert!(g4.state.pending.is_none());
    assert!(!g4.state.players.two.main.as_ref().expect("Seer").ready);

    let stable = g4.state.clone();
    assert_eq!(
        apply(
            &g4.state,
            &GameAction::EndTurn {
                player: PlayerId::One,
            },
        ),
        Err(ActionError::NotYourDecision)
    );
    assert_eq!(g4.state, stable, "the actor gate changes no state");

    assert!(
        step(
            &mut g4.state,
            GameAction::EndTurn {
                player: PlayerId::Two,
            },
        )
        .is_empty()
    );
    assert_eq!(
        g4.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        })
    );
    let stable = g4.state.clone();
    assert_eq!(
        apply(
            &g4.state,
            &GameAction::PassPriority {
                player: PlayerId::Two,
            },
        ),
        Err(ActionError::NotYourDecision)
    );
    assert_eq!(g4.state, stable);
    assert_eq!(
        pass(&mut g4.state, PlayerId::One),
        vec![GameEvent::PriorityPassed {
            player: PlayerId::One,
        }]
    );
    let one_began = pass(&mut g4.state, PlayerId::Two);
    assert_eq!(
        one_began,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::TurnBegan {
                player: PlayerId::One,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: g4.one_upkeep_draw.instance,
            },
            GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Player,
                mana_type: ManaType::Matter,
            },
        ]
    );
    assert_eq!(g4.state.turn.active_player, PlayerId::One);
    assert_eq!(g4.state.turn.phase, Phase::Main);
    assert!(g4.state.pending.is_none());
    assert_eq!(g4.state.players.one.deck, vec![g4.one_effect_draw]);
    assert_eq!(
        g4.state.players.one.hand,
        vec![g4.scrying_glass, g4.one_upkeep_draw]
    );

    let cast = step(
        &mut g4.state,
        GameAction::CastSpell {
            player: PlayerId::One,
            card: g4.scrying_glass.instance,
            targets: vec![],
            mana_hint: Some(ManaType::Matter),
        },
    );
    assert_eq!(
        cast,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SpellCast {
                player: PlayerId::One,
                card: g4.scrying_glass.instance,
                targets: vec![],
            },
        ]
    );
    assert_eq!(
        g4.state.stack,
        vec![StackItem::Spell {
            caster: PlayerId::One,
            card: g4.scrying_glass,
            targets: vec![],
        }]
    );
    assert_eq!(
        g4.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
    assert_eq!(
        pass(&mut g4.state, PlayerId::Two),
        vec![GameEvent::PriorityPassed {
            player: PlayerId::Two,
        }]
    );
    let last_draw = pass(&mut g4.state, PlayerId::One);
    assert_eq!(
        last_draw,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Spell {
                    caster: PlayerId::One,
                    card: g4.scrying_glass,
                    targets: vec![],
                },
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: g4.one_effect_draw.instance,
            },
        ],
        "drawing the final available card succeeds"
    );
    assert!(g4.state.players.one.deck.is_empty());
    assert_eq!(
        g4.state.players.one.hand,
        vec![g4.one_upkeep_draw, g4.one_effect_draw]
    );
    assert_eq!(g4.state.players.one.discard, vec![g4.scrying_glass]);
    assert_eq!(g4.state.status, GameStatus::Playing);
    assert_eq!(g4.state.turn.phase, Phase::Main);
    assert!(g4.state.stack.is_empty());
    assert!(g4.state.work.is_empty());
    assert!(g4.state.pending.is_none());

    assert!(
        step(
            &mut g4.state,
            GameAction::EndTurn {
                player: PlayerId::One,
            },
        )
        .is_empty()
    );
    assert_eq!(
        pass(&mut g4.state, PlayerId::Two),
        vec![GameEvent::PriorityPassed {
            player: PlayerId::Two,
        }]
    );
    let two_began = pass(&mut g4.state, PlayerId::One);
    assert_eq!(
        two_began,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: g4.returned_spell.instance,
            },
            GameEvent::ManaProduced {
                player: PlayerId::Two,
                source: ManaSource::Player,
                mana_type: ManaType::Spirit,
            },
        ]
    );
    assert_eq!(g4.state.players.two.deck, vec![]);
    assert_eq!(
        g4.state.players.two.hand,
        vec![g4.retained_spell, g4.two_skill_draw, g4.returned_spell,],
        "the returned first Spell is the next Upkeep draw"
    );
    assert_eq!(g4.state.status, GameStatus::Playing);
    assert_eq!(g4.state.turn.phase, Phase::Main);

    assert!(
        step(
            &mut g4.state,
            GameAction::EndTurn {
                player: PlayerId::Two,
            },
        )
        .is_empty()
    );
    assert_eq!(
        pass(&mut g4.state, PlayerId::One),
        vec![GameEvent::PriorityPassed {
            player: PlayerId::One,
        }]
    );
    let failed_upkeep = pass(&mut g4.state, PlayerId::Two);
    assert_eq!(
        failed_upkeep,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::TurnBegan {
                player: PlayerId::One,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            },
            GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: LossReason::EmptyDeckDraw,
            },
        ]
    );
    assert!(
        failed_upkeep
            .iter()
            .all(|event| !matches!(event, GameEvent::ManaProduced { .. })),
        "failed Upkeep draw ends the game before Mana production"
    );
    assert_eq!(
        g4.state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::Two,
            reason: LossReason::EmptyDeckDraw,
        })
    );
    assert_eq!(g4.state.turn.active_player, PlayerId::One);
    assert_eq!(g4.state.turn.phase, Phase::Upkeep);
    assert_eq!(
        g4.state.work,
        VecDeque::from([
            WorkItem::ProduceMana {
                player: PlayerId::One,
                source: ManaSource::Player,
            },
            WorkItem::BeginMainPhase,
        ]),
        "later Upkeep work remains queued after the immediate loss"
    );
    assert!(g4.state.pending.is_none());
    let ended = g4.state.clone();
    assert_eq!(
        apply(
            &g4.state,
            &GameAction::EndTurn {
                player: PlayerId::One,
            },
        ),
        Err(ActionError::GameAlreadyOver)
    );
    assert_eq!(g4.state, ended, "the terminal gate changes no state");
}
