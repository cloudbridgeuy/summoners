//! `Scenario`: a plain description of any representable match situation, and
//! `from_scenario`, the parser that turns it into a `GameState` or a typed
//! `InvalidScenario` (decision 17; `~/.claude/patterns/parse-dont-validate.md`).
//!
//! Parsing accepts any board it can interpret. It rejects only the five
//! shapes it cannot: a repeated card instance, a card naming no known
//! fixture, an upgrade chain with no cards in it, a chain that does not
//! climb Base, Enhanced, Elite in order, and a player with no Summon
//! anywhere on the board. It enforces no deck size, no copy limit, and no
//! hand size — a wrapper may add that strictness later. The first version
//! accepts quiescent boards only: no Stack, no open Priority window, no
//! pending decision besides the promotion an empty Main derives on its own.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use crate::domain::cards::{CardSet, Form};
use crate::domain::errors::InvalidScenario;
use crate::domain::ids::{BenchSlot, PlayerId, Position};
use crate::domain::state::{
    CardRef, GameState, ManaBank, PendingInput, PerPlayer, Phase, PlayerState, SummonInstance,
    TurnState, UpgradeChain,
};

/// One Summon as a scenario describes it: its printed chain, bottom to top,
/// its accumulated Damage, and whether it is Ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioSummon {
    pub chain: Vec<CardRef>,
    pub damage: u32,
    pub ready: bool,
}

/// One player's board as a scenario describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioPlayer {
    pub deck: Vec<CardRef>,
    pub hand: Vec<CardRef>,
    pub prizes: Vec<CardRef>,
    pub discard: Vec<CardRef>,
    pub mana: ManaBank,
    pub main_losses: u8,
    pub has_coin: bool,
    pub main: Option<ScenarioSummon>,
    pub bench: [Option<ScenarioSummon>; 3],
}

/// A complete match situation, ready to parse into a `GameState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    pub players: PerPlayer<ScenarioPlayer>,
    pub active_player: PlayerId,
}

/// Every card reference in one player's zones and chains.
fn player_card_refs(player: &ScenarioPlayer) -> Vec<CardRef> {
    let mut refs = Vec::new();
    refs.extend(player.deck.iter().copied());
    refs.extend(player.hand.iter().copied());
    refs.extend(player.prizes.iter().copied());
    refs.extend(player.discard.iter().copied());
    for summon in positioned_summons(player).into_iter().map(|(_, s)| s) {
        refs.extend(summon.chain.iter().copied());
    }
    refs
}

/// Every card reference across the whole board.
fn all_card_refs(scenario: &Scenario) -> Vec<CardRef> {
    let mut refs = player_card_refs(&scenario.players.one);
    refs.extend(player_card_refs(&scenario.players.two));
    refs
}

/// Reject the first `CardInstanceId` seen twice anywhere on the board.
fn check_no_duplicate_instances(scenario: &Scenario) -> Result<(), InvalidScenario> {
    let mut seen = HashSet::new();
    for card in all_card_refs(scenario) {
        if !seen.insert(card.instance) {
            return Err(InvalidScenario::DuplicateCardInstance(card.instance));
        }
    }
    Ok(())
}

/// Reject the first card whose `EntityId` `cards` has no entity for.
fn check_every_def_known(cards: &CardSet, scenario: &Scenario) -> Result<(), InvalidScenario> {
    for card in all_card_refs(scenario) {
        if cards.get(card.def).is_none() {
            return Err(InvalidScenario::UnknownCardDef(card.def));
        }
    }
    Ok(())
}

/// Every occupied position on one player's board, Main first.
fn positioned_summons(player: &ScenarioPlayer) -> Vec<(Position, &ScenarioSummon)> {
    let mut items = Vec::new();
    if let Some(summon) = &player.main {
        items.push((Position::Main, summon));
    }
    for slot in BenchSlot::ALL {
        if let Some(summon) = &player.bench[slot.index()] {
            items.push((Position::Bench(slot), summon));
        }
    }
    items
}

