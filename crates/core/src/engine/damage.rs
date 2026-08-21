//! Pure Damage evaluation and the separate state commit.

use crate::domain::cards::{DamageConstraint, DamageEffect, EffectCondition, Modifier};
use crate::domain::events::{
    DamageContext, DamageOperation, DamageOrigin, DamageSource, DamageStage, GameEvent,
};
use crate::domain::ids::{PlayerId, Position};
use crate::domain::state::{GameState, PlayerState, SummonInstance, WorkItem};

/// One grouped printed Damage effect with its resolved source and target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DamageIntent {
    pub context: DamageContext,
    pub effect: DamageEffect,
}

/// One operation ready for the fixed-stage evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DamageAdjustment {
    stage: DamageStage,
    operation: DamageOperation,
    origin: DamageOrigin,
}

/// One evaluated line, before it is flattened into public events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DamageLine {
    Applied {
        stage: DamageStage,
        operation: DamageOperation,
        origin: DamageOrigin,
        input: u32,
        output: u32,
    },
    Skipped {
        stage: DamageStage,
        operation: DamageOperation,
        origin: DamageOrigin,
        input: u32,
        constraint: DamageConstraint,
    },
}

/// The complete immutable result the interpreter commits once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DamageResolution {
    context: DamageContext,
    base: u32,
    constraints: crate::domain::cards::DamageConstraints,
    amount: u32,
    lines: Vec<DamageLine>,
}

/// Evaluate additions, persistent reductions, and the zero clamp without
/// mutating state.
pub(crate) fn evaluate(state: &GameState, intent: &DamageIntent) -> DamageResolution {
    let origin = intent.context.source.origin();
    let mut adjustments = collect_adjustments(state, intent);
    adjustments.push(DamageAdjustment {
        stage: DamageStage::Clamp,
        operation: DamageOperation::ClampToZero,
        origin,
    });

    let mut amount = intent.effect.base;
    let mut lines = Vec::with_capacity(adjustments.len());
    for adjustment in adjustments {
        let blocked = match adjustment.stage {
            DamageStage::Addition
                if intent
                    .effect
                    .constraints
                    .contains(DamageConstraint::Unincreasable) =>
            {
                Some(DamageConstraint::Unincreasable)
            }
            DamageStage::PersistentReduction
                if intent
                    .effect
                    .constraints
                    .contains(DamageConstraint::Unpreventable) =>
            {
                Some(DamageConstraint::Unpreventable)
            }
            DamageStage::Addition
            | DamageStage::PersistentReduction
            | DamageStage::Clamp
            | DamageStage::Commit => None,
        };

        if let Some(constraint) = blocked {
            lines.push(DamageLine::Skipped {
                stage: adjustment.stage,
                operation: adjustment.operation,
                origin: adjustment.origin,
                input: amount,
                constraint,
            });
            continue;
        }

        let input = amount;
        amount = match adjustment.operation {
            DamageOperation::Add(value) => amount.saturating_add(value),
            DamageOperation::Reduce(value) => amount.saturating_sub(value),
            DamageOperation::ClampToZero => amount,
        };
        lines.push(DamageLine::Applied {
            stage: adjustment.stage,
            operation: adjustment.operation,
            origin: adjustment.origin,
            input,
            output: amount,
        });
    }

    DamageResolution {
        context: intent.context,
        base: intent.effect.base,
        constraints: intent.effect.constraints,
        amount,
        lines,
    }
}

/// Commit an evaluated total to one target, queue one destruction check,
/// and flatten the complete ordered event trace.
pub(crate) fn commit(
    state: &GameState,
    resolution: &DamageResolution,
) -> (GameState, Vec<GameEvent>) {
    let mut state = state.clone();
    let target = resolution.context.target;
    let Some(summon) = summon_at_mut(state.players.get_mut(target.controller), target.position)
    else {
        return (state, Vec::new());
    };

    let before = summon.damage;
    let after = before.saturating_add(resolution.amount);
    summon.damage = after;
    state
        .work
        .push_back(WorkItem::DestructionCheck(target.position));

    let mut events = Vec::with_capacity(resolution.lines.len() + 2);
    events.push(GameEvent::DamageCalculationStarted {
        context: resolution.context,
        base: resolution.base,
        constraints: resolution.constraints,
    });
    events.extend(resolution.lines.iter().map(|line| match *line {
        DamageLine::Applied {
            stage,
            operation,
            origin,
            input,
            output,
        } => GameEvent::DamageAdjustmentApplied {
            context: resolution.context,
            stage,
            operation,
            origin,
            input,
            output,
        },
        DamageLine::Skipped {
            stage,
            operation,
            origin,
            input,
            constraint,
        } => GameEvent::DamageAdjustmentSkipped {
            context: resolution.context,
            stage,
            operation,
            origin,
            input,
            constraint,
        },
    }));
    events.push(GameEvent::DamageApplied {
        context: resolution.context,
        amount: resolution.amount,
        before,
        after,
    });

    (state, events)
}

fn collect_adjustments(state: &GameState, intent: &DamageIntent) -> Vec<DamageAdjustment> {
    let origin = intent.context.source.origin();
    let controller = intent.context.source.controller();
    let selected_target = [intent.context.target.position];
    let mut adjustments: Vec<DamageAdjustment> = intent
        .effect
        .additions
        .iter()
        .filter(|addition| condition_holds(state, controller, &selected_target, addition.condition))
        .map(|addition| DamageAdjustment {
            stage: DamageStage::Addition,
            operation: DamageOperation::Add(addition.amount),
            origin,
        })
        .collect();

    if matches!(intent.context.source, DamageSource::Attack { controller, .. }
        if controller.opponent() == intent.context.target.controller)
    {
        for card in &state
            .players
            .get(intent.context.target.controller)
            .enchantments
        {
            let Some(entity) = state.cards.get(card.def) else {
                continue;
            };
            for modifier in entity.all::<Modifier>() {
                if let Modifier::IncomingAttackDamageReduction(amount) = modifier {
                    adjustments.push(DamageAdjustment {
                        stage: DamageStage::PersistentReduction,
                        operation: DamageOperation::Reduce(*amount),
                        origin: DamageOrigin::PersistentCard(card.instance),
                    });
                }
            }
        }
    }

    adjustments
}

/// Whether one controller-relative condition currently holds.
pub(crate) fn condition_holds(
    state: &GameState,
    controller: PlayerId,
    targets: &[Position],
    condition: EffectCondition,
) -> bool {
    match condition {
        EffectCondition::SpellPlayedThisTurn => *state.turn.spell_played_this_turn.get(controller),
        EffectCondition::DefenderEnteredMainThisTurn => targets.first().is_some_and(|&position| {
            summon_at(state.players.get(controller.opponent()), position)
                .is_some_and(|summon| summon.turn.main_entry.is_some())
        }),
    }
}

fn summon_at(player: &PlayerState, position: Position) -> Option<&SummonInstance> {
    match position {
        Position::Main => player.main.as_ref(),
        Position::Bench(slot) => player.bench[slot.index()].as_ref(),
    }
}

fn summon_at_mut(player: &mut PlayerState, position: Position) -> Option<&mut SummonInstance> {
    match position {
        Position::Main => player.main.as_mut(),
        Position::Bench(slot) => player.bench[slot.index()].as_mut(),
    }
}

#[cfg(test)]
mod tests;
