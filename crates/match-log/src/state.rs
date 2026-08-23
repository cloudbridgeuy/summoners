use std::{collections::VecDeque, sync::Arc};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use summoners_core::domain::{
    cards::{
        Breakage, CardSet, ComponentKind, DamageAddition, DamageConstraint, DamageConstraints,
        DamageEffect, EffectCondition, EffectLeaf, EffectTarget, EntityId, ResponseBlock,
        TriggerEvent,
    },
    ids::{BenchSlot, CardInstanceId, PlayerId, Position},
    state::{
        CardRef, Coin, DurationMarker, EnteredMain, GameOutcome, GameState, GameStatus, LossReason,
        ManaBank, ManaSource, MovementStep, PendingInput, PerPlayer, Phase, PlayerState, Readiness,
        StackItem, StackWindow, SummonInstance, SummonTurnRecord, TurnState, UpgradeActivity,
        UpgradeChain, WorkItem,
    },
};

use crate::{
    error::{CanonicalStateError, StateRebuildError},
    wire::{
        BenchSlotV1, BreakageV1, CardRefV1, ComponentKindV1, DamageAdditionV1, DamageConstraintsV1,
        DamageEffectV1, DurationMarkerV1, EffectConditionV1, EffectLeafV1, EffectTargetV1,
        EntityIdV1, GameOutcomeV1, GameStatusV1, LossReasonV1, ManaBankV1, ManaSourceV1,
        MovementStepV1, PendingInputV1, PerPlayerBoolV1, PhaseV1, PlayerIdV1, PlayerStateV1,
        PlayersV1, PositionV1, ReadinessV1, ResponseBlockV1, StackItemV1, StackWindowV1,
        SummonInstanceV1, SummonTurnRecordV1, TriggerEventV1, TurnStateV1, UpgradeActivityV1,
        UpgradeChainV1, WorkItemV1,
    },
};

pub const STATE_PROJECTION_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StateProjectionV1 {
    pub players: PlayersV1,
    pub coin: bool,
    pub turn: TurnStateV1,
    pub stack: Vec<StackItemV1>,
    pub stack_segment_bases: Vec<u64>,
    pub work: Vec<WorkItemV1>,
    pub pending: Option<PendingInputV1>,
    pub status: GameStatusV1,
}

impl StateProjectionV1 {
    #[must_use]
    pub fn from_state(state: &GameState) -> Self {
        Self {
            players: PlayersV1 {
                one: PlayerStateV1::from(&state.players.one),
                two: PlayerStateV1::from(&state.players.two),
            },
            coin: state.coin.is_some(),
            turn: TurnStateV1::from(state.turn),
            stack: state.stack.iter().map(StackItemV1::from).collect(),
            stack_segment_bases: state
                .stack_segment_bases
                .iter()
                .map(|base| *base as u64)
                .collect(),
            work: state.work.iter().map(WorkItemV1::from).collect(),
            pending: state.pending.map(PendingInputV1::from),
            status: GameStatusV1::from(state.status),
        }
    }

    pub fn into_game_state(self, cards: Arc<CardSet>) -> Result<GameState, StateRebuildError> {
        Ok(GameState {
            players: PerPlayer::new(self.players.one.try_into()?, self.players.two.try_into()?),
            coin: self.coin.then_some(Coin),
            turn: self.turn.into(),
            stack: self
                .stack
                .into_iter()
                .map(StackItem::try_from)
                .collect::<Result<_, _>>()?,
            stack_segment_bases: self
                .stack_segment_bases
                .into_iter()
                .map(|base| {
                    usize::try_from(base)
                        .map_err(|_| StateRebuildError::StackSegmentBaseOutOfRange { value: base })
                })
                .collect::<Result<_, _>>()?,
            work: self
                .work
                .into_iter()
                .map(WorkItem::try_from)
                .collect::<Result<VecDeque<_>, _>>()?,
            pending: self.pending.map(PendingInput::from),
            status: self.status.try_into()?,
            cards,
        })
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalStateError> {
        #[derive(Serialize)]
        struct CanonicalState<'a> {
            projection_version: u32,
            state: &'a StateProjectionV1,
        }

        serde_json::to_vec(&CanonicalState {
            projection_version: STATE_PROJECTION_VERSION,
            state: self,
        })
        .map_err(CanonicalStateError::new)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct StateDigestV1(pub String);

impl StateDigestV1 {
    pub fn compute(state: &StateProjectionV1) -> Result<Self, CanonicalStateError> {
        let digest = Sha256::digest(state.canonical_bytes()?);
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self(format!("sha256:{hex}")))
    }
}

impl From<EntityId> for EntityIdV1 {
    fn from(value: EntityId) -> Self {
        Self(value.to_string())
    }
}

impl TryFrom<EntityIdV1> for EntityId {
    type Error = StateRebuildError;

