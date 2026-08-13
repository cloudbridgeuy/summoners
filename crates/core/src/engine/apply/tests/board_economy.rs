//! Board economy reads, driven end to end through `apply`: a card printing
//! two `Produces` components anchors Mana on the first; a card printing
//! none produces nothing, and no rule complains; a card printing no
//! `RetreatCost` cannot retreat, and that rejection never breaks the game.
//! Shares `tests.rs`'s fixtures through `super::*`, like `demo.rs` and its
//! siblings.

use super::*;
use crate::domain::cards::{CardSet, Component, Entity, EntityId, ManaTypes};
use std::sync::Arc;

/// An id no fixture card in `fixtures::entities()` uses.
fn probe_id(byte: u8) -> EntityId {
    EntityId::parse(&format!("{byte:02x}").repeat(16)).expect("valid probe id")
}

/// Every fixture entity `apply`'s handlers already know how to read, plus
/// `extra`, wrapped in a fresh `CardSet`/`Arc` of its own — the shared
/// fixture `Arc` (`fixtures::card_set()`) is never touched.
fn cards_with(extra: Vec<Entity>) -> Arc<CardSet> {
    let mut entities = fixtures::entities();
    entities.extend(extra);
    Arc::new(CardSet::new(entities))
}

#[test]
fn a_card_printing_two_produces_components_anchors_mana_on_the_first() {
    let id = probe_id(1);
    let entity = Entity {
        id,
        components: vec![
            Component::Produces(ManaTypes(vec![ManaType::Mind])),
            Component::Produces(ManaTypes(vec![ManaType::Spirit])),
        ],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: id,
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).deck = vec![card_ref(100, "quarry-whelp")];

    let outcome = full_end_turn(&state, PlayerId::One)
        .expect("a resting Main hands off cleanly regardless of what Two's board prints");

    assert_eq!(
        outcome.state.pending, None,
        "one anchored Type never pauses"
    );
    let two = outcome.state.players.get(PlayerId::Two);
    assert_eq!(
        two.mana,
        ManaBank {
            matter: 0,
            mind: 1,
            spirit: 0,
        },
        "only the first Produces component (Mind) anchors Mana; the second \
         (Spirit) never counts"
    );
}

#[test]
fn a_card_printing_no_produces_component_produces_nothing_without_complaint() {
    let id = probe_id(2);
    let entity = Entity {
        id,
        components: vec![],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: id,
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).deck = vec![card_ref(100, "quarry-whelp")];

    let outcome = full_end_turn(&state, PlayerId::One)
        .expect("a Summon printing no Produces component at all is still a legal board");

    assert_eq!(outcome.state.status, GameStatus::Playing);
    assert_eq!(outcome.state.pending, None);
    assert_eq!(
        outcome.state.players.get(PlayerId::Two).mana,
        ManaBank::default(),
        "no anchored Type leaves Upkeep's Mana production a documented no-op"
    );
}

#[test]
fn a_card_printing_no_retreat_cost_cannot_retreat_and_does_not_break_the_game() {
    let id = probe_id(3);
    let entity = Entity {
        id,
        // Still prints Produces, so only RetreatCost's absence is on trial.
        components: vec![Component::Produces(ManaTypes(vec![ManaType::Matter]))],
    };
    let mut state = base_state();
    state.cards = cards_with(vec![entity]);
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: id,
            },
            vec![],
        ),
        ..summon(PlayerId::One)
    });
    state.players.get_mut(PlayerId::One).bench[0] = Some(summon(PlayerId::One));

    let result = apply(
        &state,
        &GameAction::Retreat {
            player: PlayerId::One,
            slot: BenchSlot::First,
            mana_hint: None,
        },
    );

    assert_eq!(
        result,
        Err(ActionError::UnknownCard),
        "a missing RetreatCost is read as absent, not demanded, so the \
         action is rejected as illegal instead of breaking the game"
    );
    assert_eq!(
        state.status,
        GameStatus::Playing,
        "the rejected Retreat must never turn a legitimate board Broken"
    );
}
