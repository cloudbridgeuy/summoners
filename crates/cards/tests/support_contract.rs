#![allow(clippy::expect_used)]

mod support;

use std::sync::Arc;

use summoners_cards::built_in_catalog;
use summoners_core::domain::ids::PlayerId;

use support::{PhysicalCards, player, state};

#[test]
fn physical_card_helper_expands_a_real_deck_in_order_with_unique_instances() {
    let catalog = built_in_catalog().expect("catalog is valid");
    let mut physical = PhysicalCards::new(catalog.library(), 100);
    let cards = physical.deck(catalog.set_paths());

    assert_eq!(cards.len(), 20);
    assert_eq!(cards[0].instance.0, 100);
    assert_eq!(cards[19].instance.0, 119);
    assert_eq!(cards[0].def, catalog.set_paths().body()[0]);
    assert_eq!(cards[19].def, catalog.set_paths().body()[19]);
}

#[test]
fn state_helper_uses_scenario_and_preserves_the_library_arc() {
    let catalog = built_in_catalog().expect("catalog is valid");
    let mut physical = PhysicalCards::new(catalog.library(), 1);
    let one = player(physical.summon(["foundations/warden-initiate"], 5, true));
    let two = player(physical.summon(["foundations/sow-piglet"], 0, false));

    let state = state(&catalog, one, two, PlayerId::One).expect("scenario is valid");

    assert!(Arc::ptr_eq(&state.cards, &catalog.library().core_cards()));
    assert_eq!(
        state.players.one.main.as_ref().map(|summon| summon.damage),
        Some(5)
    );
    assert_eq!(
        state.players.two.main.as_ref().map(|summon| summon.ready),
        Some(false)
    );
}