    fn try_from(value: EntityIdV1) -> Result<Self, Self::Error> {
        EntityId::parse(&value.0).map_err(|_| StateRebuildError::InvalidEntityId { value: value.0 })
    }
}

impl From<PlayerId> for PlayerIdV1 {
    fn from(value: PlayerId) -> Self {
        match value {
            PlayerId::One => Self::One,
            PlayerId::Two => Self::Two,
        }
    }
}

impl From<PlayerIdV1> for PlayerId {
    fn from(value: PlayerIdV1) -> Self {
        match value {
            PlayerIdV1::One => Self::One,
            PlayerIdV1::Two => Self::Two,
        }
    }
}

impl From<BenchSlot> for BenchSlotV1 {
    fn from(value: BenchSlot) -> Self {
        match value {
            BenchSlot::First => Self::First,
            BenchSlot::Second => Self::Second,
            BenchSlot::Third => Self::Third,
        }
    }
}

impl From<BenchSlotV1> for BenchSlot {
    fn from(value: BenchSlotV1) -> Self {
        match value {
            BenchSlotV1::First => Self::First,
            BenchSlotV1::Second => Self::Second,
            BenchSlotV1::Third => Self::Third,
        }
    }
}

impl From<Position> for PositionV1 {
    fn from(value: Position) -> Self {
        match value {
            Position::Main => Self::Main,
            Position::Bench(slot) => Self::Bench { slot: slot.into() },
        }
    }
}

impl From<PositionV1> for Position {
    fn from(value: PositionV1) -> Self {
        match value {
            PositionV1::Main => Self::Main,
            PositionV1::Bench { slot } => Self::Bench(slot.into()),
        }
    }
}

impl From<CardRef> for CardRefV1 {
    fn from(value: CardRef) -> Self {
        Self {
            instance: value.instance.0,
            definition: value.def.into(),
        }
    }
}

impl TryFrom<CardRefV1> for CardRef {
    type Error = StateRebuildError;

    fn try_from(value: CardRefV1) -> Result<Self, Self::Error> {
        Ok(Self {
            instance: CardInstanceId(value.instance),
            def: value.definition.try_into()?,
        })
    }
}

impl From<&UpgradeChain> for UpgradeChainV1 {
    fn from(value: &UpgradeChain) -> Self {
        Self {
            layers: value.layers().copied().map(CardRefV1::from).collect(),
        }
    }
}

impl TryFrom<UpgradeChainV1> for UpgradeChain {
    type Error = StateRebuildError;

