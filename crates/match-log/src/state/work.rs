use super::{StateRebuildError, WorkItem, WorkItemV1};

impl TryFrom<WorkItemV1> for WorkItem {
    type Error = StateRebuildError;

    fn try_from(value: WorkItemV1) -> Result<Self, Self::Error> {
        match value {
            WorkItemV1::DestructionCheck { position } => {
                Ok(Self::DestructionCheck(position.into()))
            }
            WorkItemV1::DiscardDestroyedChain { position } => {
                Ok(Self::DiscardDestroyedChain(position.into()))
            }
            WorkItemV1::RecordMainLoss { player } => Ok(Self::RecordMainLoss(player.into())),
            WorkItemV1::RecoverPrize { player } => Ok(Self::RecoverPrize(player.into())),
            WorkItemV1::PromoteBenchSummon { player } => {
                Ok(Self::PromoteBenchSummon(player.into()))
            }
            WorkItemV1::ResolveMovementConsequences { player } => {
                Ok(Self::ResolveMovementConsequences(player.into()))
            }
            WorkItemV1::MovementTrigger {
                step,
                player,
                position,
            } => Ok(Self::MovementTrigger(
                step.into(),
                player.into(),
                position.into(),
            )),
            WorkItemV1::FireTrigger {
                player,
                position,
                event,
                ability,
            } => Ok(Self::FireTrigger(
                player.into(),
                position.into(),
                event.into(),
                ability.try_into()?,
            )),
            WorkItemV1::LossCheck { player } => Ok(Self::LossCheck(player.into())),
            WorkItemV1::ReadyAll => Ok(Self::ReadyAll),
            WorkItemV1::DrawCard => Ok(Self::DrawCard),
            WorkItemV1::ProduceMana { player, source } => Ok(Self::ProduceMana {
                player: player.into(),
                source: source.into(),
            }),
            WorkItemV1::BeginMainPhase => Ok(Self::BeginMainPhase),
        }
    }
}
