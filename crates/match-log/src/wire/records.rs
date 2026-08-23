use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::{StateDigestV1, StateProjectionV1};

use super::{ActionV1, ErrorV1, EventV1, LossReasonV1, PlayerIdV1};

macro_rules! record_kind {
    ($name:ident, $variant:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $variant,
        }
    };
}

record_kind!(ActionRecordKindV1, Action);
record_kind!(EventRecordKindV1, Event);
record_kind!(StepCompletedRecordKindV1, StepCompleted);
record_kind!(StepRejectedRecordKindV1, StepRejected);
record_kind!(FinalStateRecordKindV1, FinalState);
record_kind!(MatchCompletedRecordKindV1, MatchCompleted);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionRecordV1 {
    pub sequence: u64,
    pub record: ActionRecordKindV1,
    pub step: u64,
    pub action: ActionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventRecordV1 {
    pub sequence: u64,
    pub record: EventRecordKindV1,
    pub step: u64,
    pub index: u64,
    pub event: EventV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepCompletedV1 {
    pub sequence: u64,
    pub record: StepCompletedRecordKindV1,
    pub step: u64,
    pub event_count: u64,
    pub state_digest: StateDigestV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepRejectedV1 {
    pub sequence: u64,
    pub record: StepRejectedRecordKindV1,
    pub step: u64,
    pub error: ErrorV1,
    pub state_digest: StateDigestV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FinalStateV1 {
    pub sequence: u64,
    pub record: FinalStateRecordKindV1,
    pub final_state: StateProjectionV1,
    pub state_digest: StateDigestV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchCompletedV1 {
    pub sequence: u64,
    pub record: MatchCompletedRecordKindV1,
    pub step_count: u64,
    pub event_count: u64,
    pub state_digest: StateDigestV1,
    pub winner: PlayerIdV1,
    pub reason: LossReasonV1,
}