    fn try_from(value: UpgradeChainV1) -> Result<Self, Self::Error> {
        let mut layers = value.layers.into_iter().map(CardRef::try_from);
        let base = layers
            .next()
            .transpose()?
            .ok_or(StateRebuildError::EmptyUpgradeChain)?;
        Ok(Self::new(base, layers.collect::<Result<_, _>>()?))
    }
}

impl From<DurationMarker> for DurationMarkerV1 {
    fn from(value: DurationMarker) -> Self {
        match value {
            DurationMarker::CannotBeMovedByOpponent => Self::CannotBeMovedByOpponent,
        }
    }
}

impl From<DurationMarkerV1> for DurationMarker {
    fn from(value: DurationMarkerV1) -> Self {
        match value {
            DurationMarkerV1::CannotBeMovedByOpponent => Self::CannotBeMovedByOpponent,
        }
    }
}

impl From<UpgradeActivity> for UpgradeActivityV1 {
    fn from(value: UpgradeActivity) -> Self {
        match value {
            UpgradeActivity::Available => Self::Available,
            UpgradeActivity::PlayedThisTurn => Self::PlayedThisTurn,
            UpgradeActivity::UpgradedThisTurn => Self::UpgradedThisTurn,
        }
    }
}

impl From<UpgradeActivityV1> for UpgradeActivity {
    fn from(value: UpgradeActivityV1) -> Self {
        match value {
            UpgradeActivityV1::Available => Self::Available,
            UpgradeActivityV1::PlayedThisTurn => Self::PlayedThisTurn,
            UpgradeActivityV1::UpgradedThisTurn => Self::UpgradedThisTurn,
        }
    }
}

impl From<Readiness> for ReadinessV1 {
    fn from(value: Readiness) -> Self {
        match value {
            Readiness::Ready => Self::Ready,
            Readiness::Exhausted => Self::Exhausted,
        }
    }
}

impl From<ReadinessV1> for Readiness {
    fn from(value: ReadinessV1) -> Self {
        match value {
            ReadinessV1::Ready => Self::Ready,
            ReadinessV1::Exhausted => Self::Exhausted,
        }
    }
}

impl From<&SummonInstance> for SummonInstanceV1 {
    fn from(value: &SummonInstance) -> Self {
        Self {
            chain: (&value.chain).into(),
            damage: value.damage,
            readiness: value.readiness.into(),
            owner: value.owner.into(),
            controller: value.controller.into(),
            duration_markers: value
                .duration_markers
                .iter()
                .copied()
                .map(DurationMarkerV1::from)
                .collect(),
            turn: SummonTurnRecordV1 {
                upgrade: value.turn.upgrade.into(),
                entered_main: value.turn.main_entry.is_some(),
            },
        }
    }
}

impl TryFrom<SummonInstanceV1> for SummonInstance {
    type Error = StateRebuildError;

    fn try_from(value: SummonInstanceV1) -> Result<Self, Self::Error> {
        Ok(Self {
            chain: value.chain.try_into()?,
            damage: value.damage,
            readiness: value.readiness.into(),
            owner: value.owner.into(),
            controller: value.controller.into(),
            duration_markers: value
                .duration_markers
                .into_iter()
                .map(DurationMarker::from)
                .collect(),
            turn: SummonTurnRecord {
                upgrade: value.turn.upgrade.into(),
                main_entry: value.turn.entered_main.then_some(EnteredMain),
            },
        })
    }
}

impl From<ManaBank> for ManaBankV1 {
    fn from(value: ManaBank) -> Self {
        Self {
            matter: value.matter,
            mind: value.mind,
            spirit: value.spirit,
        }
    }
}

impl From<ManaBankV1> for ManaBank {
    fn from(value: ManaBankV1) -> Self {
        Self {
            matter: value.matter,
            mind: value.mind,
            spirit: value.spirit,
        }
    }
}

impl From<&PlayerState> for PlayerStateV1 {
    fn from(value: &PlayerState) -> Self {
        Self {
            main: value.main.as_ref().map(SummonInstanceV1::from),
            bench: value
                .bench
                .each_ref()
                .map(|summon| summon.as_ref().map(SummonInstanceV1::from)),
            deck: value.deck.iter().copied().map(CardRefV1::from).collect(),
            hand: value.hand.iter().copied().map(CardRefV1::from).collect(),
            prizes: value.prizes.iter().copied().map(CardRefV1::from).collect(),
            discard: value.discard.iter().copied().map(CardRefV1::from).collect(),
            mana: value.mana.into(),
            main_losses: value.main_losses,
            enchantments: value
                .enchantments
                .iter()
                .copied()
                .map(CardRefV1::from)
                .collect(),
        }
    }
}

impl TryFrom<PlayerStateV1> for PlayerState {
    type Error = StateRebuildError;