/// The Form printed on one card, read straight off its entity, or the
/// chain-order error this card causes if it prints no Form at all. This is
/// the one place in this crate that reads a `CardSet` and its entities
/// directly rather than through the compatibility shim.
fn form_of(
    cards: &CardSet,
    card: CardRef,
    player: PlayerId,
    position: Position,
) -> Result<Form, InvalidScenario> {
    let entity = cards
        .get(card.def)
        .ok_or(InvalidScenario::UnknownCardDef(card.def))?;
    entity
        .get::<Form>()
        .copied()
        .ok_or(InvalidScenario::IllegalChainOrder { player, position })
}

/// Validate and build one Summon's upgrade chain in the same pass: an empty
/// chain and an out-of-order chain can never become an `UpgradeChain` value
/// (make impossible states impossible).
fn build_chain(
    cards: &CardSet,
    player: PlayerId,
    position: Position,
    refs: &[CardRef],
) -> Result<UpgradeChain, InvalidScenario> {
    let Some((base, rest)) = refs.split_first() else {
        return Err(InvalidScenario::EmptyUpgradeChain { player, position });
    };

    let mut previous = form_of(cards, *base, player, position)?;
    for card in rest {
        let form = form_of(cards, *card, player, position)?;
        if form <= previous {
            return Err(InvalidScenario::IllegalChainOrder { player, position });
        }
        previous = form;
    }

    Ok(UpgradeChain::new(*base, rest.to_vec()))
}

/// Build one Summon at `position`, or fail with the chain error it caused.
fn build_summon_instance(
    cards: &CardSet,
    player: PlayerId,
    position: Position,
    summon: &ScenarioSummon,
) -> Result<SummonInstance, InvalidScenario> {
    let chain = build_chain(cards, player, position, &summon.chain)?;
    Ok(SummonInstance {
        chain,
        damage: summon.damage,
        ready: summon.ready,
        owner: player,
        controller: player,
        duration_markers: vec![],
        played_this_turn: false,
        upgraded_this_turn: false,
        entered_main_this_turn: false,
    })
}

/// Build one player's zones, or fail with the first violation found on
/// their board: an empty or misordered chain, or no Summon anywhere.
fn build_player_state(
    cards: &CardSet,
    player: PlayerId,
    scenario_player: &ScenarioPlayer,
) -> Result<PlayerState, InvalidScenario> {
    let main = scenario_player
        .main
        .as_ref()
        .map(|summon| build_summon_instance(cards, player, Position::Main, summon))
        .transpose()?;

    let mut bench: [Option<SummonInstance>; 3] = [None, None, None];
    for slot in BenchSlot::ALL {
        if let Some(summon) = &scenario_player.bench[slot.index()] {
            let position = Position::Bench(slot);
            bench[slot.index()] = Some(build_summon_instance(cards, player, position, summon)?);
        }
    }

    if main.is_none() && bench.iter().all(Option::is_none) {
        return Err(InvalidScenario::NoSummonInPlay { player });
    }

    Ok(PlayerState {
        main,
        bench,
        deck: scenario_player.deck.clone(),
        hand: scenario_player.hand.clone(),
        prizes: scenario_player.prizes.clone(),
        discard: scenario_player.discard.clone(),
        mana: scenario_player.mana,
        main_losses: scenario_player.main_losses,
        has_coin: scenario_player.has_coin,
        // `Scenario` carries no field for a starting Enchantment yet, so
        // parsing can never seed one; every parsed board starts with none.
        enchantments: vec![],
    })
}

/// An empty Main with a non-empty Bench pauses the parsed state on a
/// promotion; the first such player found, in seat order, is the one
/// answering it. A real game only ever reaches this shape for one player at
/// a time.
fn derive_pending(players: &PerPlayer<PlayerState>) -> Option<PendingInput> {
    for player in [PlayerId::One, PlayerId::Two] {
        if players.get(player).main.is_none() {
            return Some(PendingInput::Promotion { player });
        }
    }
    None
}

