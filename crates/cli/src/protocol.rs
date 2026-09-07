use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use summoners_core::domain::{
    actions::GameAction,
    cards::{
        Attack, CardSet, Cost, EffectLeaf, EntityId, Life, ManaTypes, Name, RetreatCost, Skill,
    },
    events::GameEvent,
    ids::{BenchSlot, CardInstanceId, ManaType, PlayerId, Position},
    state::{
        GameOutcome, GameState, GameStatus, PendingInput, Phase, Readiness, StackItem,
        SummonInstance,
    },
};
use summoners_match_log::ActionV1;

pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Seat {
    One,
    Two,
}

impl From<Seat> for PlayerId {
    fn from(value: Seat) -> Self {
        match value {
            Seat::One => Self::One,
            Seat::Two => Self::Two,
        }
    }
}
impl From<PlayerId> for Seat {
    fn from(value: PlayerId) -> Self {
        match value {
            PlayerId::One => Self::One,
            PlayerId::Two => Self::Two,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientEnvelope {
    Join {
        version: u32,
        seat: Seat,
    },
    Submit {
        version: u32,
        request_id: u64,
        based_on_revision: u64,
        action: ActionV1,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerEnvelope {
    Waiting {
        seat: Seat,
    },
    Update {
        revision: u64,
        view: PlayerView,
        notices: Vec<Notice>,
        reply: Option<u64>,
        result: Option<SubmissionResult>,
    },
    Finished {
        outcome: OutcomeView,
        view: PlayerView,
        notices: Vec<Notice>,
        reply: Option<u64>,
    },
    Stopped {
        reason: String,
    },
    Rejected {
        request_id: Option<u64>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionResult {
    Accepted,
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDescription {
    pub name: String,
    pub life: Option<u32>,
    pub retreat_cost: Option<u32>,
    pub mana_types: Vec<String>,
    pub cost: Option<String>,
    pub abilities: Vec<String>,
    pub effects: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CardDescriptions(HashMap<EntityId, CardDescription>);

impl CardDescriptions {
    pub fn from_card_set(cards: &CardSet) -> Self {
        Self(
            cards
                .entities()
                .iter()
                .map(|card| (card.id, describe_card(card)))
                .collect(),
        )
    }

    fn get(&self, definition: EntityId) -> CardDescription {
        self.0.get(&definition).cloned().unwrap_or(CardDescription {
            name: "Unknown card".to_string(),
            life: None,
            retreat_cost: None,
            mana_types: Vec::new(),
            cost: None,
            abilities: Vec::new(),
            effects: Vec::new(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct CardIdentityMap(HashMap<CardInstanceId, CardDescription>);

impl CardIdentityMap {
    pub fn from_initial_state(state: &GameState, descriptions: &CardDescriptions) -> Self {
        let mut cards = HashMap::new();
        for player in [&state.players.one, &state.players.two] {
            for card in player
                .deck
                .iter()
                .chain(&player.hand)
                .chain(&player.prizes)
                .chain(&player.discard)
                .chain(&player.enchantments)
            {
                cards.insert(card.instance, descriptions.get(card.def));
            }
            for summon in player.main.iter().chain(player.bench.iter().flatten()) {
                for card in summon.chain.layers() {
                    cards.insert(card.instance, descriptions.get(card.def));
                }
            }
        }
        Self(cards)
    }

    fn name(&self, card: CardInstanceId) -> String {
        self.0
            .get(&card)
            .map(|description| description.name.clone())
            .unwrap_or_else(|| "Unknown card".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub text: String,
}

pub fn player_notices(
    events: &[GameEvent],
    identities: &CardIdentityMap,
    viewer: PlayerId,
) -> Vec<Notice> {
    events
        .iter()
        .map(|event| Notice {
            text: notice_text(event, identities, viewer),
        })
        .collect()
}

fn notice_text(event: &GameEvent, identities: &CardIdentityMap, viewer: PlayerId) -> String {
    match event {
        GameEvent::TurnBegan { player } => format!("{} began a turn", player_text(*player)),
        GameEvent::SummonsReadied { player, positions } => format!(
            "{} readied {}",
            player_text(*player),
            positions
                .iter()
                .map(|position| position_text(*position))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        GameEvent::CardDrawn { player, card } => {
            private_card_notice("drew", *player, *card, identities, viewer)
        }
        GameEvent::ManaProduced {
            player, mana_type, ..
        } => format!(
            "{} produced {} Mana",
            player_text(*player),
            mana_text(*mana_type)
        ),
        GameEvent::SummonPlayed { player, card, slot } => format!(
            "{} played {} to {}",
            player_text(*player),
            identities.name(*card),
            bench_text(*slot)
        ),
        GameEvent::SummonUpgraded {
            player,
            card,
            position,
        } => format!(
            "{} upgraded {} with {}",
            player_text(*player),
            position_text(*position),
            identities.name(*card)
        ),
        GameEvent::SpellCast { player, card, .. } => {
            format!("{} cast {}", player_text(*player), identities.name(*card))
        }
        GameEvent::SkillActivated {
            player, position, ..
        } => format!(
            "{} activated a Skill at {}",
            player_text(*player),
            position_text(*position)
        ),
        GameEvent::AttackDeclared { player, target } => format!(
            "{} declared an attack at {}",
            player_text(*player),
            position_text(*target)
        ),
        GameEvent::PriorityPassed { player } => format!("{} passed Priority", player_text(*player)),
        GameEvent::StackItemResolved { .. } => "A Stack item resolved".to_string(),
        GameEvent::DamageCalculationStarted { base, .. } => {
            format!("Damage calculation began at {base}")
        }
        GameEvent::DamageAdjustmentApplied { output, .. } => format!("Damage adjusted to {output}"),
        GameEvent::DamageAdjustmentSkipped { .. } => "A damage adjustment was skipped".to_string(),
        GameEvent::DamageApplied {
            amount,
            before,
            after,
            ..
        } => format!("Damage {amount} applied: {before} to {after}"),
        GameEvent::Healed { position, amount } => {
            format!("{} healed for {amount}", position_text(*position))
        }
        GameEvent::SummonDestroyed { position, .. } => {
            format!("{} was destroyed", position_text(*position))
        }
        GameEvent::PrizeRecovered { player, card } => {
            private_card_notice("recovered a Prize", *player, *card, identities, viewer)
        }
        GameEvent::PrizesViewed { player, prizes } => {
            if *player == viewer {
                format!(
                    "{} viewed Prizes: {}",
                    player_text(*player),
                    prizes
                        .iter()
                        .map(|card| identities.name(*card))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{} viewed {} Prizes", player_text(*player), prizes.len())
            }
        }
        GameEvent::SummonPromoted { player, from } => {
            format!("{} promoted {}", player_text(*player), bench_text(*from))
        }
        GameEvent::SummonsSwapped { player, main } => format!(
            "{} swapped Main with {}",
            player_text(*player),
            bench_text(*main)
        ),
        GameEvent::TriggerFired {
            controller,
            position,
            ..
        } => format!(
            "{} triggered an ability at {}",
            player_text(*controller),
            position_text(*position)
        ),
        GameEvent::CoinConverted { player, mana_type } => format!(
            "{} converted Coin to {} Mana",
            player_text(*player),
            mana_text(*mana_type)
        ),
        GameEvent::ManaDeducted {
            player,
            mana_type,
            amount,
        } => format!(
            "{} spent {amount} {} Mana",
            player_text(*player),
            mana_text(*mana_type)
        ),
        GameEvent::GameEnded { winner, .. } => format!("{} won the game", player_text(*winner)),
    }
}

fn private_card_notice(
    action: &str,
    player: PlayerId,
    card: CardInstanceId,
    identities: &CardIdentityMap,
    viewer: PlayerId,
) -> String {
    if player == viewer {
        format!(
            "{} {} {}",
            player_text(player),
            action,
            identities.name(card)
        )
    } else {
        format!("{} {} a card", player_text(player), action)
    }
}
fn player_text(player: PlayerId) -> &'static str {
    match player {
        PlayerId::One => "Player One",
        PlayerId::Two => "Player Two",
    }
}
fn bench_text(slot: BenchSlot) -> &'static str {
    match slot {
        BenchSlot::First => "Bench 1",
        BenchSlot::Second => "Bench 2",
        BenchSlot::Third => "Bench 3",
    }
}
fn mana_text(mana: ManaType) -> &'static str {
    match mana {
        ManaType::Matter => "Matter",
        ManaType::Mind => "Mind",
        ManaType::Spirit => "Spirit",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandCardView {
    pub instance: u32,
    pub card: CardDescription,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicCardView {
    pub card: CardDescription,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummonView {
    pub chain: Vec<CardDescription>,
    pub damage: u32,
    pub readiness: ReadinessView,
    pub owner: Seat,
    pub controller: Seat,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessView {
    Ready,
    Exhausted,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardView {
    pub main: Option<SummonView>,
    pub bench: [Option<SummonView>; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaView {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerPublicView {
    pub board: BoardView,
    pub mana: ManaView,
    pub main_losses: u8,
    pub discard: Vec<PublicCardView>,
    pub persistent: Vec<PublicCardView>,
    pub deck_count: usize,
    pub prize_count: usize,
    pub hand_count: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatsView<T> {
    pub one: T,
    pub two: T,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseView {
    Upkeep,
    Main,
    Combat,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StackView {
    Attack {
        attacker: Seat,
        target: String,
    },
    Spell {
        caster: Seat,
        card: CardDescription,
        targets: Vec<String>,
    },
    Trigger {
        controller: Seat,
        source: String,
        event: String,
        targets: Vec<String>,
        effects: Vec<String>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingKindView {
    ManaProduction,
    Promotion,
    PrizePick,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingView {
    pub kind: PendingKindView,
    pub owner: Seat,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeReasonView {
    ThirdMainLoss,
    NoPromotionAvailable,
    EmptyDeckDraw,
    Resignation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeView {
    pub winner: Seat,
    pub reason: OutcomeReasonView,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerView {
    pub you: Seat,
    pub hand: Vec<HandCardView>,
    pub players: SeatsView<PlayerPublicView>,
    pub coin: bool,
    pub stack: Vec<StackView>,
    pub phase: PhaseView,
    pub active_player: Seat,
    pub priority_holder: Option<Seat>,
    pub pending: Option<PendingView>,
    pub outcome: Option<OutcomeView>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProtocolError {
    Actor,
    Conversion(String),
}

pub fn normalize_action(seat: PlayerId, action: ActionV1) -> Result<GameAction, ProtocolError> {
    let action = GameAction::try_from(action)
        .map_err(|error| ProtocolError::Conversion(error.to_string()))?;
    if action.actor() == seat {
        Ok(action)
    } else {
        Err(ProtocolError::Actor)
    }
}

pub fn player_view(
    state: &GameState,
    viewer: PlayerId,
    descriptions: &CardDescriptions,
) -> PlayerView {
    PlayerView {
        you: viewer.into(),
        hand: state
            .players
            .get(viewer)
            .hand
            .iter()
            .map(|card| HandCardView {
                instance: card.instance.0,
                card: descriptions.get(card.def),
            })
            .collect(),
        players: SeatsView {
            one: public_player(&state.players.one, descriptions),
            two: public_player(&state.players.two, descriptions),
        },
        coin: state.coin.is_some(),
        stack: state
            .stack
            .iter()
            .map(|item| stack_view(item, descriptions))
            .collect(),
        phase: phase_view(state.turn.phase),
        active_player: state.turn.active_player.into(),
        priority_holder: state.turn.window.map(|window| window.holder.into()),
        pending: state.pending.map(pending_view),
        outcome: match state.status {
            GameStatus::Ended(outcome) => Some(outcome_view(outcome)),
            GameStatus::Playing | GameStatus::Broken(_) => None,
        },
    }
}

fn describe_card(card: &summoners_core::domain::cards::Entity) -> CardDescription {
    CardDescription {
        name: card
            .get::<Name>()
            .map_or_else(|| "Unnamed card".to_string(), |name| name.0.clone()),
        life: card.get::<Life>().map(|life| life.0),
        retreat_cost: card.get::<RetreatCost>().map(|cost| cost.0),
        mana_types: card.get::<ManaTypes>().map_or_else(Vec::new, |types| {
            types.0.iter().map(|mana| format!("{mana:?}")).collect()
        }),
        cost: card.get::<Cost>().map(cost_text),
        abilities: card
            .all::<Skill>()
            .into_iter()
            .chain(card.all::<Attack>())
            .map(describe_ability)
            .collect(),
        effects: card
            .all::<EffectLeaf>()
            .into_iter()
            .map(effect_text)
            .collect(),
    }
}

fn public_player(
    player: &summoners_core::domain::state::PlayerState,
    descriptions: &CardDescriptions,
) -> PlayerPublicView {
    PlayerPublicView {
        board: BoardView {
            main: player
                .main
                .as_ref()
                .map(|summon| summon_view(summon, descriptions)),
            bench: player.bench.each_ref().map(|summon| {
                summon
                    .as_ref()
                    .map(|summon| summon_view(summon, descriptions))
            }),
        },
        mana: ManaView {
            matter: player.mana.matter,
            mind: player.mana.mind,
            spirit: player.mana.spirit,
        },
        main_losses: player.main_losses,
        discard: player
            .discard
            .iter()
            .map(|card| PublicCardView {
                card: descriptions.get(card.def),
            })
            .collect(),
        persistent: player
            .enchantments
            .iter()
            .map(|card| PublicCardView {
                card: descriptions.get(card.def),
            })
            .collect(),
        deck_count: player.deck.len(),
        prize_count: player.prizes.len(),
        hand_count: player.hand.len(),
    }
}
fn summon_view(summon: &SummonInstance, descriptions: &CardDescriptions) -> SummonView {
    SummonView {
        chain: summon
            .chain
            .layers()
            .map(|card| descriptions.get(card.def))
            .collect(),
        damage: summon.damage,
        readiness: readiness_view(summon.readiness),
        owner: summon.owner.into(),
        controller: summon.controller.into(),
    }
}
fn stack_view(item: &StackItem, descriptions: &CardDescriptions) -> StackView {
    match item {
        StackItem::Attack { attacker, target } => StackView::Attack {
            attacker: (*attacker).into(),
            target: position_text(*target),
        },
        StackItem::Spell {
            caster,
            card,
            targets,
        } => StackView::Spell {
            caster: (*caster).into(),
            card: descriptions.get(card.def),
            targets: targets
                .iter()
                .map(|target| position_text(*target))
                .collect(),
        },
        StackItem::Trigger {
            controller,
            source,
            event,
            targets,
            effects,
            ..
        } => StackView::Trigger {
            controller: (*controller).into(),
            source: position_text(*source),
            event: format!("{event:?}"),
            targets: targets
                .iter()
                .map(|target| position_text(*target))
                .collect(),
            effects: effects.iter().map(effect_text).collect(),
        },
    }
}
fn position_text(position: Position) -> String {
    format!("{position:?}")
}
fn cost_text(cost: &Cost) -> String {
    format!(
        "matter {} mind {} spirit {} generic {}",
        cost.matter, cost.mind, cost.spirit, cost.generic
    )
}
fn describe_ability(ability: &summoners_core::domain::cards::Entity) -> String {
    let name = ability
        .get::<Name>()
        .map_or_else(|| "Unnamed ability".to_string(), |name| name.0.clone());
    let effects = ability
        .all::<EffectLeaf>()
        .into_iter()
        .map(effect_text)
        .collect::<Vec<_>>();
    if effects.is_empty() {
        name
    } else {
        format!("{name}: {}", effects.join(", "))
    }
}
fn effect_text(effect: &EffectLeaf) -> String {
    match effect {
        EffectLeaf::DealDamage(damage) => format!("deal {} damage", damage.base),
        EffectLeaf::Heal { amount, .. } => format!("heal {amount}"),
        EffectLeaf::MoveSummon => "move a Summon".to_string(),
        EffectLeaf::SwapPositions => "swap positions".to_string(),
        EffectLeaf::BlockResponses { .. } => "block responses".to_string(),
        EffectLeaf::ReturnSpellFromDiscard => "return a Spell from discard".to_string(),
        EffectLeaf::LookAtPrizes => "look at Prizes".to_string(),
        EffectLeaf::DrawCards { amount } => format!("draw {amount} cards"),
        EffectLeaf::ReturnSpellToDeckTop => "return a Spell to deck top".to_string(),
        EffectLeaf::ProduceMana { .. } => "produce Mana".to_string(),
        EffectLeaf::CannotBeMovedByOpponent { .. } => "prevent opponent movement".to_string(),
        EffectLeaf::ReadySummon => "ready a Summon".to_string(),
        EffectLeaf::SwapOpposingPositions => "swap opposing positions".to_string(),
    }
}
fn readiness_view(readiness: Readiness) -> ReadinessView {
    match readiness {
        Readiness::Ready => ReadinessView::Ready,
        Readiness::Exhausted => ReadinessView::Exhausted,
    }
}
fn phase_view(phase: Phase) -> PhaseView {
    match phase {
        Phase::Upkeep => PhaseView::Upkeep,
        Phase::Main => PhaseView::Main,
        Phase::Combat => PhaseView::Combat,
    }
}
fn pending_view(pending: PendingInput) -> PendingView {
    match pending {
        PendingInput::ManaProduction { player, .. } => PendingView {
            kind: PendingKindView::ManaProduction,
            owner: player.into(),
        },
        PendingInput::Promotion { player } => PendingView {
            kind: PendingKindView::Promotion,
            owner: player.into(),
        },
        PendingInput::PrizePick { chooser } => PendingView {
            kind: PendingKindView::PrizePick,
            owner: chooser.into(),
        },
    }
}
fn outcome_view(outcome: GameOutcome) -> OutcomeView {
    OutcomeView {
        winner: outcome.winner.into(),
        reason: match outcome.reason {
            summoners_core::domain::state::LossReason::ThirdMainLoss => {
                OutcomeReasonView::ThirdMainLoss
            }
            summoners_core::domain::state::LossReason::NoPromotionAvailable => {
                OutcomeReasonView::NoPromotionAvailable
            }
            summoners_core::domain::state::LossReason::EmptyDeckDraw => {
                OutcomeReasonView::EmptyDeckDraw
            }
            summoners_core::domain::state::LossReason::Resignation => {
                OutcomeReasonView::Resignation
            }
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::setup::initial_state;
    use summoners_cards::built_in_catalog;

    fn state_and_descriptions() -> (GameState, CardDescriptions) {
        let catalog = built_in_catalog().expect("catalog");
        let state = initial_state(
            catalog.library(),
            [catalog.set_paths(), catalog.barrow_herd()],
            4,
        )
        .expect("state");
        let descriptions = CardDescriptions::from_card_set(&state.cards);
        (state, descriptions)
    }

    #[test]
    fn view_exposes_own_hand_and_hides_all_known_opponent_private_ids() {
        let (state, descriptions) = state_and_descriptions();
        for (viewer, own, opponent) in [
            (PlayerId::One, &state.players.one, &state.players.two),
            (PlayerId::Two, &state.players.two, &state.players.one),
        ] {
            let encoded =
                serde_json::to_string(&player_view(&state, viewer, &descriptions)).expect("view");
            for card in &own.hand {
                assert!(encoded.contains(&format!("\"instance\":{},\"card\"", card.instance.0)));
            }
            for card in own
                .deck
                .iter()
                .chain(&own.prizes)
                .chain(&opponent.hand)
                .chain(&opponent.deck)
                .chain(&opponent.prizes)
            {
                assert!(
                    !encoded.contains(&format!("\"instance\":{},\"card\"", card.instance.0)),
                    "{}",
                    card.instance.0
                );
            }
            assert!(!encoded.contains("stack_segment_bases"));
            assert!(!encoded.contains("work"));
        }
    }

    #[test]
    fn card_descriptions_include_readable_name_and_stats() {
        let (state, descriptions) = state_and_descriptions();
        let card = descriptions.get(state.players.one.hand[0].def);
        assert_ne!(card.name, "Unnamed card");
        assert!(card.life.is_some() || !card.effects.is_empty() || !card.mana_types.is_empty());
    }

    #[test]
    fn spoofed_actor_is_rejected() {
        let action = ActionV1::Resign {
            player: summoners_match_log::wire::PlayerIdV1::Two,
        };
        assert_eq!(
            normalize_action(PlayerId::One, action),
            Err(ProtocolError::Actor)
        );
    }

    #[test]
    fn phase_and_readiness_have_transport_values() {
        assert_eq!(phase_view(Phase::Combat), PhaseView::Combat);
        assert_eq!(readiness_view(Readiness::Ready), ReadinessView::Ready);
    }

    #[test]
    fn conversions_and_private_view_helpers_are_direct() {
        assert_eq!(PlayerId::from(Seat::One), PlayerId::One);
        assert_eq!(PlayerId::from(Seat::Two), PlayerId::Two);
        assert_eq!(Seat::from(PlayerId::One), Seat::One);
        assert_eq!(Seat::from(PlayerId::Two), Seat::Two);
        assert_eq!(position_text(Position::Main), "Main");
        let cost = Cost {
            matter: 1,
            mind: 2,
            spirit: 3,
            generic: 4,
        };
        assert_eq!(cost_text(&cost), "matter 1 mind 2 spirit 3 generic 4");
        let (state, descriptions) = state_and_descriptions();
        let public = public_player(&state.players.one, &descriptions);
        assert_eq!(public.hand_count, state.players.one.hand.len());
        assert_eq!(public.deck_count, state.players.one.deck.len());
        let summon = summon_view(
            state.players.one.main.as_ref().expect("main"),
            &descriptions,
        );
        assert_eq!(summon.damage, 0);
        assert!(!summon.chain.is_empty());
        assert_eq!(
            descriptions
                .get(EntityId::parse("ffffffffffffffffffffffffffffffff").expect("id"))
                .name,
            "Unknown card"
        );
    }

    #[test]
    fn stack_pending_outcome_and_text_helpers_are_direct() {
        let (state, descriptions) = state_and_descriptions();
        let card = state.players.one.hand[0];
        assert!(matches!(
            stack_view(
                &StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main
                },
                &descriptions
            ),
            StackView::Attack { .. }
        ));
        assert!(matches!(
            stack_view(
                &StackItem::Spell {
                    caster: PlayerId::One,
                    card,
                    targets: vec![Position::Main]
                },
                &descriptions
            ),
            StackView::Spell { .. }
        ));
        assert!(matches!(
            stack_view(
                &StackItem::Trigger {
                    controller: PlayerId::Two,
                    source: Position::Main,
                    ability: card.def,
                    event: summoners_core::domain::cards::TriggerEvent::YourUpkeep,
                    targets: vec![Position::Main],
                    effects: vec![EffectLeaf::DrawCards { amount: 2 }]
                },
                &descriptions
            ),
            StackView::Trigger { .. }
        ));
        assert_eq!(
            pending_view(PendingInput::ManaProduction {
                player: PlayerId::One,
                source: summoners_core::domain::state::ManaSource::Player
            })
            .kind,
            PendingKindView::ManaProduction
        );
        assert_eq!(
            pending_view(PendingInput::Promotion {
                player: PlayerId::Two
            })
            .owner,
            Seat::Two
        );
        assert_eq!(
            pending_view(PendingInput::PrizePick {
                chooser: PlayerId::One
            })
            .kind,
            PendingKindView::PrizePick
        );
        for (reason, expected) in [
            (
                summoners_core::domain::state::LossReason::ThirdMainLoss,
                OutcomeReasonView::ThirdMainLoss,
            ),
            (
                summoners_core::domain::state::LossReason::NoPromotionAvailable,
                OutcomeReasonView::NoPromotionAvailable,
            ),
            (
                summoners_core::domain::state::LossReason::EmptyDeckDraw,
                OutcomeReasonView::EmptyDeckDraw,
            ),
            (
                summoners_core::domain::state::LossReason::Resignation,
                OutcomeReasonView::Resignation,
            ),
        ] {
            assert_eq!(
                outcome_view(GameOutcome {
                    winner: PlayerId::One,
                    reason
                })
                .reason,
                expected
            );
        }
        assert_eq!(
            effect_text(&EffectLeaf::DrawCards { amount: 2 }),
            "draw 2 cards"
        );
        assert_eq!(effect_text(&EffectLeaf::MoveSummon), "move a Summon");
        let ability = summoners_core::domain::cards::Entity {
            id: card.def,
            components: vec![summoners_core::domain::cards::Component::Name(Name(
                "Skill".to_string(),
            ))],
        };
        assert_eq!(describe_ability(&ability), "Skill");
        assert_ne!(
            describe_card(state.cards.entities().first().expect("card")).name,
            "Unnamed card"
        );
    }
}