    fn try_from(value: PlayerStateV1) -> Result<Self, Self::Error> {
        Ok(Self {
            main: value.main.map(SummonInstance::try_from).transpose()?,
            bench: value
                .bench
                .map(|summon| summon.map(SummonInstance::try_from).transpose())
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| StateRebuildError::EmptyUpgradeChain)?,
            deck: card_refs(value.deck)?,
            hand: card_refs(value.hand)?,
            prizes: card_refs(value.prizes)?,
            discard: card_refs(value.discard)?,
            mana: value.mana.into(),
            main_losses: value.main_losses,
            enchantments: card_refs(value.enchantments)?,
        })
    }
}

fn card_refs(values: Vec<CardRefV1>) -> Result<Vec<CardRef>, StateRebuildError> {
    values.into_iter().map(CardRef::try_from).collect()
}

impl From<StackWindow> for StackWindowV1 {
    fn from(value: StackWindow) -> Self {
        Self {
            holder: value.holder.into(),
            prior_pass: value.prior_pass,
        }
    }
}

impl From<StackWindowV1> for StackWindow {
    fn from(value: StackWindowV1) -> Self {
        Self {
            holder: value.holder.into(),
            prior_pass: value.prior_pass,
        }
    }
}

impl From<Phase> for PhaseV1 {
    fn from(value: Phase) -> Self {
        match value {
            Phase::Upkeep => Self::Upkeep,
            Phase::Main => Self::Main,
            Phase::Combat => Self::Combat,
        }
    }
}

impl From<PhaseV1> for Phase {
    fn from(value: PhaseV1) -> Self {
        match value {
            PhaseV1::Upkeep => Self::Upkeep,
            PhaseV1::Main => Self::Main,
            PhaseV1::Combat => Self::Combat,
        }
    }
}

impl From<TurnState> for TurnStateV1 {
    fn from(value: TurnState) -> Self {
        Self {
            active_player: value.active_player.into(),
            phase: value.phase.into(),
            window: value.window.map(StackWindowV1::from),
            normal_attack_used: value.normal_attack_used,
            normal_retreat_used: value.normal_retreat_used,
            spell_played_this_turn: PerPlayerBoolV1 {
                one: value.spell_played_this_turn.one,
                two: value.spell_played_this_turn.two,
            },
        }
    }
}

impl From<TurnStateV1> for TurnState {
    fn from(value: TurnStateV1) -> Self {
        Self {
            active_player: value.active_player.into(),
            phase: value.phase.into(),
            window: value.window.map(StackWindow::from),
            normal_attack_used: value.normal_attack_used,
            normal_retreat_used: value.normal_retreat_used,
            spell_played_this_turn: PerPlayer::new(
                value.spell_played_this_turn.one,
                value.spell_played_this_turn.two,
            ),
        }
    }
}

impl From<ManaSource> for ManaSourceV1 {
    fn from(value: ManaSource) -> Self {
        match value {
            ManaSource::Player => Self::Player,
            ManaSource::Summon(position) => Self::Summon {
                position: position.into(),
            },
        }
    }
}

impl From<ManaSourceV1> for ManaSource {
    fn from(value: ManaSourceV1) -> Self {
        match value {
            ManaSourceV1::Player => Self::Player,
            ManaSourceV1::Summon { position } => Self::Summon(position.into()),
        }
    }
}

impl From<PendingInput> for PendingInputV1 {
    fn from(value: PendingInput) -> Self {
        match value {
            PendingInput::ManaProduction { player, source } => Self::ManaProduction {
                player: player.into(),
                source: source.into(),
            },
            PendingInput::Promotion { player } => Self::Promotion {
                player: player.into(),
            },
            PendingInput::PrizePick { chooser } => Self::PrizePick {
                chooser: chooser.into(),
            },
        }
    }
}

impl From<PendingInputV1> for PendingInput {
    fn from(value: PendingInputV1) -> Self {
        match value {
            PendingInputV1::ManaProduction { player, source } => Self::ManaProduction {
                player: player.into(),
                source: source.into(),
            },
            PendingInputV1::Promotion { player } => Self::Promotion {
                player: player.into(),
            },
            PendingInputV1::PrizePick { chooser } => Self::PrizePick {
                chooser: chooser.into(),
            },
        }
    }
}