/// Parse a `Scenario` into a `GameState`, or report the first shape it
/// cannot interpret. `cards` is the authored card pool this match will read
/// facts from for the rest of its life; the returned state holds the same
/// `Arc` handle.
pub fn from_scenario(
    cards: Arc<CardSet>,
    scenario: &Scenario,
) -> Result<GameState, InvalidScenario> {
    check_no_duplicate_instances(scenario)?;
    check_every_def_known(&cards, scenario)?;

    let one = build_player_state(&cards, PlayerId::One, &scenario.players.one)?;
    let two = build_player_state(&cards, PlayerId::Two, &scenario.players.two)?;
    let players = PerPlayer::new(one, two);
    let pending = derive_pending(&players);

    Ok(GameState {
        players,
        turn: TurnState {
            active_player: scenario.active_player,
            phase: Phase::Main,
            window: None,
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: false,
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: VecDeque::new(),
        pending,
        outcome: None,
        cards,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::domain::cards::fixtures;
    use crate::domain::ids::CardInstanceId;

    fn card(instance: u32, def: &'static str) -> CardRef {
        CardRef {
            instance: CardInstanceId(instance),
            def: fixtures::id(def),
        }
    }

    /// A card reference naming an id no fixture in `fixtures::card_set`
    /// carries — every fixture id is built from a small card number
    /// (`fixtures::fid`'s `card * 1000 + ability` shape stays under
    /// 25000), so an id built from digits alone is guaranteed absent.
    fn unknown_card(instance: u32) -> CardRef {
        CardRef {
            instance: CardInstanceId(instance),
            def: crate::domain::cards::EntityId::parse(&"9".repeat(32)).expect("valid probe id"),
        }
    }

    fn chain_summon(chain: Vec<(u32, &'static str)>) -> ScenarioSummon {
        ScenarioSummon {
            chain: chain
                .into_iter()
                .map(|(instance, def)| card(instance, def))
                .collect(),
            damage: 0,
            ready: true,
        }
    }

    fn minimal_player_one() -> ScenarioPlayer {
        ScenarioPlayer {
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            has_coin: false,
            main: Some(chain_summon(vec![(1, "quarry-whelp")])),
            bench: [None, None, None],
        }
    }

    fn minimal_player_two() -> ScenarioPlayer {
        ScenarioPlayer {
            main: Some(chain_summon(vec![(101, "set-path-adept")])),
            ..minimal_player_one()
        }
    }

    fn base_scenario() -> Scenario {
        Scenario {
            players: PerPlayer::new(minimal_player_one(), minimal_player_two()),
            active_player: PlayerId::One,
        }
    }

    #[test]
    fn a_minimal_scenario_parses_with_no_pending_decision() {
        let scenario = base_scenario();

        let state = from_scenario(fixtures::card_set(), &scenario)
            .expect("a minimal scenario should parse");

        assert_eq!(state.pending, None);
        assert_eq!(state.outcome, None);
        assert_eq!(state.turn.active_player, PlayerId::One);
    }

    #[test]
    fn a_duplicate_card_instance_across_players_is_rejected() {
        let mut scenario = base_scenario();
        // Player Two's hand reuses the instance id already on Player One's
        // Main Summon.
        scenario.players.two.hand.push(card(1, "set-path-adept"));

        assert_eq!(
            from_scenario(fixtures::card_set(), &scenario),
            Err(InvalidScenario::DuplicateCardInstance(CardInstanceId(1)))
        );
    }

    #[test]
    fn a_card_naming_no_known_fixture_is_rejected() {
        let mut scenario = base_scenario();
        let absent = unknown_card(999);
        scenario.players.one.hand.push(absent);

        assert_eq!(
            from_scenario(fixtures::card_set(), &scenario),
            Err(InvalidScenario::UnknownCardDef(absent.def))
        );
    }

    #[test]
    fn a_summon_described_with_no_cards_in_its_chain_is_rejected() {
        let mut scenario = base_scenario();
        scenario.players.one.main = Some(ScenarioSummon {
            chain: vec![],
            damage: 0,
            ready: true,
        });

        assert_eq!(
            from_scenario(fixtures::card_set(), &scenario),
            Err(InvalidScenario::EmptyUpgradeChain {
                player: PlayerId::One,
                position: Position::Main,
            })
        );
    }

    #[test]
    fn a_chain_that_does_not_climb_base_enhanced_elite_is_rejected() {
        let mut scenario = base_scenario();
        scenario.players.one.main =
            Some(chain_summon(vec![(1, "quarry-brute"), (2, "quarry-whelp")]));

        assert_eq!(
            from_scenario(fixtures::card_set(), &scenario),
            Err(InvalidScenario::IllegalChainOrder {
                player: PlayerId::One,
                position: Position::Main,
            })
        );
    }

    #[test]
    fn a_repeated_form_in_one_chain_is_also_an_illegal_order() {
        let mut scenario = base_scenario();
        scenario.players.one.main =
            Some(chain_summon(vec![(1, "quarry-whelp"), (2, "quarry-whelp")]));

        assert_eq!(
            from_scenario(fixtures::card_set(), &scenario),
            Err(InvalidScenario::IllegalChainOrder {
                player: PlayerId::One,
                position: Position::Main,
            })
        );
    }

    #[test]
    fn an_empty_main_and_an_empty_bench_is_rejected() {
        let mut scenario = base_scenario();
        scenario.players.one.main = None;

        assert_eq!(
            from_scenario(fixtures::card_set(), &scenario),
            Err(InvalidScenario::NoSummonInPlay {
                player: PlayerId::One
            })
        );
    }

    #[test]
    fn an_empty_main_with_a_bench_summon_parses_pending_a_promotion() {
        let mut scenario = base_scenario();
        scenario.players.one.main = None;
        scenario.players.one.bench[0] = Some(chain_summon(vec![(1, "quarry-whelp")]));

        let state = from_scenario(fixtures::card_set(), &scenario)
            .expect("an empty Main with a Bench should parse");

        assert_eq!(
            state.pending,
            Some(PendingInput::Promotion {
                player: PlayerId::One
            })
        );
        assert!(state.players.get(PlayerId::One).main.is_none());
        assert!(state.players.get(PlayerId::One).bench[0].is_some());
    }

    #[test]
    fn a_representative_scenario_round_trips_zones_banks_and_flags() {
        let mut scenario = base_scenario();
        scenario.players.one.hand = vec![card(10, "set-path-adept")];
        scenario.players.one.deck = vec![card(11, "set-path-adept"), card(12, "set-path-warden")];
        scenario.players.one.prizes = vec![card(13, "quarry-brute")];
        scenario.players.one.discard = vec![card(14, "quarry-brute")];
        scenario.players.one.mana = ManaBank {
            matter: 2,
            mind: 1,
            spirit: 0,
        };
        scenario.players.one.main_losses = 1;
        scenario.players.one.bench[0] = Some(chain_summon(vec![(15, "set-path-adept")]));
        scenario.players.two.has_coin = true;
        scenario.active_player = PlayerId::Two;

        let state = from_scenario(fixtures::card_set(), &scenario)
            .expect("a fully populated scenario should still parse");

        let one = state.players.get(PlayerId::One);
        assert_eq!(one.hand, scenario.players.one.hand);
        assert_eq!(one.deck, scenario.players.one.deck);
        assert_eq!(one.prizes, scenario.players.one.prizes);
        assert_eq!(one.discard, scenario.players.one.discard);
        assert_eq!(one.mana, scenario.players.one.mana);
        assert_eq!(one.main_losses, 1);
        assert!(!one.has_coin);
        assert!(one.bench[0].is_some());
        assert_eq!(
            one.main.as_ref().map(|summon| summon.chain.top().def),
            Some(fixtures::id("quarry-whelp"))
        );

        let two = state.players.get(PlayerId::Two);
        assert!(two.has_coin);

        assert_eq!(state.turn.active_player, PlayerId::Two);
        assert_eq!(state.turn.phase, Phase::Main);
        assert_eq!(state.pending, None);
        assert_eq!(state.outcome, None);
        assert!(state.stack.is_empty());
        assert!(state.work.is_empty());
    }
}
