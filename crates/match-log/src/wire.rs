use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use summoners_core::domain::cards::EntityId;

use crate::error::WireConversionError;
use crate::state::{StateDigestV1, StateProjectionV1};

mod action;
mod action_error;
mod event;
mod records;

pub use action::{ActionV1, ManaTypeV1};
pub use action_error::ErrorV1;
pub use event::{
    BattlefieldTargetV1, DamageConstraintV1, DamageContextV1, DamageOperationV1, DamageOriginV1,
    DamageSourceV1, DamageStageV1, EventV1,
};
pub use records::{
    ActionRecordKindV1, ActionRecordV1, EventRecordKindV1, EventRecordV1, FinalStateRecordKindV1,
    FinalStateV1, MatchCompletedRecordKindV1, MatchCompletedV1, StepCompletedRecordKindV1,
    StepCompletedV1, StepRejectedRecordKindV1, StepRejectedV1,
};

/// Diagnostic header data. Its keys are open and are not normative by default.
pub type HeaderMetadataV1 = BTreeMap<String, serde_json::Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeaderRecordKindV1 {
    Header,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeaderV1 {
    pub sequence: u64,
    pub record: HeaderRecordKindV1,
    pub format: String,
    pub format_version: u32,
    pub metadata: HeaderMetadataV1,
}

impl HeaderV1 {
    #[must_use]
    pub fn new(metadata: HeaderMetadataV1) -> Self {
        Self {
            sequence: 0,
            record: HeaderRecordKindV1::Header,
            format: "summoners_match".to_string(),
            format_version: 1,
            metadata,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchCreatedRecordKindV1 {
    MatchCreated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRequirementV1 {
    pub set: String,
    pub revision: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchCreatedV1 {
    pub sequence: u64,
    pub record: MatchCreatedRecordKindV1,
    pub required_sets: Vec<SetRequirementV1>,
    pub initial_state: StateProjectionV1,
    pub state_digest: StateDigestV1,
}

impl MatchCreatedV1 {
    #[must_use]
    pub fn new(
        required_sets: Vec<SetRequirementV1>,
        initial_state: StateProjectionV1,
        state_digest: StateDigestV1,
    ) -> Self {
        Self {
            sequence: 1,
            record: MatchCreatedRecordKindV1::MatchCreated,
            required_sets,
            initial_state,
            state_digest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RecordV1 {
    Header(HeaderV1),
    MatchCreated(Box<MatchCreatedV1>),
    Action(ActionRecordV1),
    Event(EventRecordV1),
    StepCompleted(StepCompletedV1),
    StepRejected(StepRejectedV1),
    FinalState(Box<FinalStateV1>),
    MatchCompleted(MatchCompletedV1),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct EntityIdV1(pub String);

fn parse_entity_id(value: EntityIdV1) -> Result<EntityId, WireConversionError> {
    EntityId::parse(&value.0).map_err(|_| WireConversionError::InvalidEntityId { value: value.0 })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlayerIdV1 {
    One,
    Two,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchSlotV1 {
    First,
    Second,
    Third,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PositionV1 {
    Main,
    Bench { slot: BenchSlotV1 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CardRefV1 {
    pub instance: u32,
    pub definition: EntityIdV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpgradeChainV1 {
    pub layers: Vec<CardRefV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DurationMarkerV1 {
    CannotBeMovedByOpponent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeActivityV1 {
    Available,
    PlayedThisTurn,
    UpgradedThisTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummonTurnRecordV1 {
    pub upgrade: UpgradeActivityV1,
    pub entered_main: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessV1 {
    Ready,
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummonInstanceV1 {
    pub chain: UpgradeChainV1,
    pub damage: u32,
    pub readiness: ReadinessV1,
    pub owner: PlayerIdV1,
    pub controller: PlayerIdV1,
    pub duration_markers: Vec<DurationMarkerV1>,
    pub turn: SummonTurnRecordV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManaBankV1 {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayerStateV1 {
    pub main: Option<SummonInstanceV1>,
    pub bench: [Option<SummonInstanceV1>; 3],
    pub deck: Vec<CardRefV1>,
    pub hand: Vec<CardRefV1>,
    pub prizes: Vec<CardRefV1>,
    pub discard: Vec<CardRefV1>,
    pub mana: ManaBankV1,
    pub main_losses: u8,
    pub enchantments: Vec<CardRefV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayersV1 {
    pub one: PlayerStateV1,
    pub two: PlayerStateV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PerPlayerBoolV1 {
    pub one: bool,
    pub two: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StackWindowV1 {
    pub holder: PlayerIdV1,
    pub prior_pass: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhaseV1 {
    Upkeep,
    Main,
    Combat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurnStateV1 {
    pub active_player: PlayerIdV1,
    pub phase: PhaseV1,
    pub window: Option<StackWindowV1>,
    pub normal_attack_used: bool,
    pub normal_retreat_used: bool,
    pub spell_played_this_turn: PerPlayerBoolV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManaSourceV1 {
    Player,
    Summon { position: PositionV1 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PendingInputV1 {
    ManaProduction {
        player: PlayerIdV1,
        source: ManaSourceV1,
    },
    Promotion {
        player: PlayerIdV1,
    },
    PrizePick {
        chooser: PlayerIdV1,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LossReasonV1 {
    ThirdMainLoss,
    NoPromotionAvailable,
    EmptyDeckDraw,
    Resignation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GameOutcomeV1 {
    pub winner: PlayerIdV1,
    pub reason: LossReasonV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKindV1 {
    Name,
    AccountingId,
    Life,
    RetreatCost,
    Form,
    Produces,
    Tags,
    Cost,
    Skill,
    Attack,
    Trigger,
    Effect,
    Passive,
    Timing,
    Event,
    Respondable,
    Persistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BreakageV1 {
    pub rule: String,
    pub entity: EntityIdV1,
    pub expected: ComponentKindV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GameStatusV1 {
    Playing,
    Ended { outcome: GameOutcomeV1 },
    Broken { breakage: BreakageV1 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEventV1 {
    YourUpkeep,
    EntersMain,
    EntersBench,
    LeavesMain,
    LeavesBench,
    AnySummonDestroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectConditionV1 {
    DefenderEnteredMainThisTurn,
    SpellPlayedThisTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectTargetV1 {
    Selected,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DamageConstraintsV1 {
    pub unincreasable: bool,
    pub unpreventable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DamageAdditionV1 {
    pub amount: u32,
    pub condition: EffectConditionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DamageEffectV1 {
    pub base: u32,
    pub constraints: DamageConstraintsV1,
    pub additions: Vec<DamageAdditionV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseBlockV1 {
    AttackSpells,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectLeafV1 {
    DealDamage {
        damage: DamageEffectV1,
    },
    Heal {
        amount: u32,
        target: EffectTargetV1,
    },
    MoveSummon,
    SwapPositions,
    BlockResponses {
        condition: EffectConditionV1,
        block: ResponseBlockV1,
    },
    ReturnSpellFromDiscard,
    LookAtPrizes,
    DrawCards {
        amount: u32,
    },
    ReturnSpellToDeckTop,
    ProduceMana {
        target: EffectTargetV1,
    },
    CannotBeMovedByOpponent {
        target: EffectTargetV1,
    },
    ReadySummon,
    SwapOpposingPositions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StackItemV1 {
    Attack {
        attacker: PlayerIdV1,
        target: PositionV1,
    },
    Spell {
        caster: PlayerIdV1,
        card: CardRefV1,
        targets: Vec<PositionV1>,
    },
    Trigger {
        controller: PlayerIdV1,
        source: PositionV1,
        ability: EntityIdV1,
        event: TriggerEventV1,
        targets: Vec<PositionV1>,
        effects: Vec<EffectLeafV1>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MovementStepV1 {
    LeavingMain,
    EnteringBench,
    LeavingBench,
    EnteringMain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkItemV1 {
    DestructionCheck {
        position: PositionV1,
    },
    DiscardDestroyedChain {
        position: PositionV1,
    },
    RecordMainLoss {
        player: PlayerIdV1,
    },
    RecoverPrize {
        player: PlayerIdV1,
    },
    PromoteBenchSummon {
        player: PlayerIdV1,
    },
    ResolveMovementConsequences {
        player: PlayerIdV1,
    },
    MovementTrigger {
        step: MovementStepV1,
        player: PlayerIdV1,
        position: PositionV1,
    },
    FireTrigger {
        player: PlayerIdV1,
        position: PositionV1,
        event: TriggerEventV1,
        ability: EntityIdV1,
    },
    LossCheck {
        player: PlayerIdV1,
    },
    ReadyAll,
    DrawCard,
    ProduceMana {
        player: PlayerIdV1,
        source: ManaSourceV1,
    },
    BeginMainPhase,
}