impl From<LossReason> for LossReasonV1 {
    fn from(value: LossReason) -> Self {
        match value {
            LossReason::ThirdMainLoss => Self::ThirdMainLoss,
            LossReason::NoPromotionAvailable => Self::NoPromotionAvailable,
            LossReason::EmptyDeckDraw => Self::EmptyDeckDraw,
            LossReason::Resignation => Self::Resignation,
        }
    }
}

impl From<LossReasonV1> for LossReason {
    fn from(value: LossReasonV1) -> Self {
        match value {
            LossReasonV1::ThirdMainLoss => Self::ThirdMainLoss,
            LossReasonV1::NoPromotionAvailable => Self::NoPromotionAvailable,
            LossReasonV1::EmptyDeckDraw => Self::EmptyDeckDraw,
            LossReasonV1::Resignation => Self::Resignation,
        }
    }
}

impl From<GameOutcome> for GameOutcomeV1 {
    fn from(value: GameOutcome) -> Self {
        Self {
            winner: value.winner.into(),
            reason: value.reason.into(),
        }
    }
}

impl From<GameOutcomeV1> for GameOutcome {
    fn from(value: GameOutcomeV1) -> Self {
        Self {
            winner: value.winner.into(),
            reason: value.reason.into(),
        }
    }
}

impl From<ComponentKind> for ComponentKindV1 {
    fn from(value: ComponentKind) -> Self {
        match value {
            ComponentKind::Name => Self::Name,
            ComponentKind::AccountingId => Self::AccountingId,
            ComponentKind::Life => Self::Life,
            ComponentKind::RetreatCost => Self::RetreatCost,
            ComponentKind::Form => Self::Form,
            ComponentKind::Produces => Self::Produces,
            ComponentKind::Tags => Self::Tags,
            ComponentKind::Cost => Self::Cost,
            ComponentKind::Skill => Self::Skill,
            ComponentKind::Attack => Self::Attack,
            ComponentKind::Trigger => Self::Trigger,
            ComponentKind::Effect => Self::Effect,
            ComponentKind::Passive => Self::Passive,
            ComponentKind::Timing => Self::Timing,
            ComponentKind::Event => Self::Event,
            ComponentKind::Respondable => Self::Respondable,
            ComponentKind::Persistent => Self::Persistent,
        }
    }
}

impl From<ComponentKindV1> for ComponentKind {
    fn from(value: ComponentKindV1) -> Self {
        match value {
            ComponentKindV1::Name => Self::Name,
            ComponentKindV1::AccountingId => Self::AccountingId,
            ComponentKindV1::Life => Self::Life,
            ComponentKindV1::RetreatCost => Self::RetreatCost,
            ComponentKindV1::Form => Self::Form,
            ComponentKindV1::Produces => Self::Produces,
            ComponentKindV1::Tags => Self::Tags,
            ComponentKindV1::Cost => Self::Cost,
            ComponentKindV1::Skill => Self::Skill,
            ComponentKindV1::Attack => Self::Attack,
            ComponentKindV1::Trigger => Self::Trigger,
            ComponentKindV1::Effect => Self::Effect,
            ComponentKindV1::Passive => Self::Passive,
            ComponentKindV1::Timing => Self::Timing,
            ComponentKindV1::Event => Self::Event,
            ComponentKindV1::Respondable => Self::Respondable,
            ComponentKindV1::Persistent => Self::Persistent,
        }
    }
}

impl From<GameStatus> for GameStatusV1 {
    fn from(value: GameStatus) -> Self {
        match value {
            GameStatus::Playing => Self::Playing,
            GameStatus::Ended(outcome) => Self::Ended {
                outcome: outcome.into(),
            },
            GameStatus::Broken(breakage) => Self::Broken {
                breakage: BreakageV1 {
                    rule: breakage.rule.to_string(),
                    entity: breakage.entity.into(),
                    expected: breakage.expected.into(),
                },
            },
        }
    }
}

