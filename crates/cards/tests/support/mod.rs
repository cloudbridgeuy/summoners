#![allow(dead_code)]

use summoners_cards::{BuiltInCatalog, CardLibrary, Deck};
use summoners_core::{
    domain::{
        errors::InvalidScenario,
        ids::{CardInstanceId, PlayerId},
        state::{CardRef, Coin, GameState, ManaBank, PerPlayer, Readiness},
    },
    scenario::{Scenario, ScenarioPlayer, ScenarioSummon, from_scenario},
};

/// Deterministically assign physical identities to resolved card definitions.
pub struct PhysicalCards<'library> {
    library: &'library CardLibrary,
    next: u32,
}

impl<'library> PhysicalCards<'library> {
    pub const fn new(library: &'library CardLibrary, first_instance: u32) -> Self {
        Self {
            library,
            next: first_instance,
        }
    }

    pub fn one(&mut self, qualified_key: &str) -> CardRef {
        let definition = self
            .library
            .card_id(qualified_key)
            .unwrap_or_else(|| panic!("test definition must exist: {qualified_key}"));
        let card = CardRef {
            instance: CardInstanceId(self.next),
            def: definition,
        };
        self.next = self
            .next
            .checked_add(1)
            .expect("test card instance range must not overflow");
        card
    }

    pub fn many<'key>(
        &mut self,
        qualified_keys: impl IntoIterator<Item = &'key str>,
    ) -> Vec<CardRef> {
        qualified_keys
            .into_iter()
            .map(|key| self.one(key))
            .collect()
    }

    pub fn deck(&mut self, deck: &Deck) -> Vec<CardRef> {
        deck.body()
            .iter()
            .copied()
            .map(|definition| {
                let card = CardRef {
                    instance: CardInstanceId(self.next),
                    def: definition,
                };
                self.next = self
                    .next
                    .checked_add(1)
                    .expect("test card instance range must not overflow");
                card
            })
            .collect()
    }

    pub fn summon<'key>(
        &mut self,
        chain: impl IntoIterator<Item = &'key str>,
        damage: u32,
        ready: bool,
    ) -> ScenarioSummon {
        ScenarioSummon {
            chain: self.many(chain),
            damage,
            readiness: if ready {
                Readiness::Ready
            } else {
                Readiness::Exhausted
            },
        }
    }
}

pub fn player(main: ScenarioSummon) -> ScenarioPlayer {
    ScenarioPlayer {
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBank::default(),
        main_losses: 0,
        main: Some(main),
        bench: [None, None, None],
    }
}

pub fn state(
    catalog: &BuiltInCatalog,
    one: ScenarioPlayer,
    two: ScenarioPlayer,
    active_player: PlayerId,
) -> Result<GameState, InvalidScenario> {
    state_with_coin(catalog, one, two, active_player, None)
}

pub fn state_with_coin(
    catalog: &BuiltInCatalog,
    one: ScenarioPlayer,
    two: ScenarioPlayer,
    active_player: PlayerId,
    coin: Option<Coin>,
) -> Result<GameState, InvalidScenario> {
    from_scenario(
        catalog.library().core_cards(),
        &Scenario {
            players: PerPlayer::new(one, two),
            active_player,
            coin,
        },
    )
}
