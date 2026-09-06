use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use summoners_cards::{CardLibrary, Deck};
use summoners_core::domain::cards::EntityId;
use summoners_core::domain::errors::InvalidScenario;
use summoners_core::domain::ids::{CardInstanceId, PlayerId};
use summoners_core::domain::state::{CardRef, Coin, GameState, ManaBank, PerPlayer, Readiness};
use summoners_core::scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario};

const HAND_SIZE: usize = 5;
const PRIZE_COUNT: usize = 2;

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("the opening board is invalid: {0:?}")]
    Scenario(InvalidScenario),
}

pub fn initial_state(
    library: &CardLibrary,
    decks: [&Deck; 2],
    seed: u64,
) -> Result<GameState, SetupError> {
    let mut shuffler = StdRng::seed_from_u64(seed);
    let mut next_instance = 1;
    let one = deal(decks[0], &mut next_instance, &mut shuffler);
    let two = deal(decks[1], &mut next_instance, &mut shuffler);
    let scenario = Scenario {
        players: PerPlayer::new(one, two),
        active_player: PlayerId::One,
        coin: Some(Coin),
    };
    from_scenario(library.core_cards(), &scenario).map_err(SetupError::Scenario)
}

fn deal(deck: &Deck, next_instance: &mut u32, shuffler: &mut StdRng) -> ScenarioPlayer {
    let starter = CardRef {
        instance: mint(next_instance),
        def: deck.starter(),
    };
    let body = shuffled_body(deck.body(), shuffler)
        .into_iter()
        .map(|def| CardRef {
            instance: mint(next_instance),
            def,
        })
        .collect::<Vec<_>>();
    let (hand, rest) = body.split_at(HAND_SIZE);
    let (prizes, deck) = rest.split_at(PRIZE_COUNT);
    ScenarioPlayer {
        deck: deck.to_vec(),
        hand: hand.to_vec(),
        prizes: prizes.to_vec(),
        discard: Vec::new(),
        mana: ManaBank::default(),
        main_losses: 0,
        main: Some(ScenarioSummon {
            chain: vec![starter],
            damage: 0,
            readiness: Readiness::Exhausted,
        }),
        bench: [None, None, None],
    }
}

fn shuffled_body(body: &[EntityId], shuffler: &mut StdRng) -> Vec<EntityId> {
    let mut cards = body.to_vec();
    cards.shuffle(shuffler);
    cards
}

fn mint(next_instance: &mut u32) -> CardInstanceId {
    let card = CardInstanceId(*next_instance);
    *next_instance += 1;
    card
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::collections::HashSet;
    use summoners_cards::built_in_catalog;

    #[test]
    fn seed_sets_zone_composition_and_unique_instances() {
        let catalog = built_in_catalog().expect("catalog");
        let state = initial_state(
            catalog.library(),
            [catalog.set_paths(), catalog.barrow_herd()],
            42,
        )
        .expect("state");
        let one = &state.players.one;
        let two = &state.players.two;
        assert_eq!(
            (one.hand.len(), one.prizes.len(), one.deck.len()),
            (5, 2, 13)
        );
        assert_eq!(
            (two.hand.len(), two.prizes.len(), two.deck.len()),
            (5, 2, 13)
        );
        assert_eq!(state.turn.active_player, PlayerId::One);
        assert_eq!(state.coin, Some(Coin));
        let ids = [one, two]
            .into_iter()
            .flat_map(|player| {
                player
                    .hand
                    .iter()
                    .chain(&player.prizes)
                    .chain(&player.deck)
                    .chain(player.main.iter().flat_map(|summon| summon.chain.layers()))
                    .chain(
                        player
                            .bench
                            .iter()
                            .flatten()
                            .flat_map(|summon| summon.chain.layers()),
                    )
            })
            .map(|card| card.instance)
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 42);
    }

    #[test]
    fn one_seed_repeats_the_opening() {
        let catalog = built_in_catalog().expect("catalog");
        let decks = [catalog.set_paths(), catalog.barrow_herd()];
        assert_eq!(
            initial_state(catalog.library(), decks, 99).expect("left"),
            initial_state(catalog.library(), decks, 99).expect("right")
        );
    }
}