impl TryFrom<GameStatusV1> for GameStatus {
    type Error = StateRebuildError;

    fn try_from(value: GameStatusV1) -> Result<Self, Self::Error> {
        match value {
            GameStatusV1::Playing => Ok(Self::Playing),
            GameStatusV1::Ended { outcome } => Ok(Self::Ended(outcome.into())),
            GameStatusV1::Broken { breakage } => {
                let rule = match breakage.rule.as_str() {
                    "destruction" => "destruction",
                    "retreat" => "retreat",
                    _ => {
                        return Err(StateRebuildError::UnsupportedBreakageRule {
                            rule: breakage.rule,
                        });
                    }
                };
                Ok(Self::Broken(Breakage {
                    rule,
                    entity: breakage.entity.try_into()?,
                    expected: breakage.expected.into(),
                }))
            }
        }
    }
}

impl From<TriggerEvent> for TriggerEventV1 {
    fn from(value: TriggerEvent) -> Self {
        match value {
            TriggerEvent::YourUpkeep => Self::YourUpkeep,
            TriggerEvent::EntersMain => Self::EntersMain,
            TriggerEvent::EntersBench => Self::EntersBench,
            TriggerEvent::LeavesMain => Self::LeavesMain,
            TriggerEvent::LeavesBench => Self::LeavesBench,
            TriggerEvent::AnySummonDestroyed => Self::AnySummonDestroyed,
        }
    }
}

impl From<TriggerEventV1> for TriggerEvent {
    fn from(value: TriggerEventV1) -> Self {
        match value {
            TriggerEventV1::YourUpkeep => Self::YourUpkeep,
            TriggerEventV1::EntersMain => Self::EntersMain,
            TriggerEventV1::EntersBench => Self::EntersBench,
            TriggerEventV1::LeavesMain => Self::LeavesMain,
            TriggerEventV1::LeavesBench => Self::LeavesBench,
            TriggerEventV1::AnySummonDestroyed => Self::AnySummonDestroyed,
        }
    }
}

impl From<EffectCondition> for EffectConditionV1 {
    fn from(value: EffectCondition) -> Self {
        match value {
            EffectCondition::DefenderEnteredMainThisTurn => Self::DefenderEnteredMainThisTurn,
            EffectCondition::SpellPlayedThisTurn => Self::SpellPlayedThisTurn,
        }
    }
}

impl From<EffectConditionV1> for EffectCondition {
    fn from(value: EffectConditionV1) -> Self {
        match value {
            EffectConditionV1::DefenderEnteredMainThisTurn => Self::DefenderEnteredMainThisTurn,
            EffectConditionV1::SpellPlayedThisTurn => Self::SpellPlayedThisTurn,
        }
    }
}

impl From<EffectTarget> for EffectTargetV1 {
    fn from(value: EffectTarget) -> Self {
        match value {
            EffectTarget::Selected => Self::Selected,
            EffectTarget::Source => Self::Source,
        }
    }
}

impl From<EffectTargetV1> for EffectTarget {
    fn from(value: EffectTargetV1) -> Self {
        match value {
            EffectTargetV1::Selected => Self::Selected,
            EffectTargetV1::Source => Self::Source,
        }
    }
}

impl From<ResponseBlock> for ResponseBlockV1 {
    fn from(value: ResponseBlock) -> Self {
        match value {
            ResponseBlock::AttackSpells => Self::AttackSpells,
        }
    }
}

impl From<ResponseBlockV1> for ResponseBlock {
    fn from(value: ResponseBlockV1) -> Self {
        match value {
            ResponseBlockV1::AttackSpells => Self::AttackSpells,
        }
    }
}

impl From<&DamageEffect> for DamageEffectV1 {
    fn from(value: &DamageEffect) -> Self {
        Self {
            base: value.base,
            constraints: DamageConstraintsV1 {
                unincreasable: value.constraints.contains(DamageConstraint::Unincreasable),
                unpreventable: value.constraints.contains(DamageConstraint::Unpreventable),
            },
            additions: value
                .additions
                .iter()
                .map(|addition| DamageAdditionV1 {
                    amount: addition.amount,
                    condition: addition.condition.into(),
                })
                .collect(),
        }
    }
}

impl From<DamageEffectV1> for DamageEffect {
    fn from(value: DamageEffectV1) -> Self {
        let mut constraints = DamageConstraints::new();
        if value.constraints.unincreasable {
            constraints.insert(DamageConstraint::Unincreasable);
        }
        if value.constraints.unpreventable {
            constraints.insert(DamageConstraint::Unpreventable);
        }
        Self {
            base: value.base,
            constraints,
            additions: value
                .additions
                .into_iter()
                .map(|addition| DamageAddition {
                    amount: addition.amount,
                    condition: addition.condition.into(),
                })
                .collect(),
        }
    }
}

impl From<&EffectLeaf> for EffectLeafV1 {
    fn from(value: &EffectLeaf) -> Self {
        match value {
            EffectLeaf::DealDamage(damage) => Self::DealDamage {
                damage: damage.into(),
            },
            EffectLeaf::Heal { amount, target } => Self::Heal {
                amount: *amount,
                target: (*target).into(),
            },
            EffectLeaf::MoveSummon => Self::MoveSummon,
            EffectLeaf::SwapPositions => Self::SwapPositions,
            EffectLeaf::BlockResponses { condition, block } => Self::BlockResponses {
                condition: (*condition).into(),
                block: (*block).into(),
            },
            EffectLeaf::ReturnSpellFromDiscard => Self::ReturnSpellFromDiscard,
            EffectLeaf::LookAtPrizes => Self::LookAtPrizes,
            EffectLeaf::DrawCards { amount } => Self::DrawCards { amount: *amount },
            EffectLeaf::ReturnSpellToDeckTop => Self::ReturnSpellToDeckTop,
            EffectLeaf::ProduceMana { target } => Self::ProduceMana {
                target: (*target).into(),
            },
            EffectLeaf::CannotBeMovedByOpponent { target } => Self::CannotBeMovedByOpponent {
                target: (*target).into(),
            },
            EffectLeaf::ReadySummon => Self::ReadySummon,
            EffectLeaf::SwapOpposingPositions => Self::SwapOpposingPositions,
        }
    }
}

impl From<EffectLeafV1> for EffectLeaf {
    fn from(value: EffectLeafV1) -> Self {
        match value {
            EffectLeafV1::DealDamage { damage } => Self::DealDamage(damage.into()),
            EffectLeafV1::Heal { amount, target } => Self::Heal {
                amount,
                target: target.into(),
            },
            EffectLeafV1::MoveSummon => Self::MoveSummon,
            EffectLeafV1::SwapPositions => Self::SwapPositions,
            EffectLeafV1::BlockResponses { condition, block } => Self::BlockResponses {
                condition: condition.into(),
                block: block.into(),
            },
            EffectLeafV1::ReturnSpellFromDiscard => Self::ReturnSpellFromDiscard,
            EffectLeafV1::LookAtPrizes => Self::LookAtPrizes,
            EffectLeafV1::DrawCards { amount } => Self::DrawCards { amount },
            EffectLeafV1::ReturnSpellToDeckTop => Self::ReturnSpellToDeckTop,
            EffectLeafV1::ProduceMana { target } => Self::ProduceMana {
                target: target.into(),
            },
            EffectLeafV1::CannotBeMovedByOpponent { target } => Self::CannotBeMovedByOpponent {
                target: target.into(),
            },
            EffectLeafV1::ReadySummon => Self::ReadySummon,
            EffectLeafV1::SwapOpposingPositions => Self::SwapOpposingPositions,
        }
    }
}

impl From<&StackItem> for StackItemV1 {
    fn from(value: &StackItem) -> Self {
        match value {
            StackItem::Attack { attacker, target } => Self::Attack {
                attacker: (*attacker).into(),
                target: (*target).into(),
            },
            StackItem::Spell {
                caster,
                card,
                targets,
            } => Self::Spell {
                caster: (*caster).into(),
                card: (*card).into(),
                targets: targets.iter().copied().map(PositionV1::from).collect(),
            },
            StackItem::Trigger {
                controller,
                source,
                ability,
                event,
                targets,
                effects,
            } => Self::Trigger {
                controller: (*controller).into(),
                source: (*source).into(),
                ability: (*ability).into(),
                event: (*event).into(),
                targets: targets.iter().copied().map(PositionV1::from).collect(),
                effects: effects.iter().map(EffectLeafV1::from).collect(),
            },
        }
    }
}

impl TryFrom<StackItemV1> for StackItem {
    type Error = StateRebuildError;

    fn try_from(value: StackItemV1) -> Result<Self, Self::Error> {
        match value {
            StackItemV1::Attack { attacker, target } => Ok(Self::Attack {
                attacker: attacker.into(),
                target: target.into(),
            }),
            StackItemV1::Spell {
                caster,
                card,
                targets,
            } => Ok(Self::Spell {
                caster: caster.into(),
                card: card.try_into()?,
                targets: targets.into_iter().map(Position::from).collect(),
            }),
            StackItemV1::Trigger {
                controller,
                source,
                ability,
                event,
                targets,
                effects,
            } => Ok(Self::Trigger {
                controller: controller.into(),
                source: source.into(),
                ability: ability.try_into()?,
                event: event.into(),
                targets: targets.into_iter().map(Position::from).collect(),
                effects: effects.into_iter().map(EffectLeaf::from).collect(),
            }),
        }
    }
}

impl From<MovementStep> for MovementStepV1 {
    fn from(value: MovementStep) -> Self {
        match value {
            MovementStep::LeavingMain => Self::LeavingMain,
            MovementStep::EnteringBench => Self::EnteringBench,
            MovementStep::LeavingBench => Self::LeavingBench,
            MovementStep::EnteringMain => Self::EnteringMain,
        }
    }
}

impl From<MovementStepV1> for MovementStep {
    fn from(value: MovementStepV1) -> Self {
        match value {
            MovementStepV1::LeavingMain => Self::LeavingMain,
            MovementStepV1::EnteringBench => Self::EnteringBench,
            MovementStepV1::LeavingBench => Self::LeavingBench,
            MovementStepV1::EnteringMain => Self::EnteringMain,
        }
    }
}

impl From<&WorkItem> for WorkItemV1 {
    fn from(value: &WorkItem) -> Self {
        match value {
            WorkItem::DestructionCheck(position) => Self::DestructionCheck {
                position: (*position).into(),
            },
            WorkItem::DiscardDestroyedChain(position) => Self::DiscardDestroyedChain {
                position: (*position).into(),
            },
            WorkItem::RecordMainLoss(player) => Self::RecordMainLoss {
                player: (*player).into(),
            },
            WorkItem::RecoverPrize(player) => Self::RecoverPrize {
                player: (*player).into(),
            },
            WorkItem::PromoteBenchSummon(player) => Self::PromoteBenchSummon {
                player: (*player).into(),
            },
            WorkItem::ResolveMovementConsequences(player) => Self::ResolveMovementConsequences {
                player: (*player).into(),
            },
            WorkItem::MovementTrigger(step, player, position) => Self::MovementTrigger {
                step: (*step).into(),
                player: (*player).into(),
                position: (*position).into(),
            },
            WorkItem::FireTrigger(player, position, event, ability) => Self::FireTrigger {
                player: (*player).into(),
                position: (*position).into(),
                event: (*event).into(),
                ability: (*ability).into(),
            },
            WorkItem::LossCheck(player) => Self::LossCheck {
                player: (*player).into(),
            },
            WorkItem::ReadyAll => Self::ReadyAll,
            WorkItem::DrawCard => Self::DrawCard,
            WorkItem::ProduceMana { player, source } => Self::ProduceMana {
                player: (*player).into(),
                source: (*source).into(),
            },
            WorkItem::BeginMainPhase => Self::BeginMainPhase,
        }
    }
}

mod work;

#[cfg(test)]
mod tests;
