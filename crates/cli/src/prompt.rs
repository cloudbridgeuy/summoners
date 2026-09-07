use summoners_match_log::{
    ActionV1,
    wire::{BenchSlotV1, ManaTypeV1, PlayerIdV1, PositionV1},
};

use crate::protocol::{AbilityDescription, AbilityKind, PendingKindView, PlayerView, Seat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptState {
    Menu { revision: u64 },
    Form { revision: u64, form: Form },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    PlayCard,
    PlaySlot {
        card: u32,
    },
    UpgradeCard,
    UpgradePosition {
        card: u32,
    },
    RetreatSlot,
    AttackTarget,
    EndTurn,
    Mana {
        action: ManaAction,
    },
    Promotion,
    Prize,
    CastCard,
    SkillPosition,
    SkillAbility {
        position: PositionV1,
    },
    Targets {
        action: TargetAction,
        targets: TargetList,
    },
    ManaHint {
        action: TargetAction,
        targets: TargetList,
    },
    Resign,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetList(pub [Option<PositionV1>; 4]);
impl TargetList {
    fn empty() -> Self {
        Self([None; 4])
    }
    fn values(self) -> Vec<PositionV1> {
        self.0.into_iter().flatten().collect()
    }
    fn push(&mut self, target: PositionV1) -> bool {
        if let Some(slot) = self.0.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(target);
            true
        } else {
            false
        }
    }
    fn undo(&mut self) {
        if let Some(slot) = self.0.iter_mut().rev().find(|slot| slot.is_some()) {
            *slot = None;
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetAction {
    Cast(u32),
    Skill { position: PositionV1, skill: usize },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaAction {
    Retreat(BenchSlotV1),
    Attack(PositionV1),
    Convert,
    Choose,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptEffect {
    Render(Vec<String>),
    Submit { revision: u64, action: ActionV1 },
    Cancelled,
}

pub fn revised(state: &PromptState, revision: u64) -> (PromptState, Vec<PromptEffect>) {
    let changed = match state {
        PromptState::Menu { revision: old } | PromptState::Form { revision: old, .. } => {
            *old != revision
        }
    };
    if changed {
        (
            PromptState::Menu { revision },
            vec![PromptEffect::Cancelled],
        )
    } else {
        (*state, Vec::new())
    }
}

pub fn prompt(view: &PlayerView, _revision: u64) -> Vec<String> {
    let mut lines = vec!["Actions: 1. Play Summon 2. Upgrade Summon 3. Retreat 4. Declare Attack 5. End Turn 6. Pass Priority 7. Convert Coin 8. Resign 9. Cast Spell 10. Activate Skill".to_string()];
    if let Some(pending) = &view.pending {
        lines.push(format!(
            "Awaiting {:?} from {:?}",
            pending.kind, pending.owner
        ));
    }
    if view.priority_holder.is_some() {
        lines.push(format!("Priority: {:?}", view.priority_holder));
    }
    lines
}

pub fn reduce(
    state: PromptState,
    view: &PlayerView,
    input: &str,
) -> (PromptState, Vec<PromptEffect>) {
    let revision = match &state {
        PromptState::Menu { revision } | PromptState::Form { revision, .. } => *revision,
    };
    let actor = player(view.you);
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("cancel") {
        return (
            PromptState::Menu { revision },
            vec![
                PromptEffect::Cancelled,
                PromptEffect::Render(prompt(view, revision)),
            ],
        );
    }
    let number = if matches!(
        state,
        PromptState::Form {
            form: Form::Resign,
            ..
        }
    ) && trimmed.eq_ignore_ascii_case("yes")
    {
        Some(1)
    } else {
        trimmed.parse::<usize>().ok()
    };
    match state {
        PromptState::Menu { .. } => menu(revision, view, number),
        PromptState::Form { form, .. } => form_step(revision, view, actor, form, number),
    }
}

fn menu(
    revision: u64,
    view: &PlayerView,
    number: Option<usize>,
) -> (PromptState, Vec<PromptEffect>) {
    let form = match number {
        Some(1) => Form::PlayCard,
        Some(2) => Form::UpgradeCard,
        Some(3) => Form::RetreatSlot,
        Some(4) => Form::AttackTarget,
        Some(5) => Form::EndTurn,
        Some(6) => {
            return submit(
                revision,
                ActionV1::PassPriority {
                    player: player(view.you),
                },
            );
        }
        Some(7) => Form::Mana {
            action: ManaAction::Convert,
        },
        Some(8) => Form::Resign,
        Some(9) => Form::CastCard,
        Some(10) => Form::SkillPosition,
        _ => return invalid(revision),
    };
    let lines = match form {
        Form::PlayCard | Form::UpgradeCard | Form::CastCard => hand_lines(view),
        Form::SkillPosition => position_lines(),
        Form::RetreatSlot => bench_lines(),
        Form::AttackTarget => position_lines(),
        Form::EndTurn => vec!["Enter 1 to confirm, or cancel".to_string()],
        Form::Resign => vec!["Confirm Give up with yes".to_string()],
        Form::Mana { .. } => mana_lines(),
        _ => Vec::new(),
    };
    (
        PromptState::Form { revision, form },
        vec![PromptEffect::Render(lines)],
    )
}
fn form_step(
    revision: u64,
    view: &PlayerView,
    actor: PlayerIdV1,
    form: Form,
    n: Option<usize>,
) -> (PromptState, Vec<PromptEffect>) {
    match form {
        Form::PlayCard => card_next(revision, view, n, bench_lines(), |card| Form::PlaySlot {
            card,
        }),
        Form::PlaySlot { card } => {
            slot_submit(revision, actor, card, n, |slot| ActionV1::PlaySummon {
                player: actor,
                card,
                slot,
            })
        }
        Form::UpgradeCard => card_next(revision, view, n, position_lines(), |card| {
            Form::UpgradePosition { card }
        }),
        Form::UpgradePosition { card } => {
            position_submit(revision, actor, n, |position| ActionV1::UpgradeSummon {
                player: actor,
                card,
                position,
            })
        }
        Form::RetreatSlot => slot_next(revision, n, |slot| Form::Mana {
            action: ManaAction::Retreat(slot),
        }),
        Form::AttackTarget => position_next(revision, n, |target| Form::Mana {
            action: ManaAction::Attack(target),
        }),
        Form::Mana { action } => mana_submit(revision, actor, action, n),
        Form::EndTurn => confirm_submit(revision, actor, n, ActionV1::EndTurn { player: actor }),
        Form::Resign => confirm_submit(revision, actor, n, ActionV1::Resign { player: actor }),
        Form::Promotion => slot_submit(revision, actor, 0, n, |slot| ActionV1::ChoosePromotion {
            player: actor,
            slot,
        }),
        Form::Prize => n.filter(|n| *n > 0).map_or_else(
            || invalid(revision),
            |n| {
                submit(
                    revision,
                    ActionV1::ChoosePrize {
                        player: actor,
                        prize_index: (n - 1) as u64,
                    },
                )
            },
        ),
        Form::CastCard => card_next(revision, view, n, target_lines(), |card| Form::Targets {
            action: TargetAction::Cast(card),
            targets: TargetList::empty(),
        }),
        Form::Targets { action, targets } => target_step(revision, action, targets, n),
        Form::ManaHint { action, targets } => mana_hint(n).map_or_else(
            || invalid(revision),
            |mana_hint| target_submit(revision, view, actor, (action, targets), mana_hint),
        ),
        Form::SkillPosition => position(n).map_or_else(
            || invalid(revision),
            |position| {
                (
                    PromptState::Form {
                        revision,
                        form: Form::SkillAbility { position },
                    },
                    vec![PromptEffect::Render(skill_lines(view, position))],
                )
            },
        ),
        Form::SkillAbility { position } => skill_next(revision, view, position, n),
    }
}
fn target_step(
    revision: u64,
    action: TargetAction,
    mut targets: TargetList,
    n: Option<usize>,
) -> (PromptState, Vec<PromptEffect>) {
    match n {
        Some(5) => (
            PromptState::Form {
                revision,
                form: Form::ManaHint { action, targets },
            },
            vec![PromptEffect::Render(mana_hint_lines())],
        ),
        Some(6) => {
            targets.undo();
            target_form(revision, action, targets)
        }
        Some(7) => target_form(revision, action, TargetList::empty()),
        _ => position(n).map_or_else(
            || invalid(revision),
            |position| {
                if targets.push(position) {
                    target_form(revision, action, targets)
                } else {
                    invalid(revision)
                }
            },
        ),
    }
}
fn target_form(
    revision: u64,
    action: TargetAction,
    targets: TargetList,
) -> (PromptState, Vec<PromptEffect>) {
    (
        PromptState::Form {
            revision,
            form: Form::Targets { action, targets },
        },
        vec![PromptEffect::Render(target_lines())],
    )
}
fn target_submit(
    revision: u64,
    view: &PlayerView,
    actor: PlayerIdV1,
    target: (TargetAction, TargetList),
    mana_hint: Option<ManaTypeV1>,
) -> (PromptState, Vec<PromptEffect>) {
    let (action, targets) = target;
    let action = match action {
        TargetAction::Cast(card) => ActionV1::CastSpell {
            player: actor,
            card,
            targets: targets.values(),
            mana_hint,
        },
        TargetAction::Skill { position, skill } => {
            let skills = skills_at(view, position);
            let Some(ability) = skills.get(skill) else {
                return invalid(revision);
            };
            ActionV1::ActivateSkill {
                player: actor,
                position,
                ability: ability.id.clone(),
                targets: targets.values(),
                mana_hint,
            }
        }
    };
    submit(revision, action)
}
fn skill_next(
    revision: u64,
    view: &PlayerView,
    position: PositionV1,
    n: Option<usize>,
) -> (PromptState, Vec<PromptEffect>) {
    let skills = skills_at(view, position);
    let skill = n.unwrap_or(0).saturating_sub(1);
    if skills.get(skill).is_none() {
        return invalid(revision);
    }
    target_form(
        revision,
        TargetAction::Skill { position, skill },
        TargetList::empty(),
    )
}
fn skills_at(view: &PlayerView, position: PositionV1) -> Vec<AbilityDescription> {
    let board = match view.you {
        Seat::One => &view.players.one.board,
        Seat::Two => &view.players.two.board,
    };
    let summon = match position {
        PositionV1::Main => board.main.as_ref(),
        PositionV1::Bench { slot } => board.bench[match slot {
            BenchSlotV1::First => 0,
            BenchSlotV1::Second => 1,
            BenchSlotV1::Third => 2,
        }]
        .as_ref(),
    };
    summon
        .and_then(|summon| summon.chain.last())
        .map_or_else(Vec::new, |card| {
            card.abilities
                .iter()
                .filter(|ability| ability.kind == AbilityKind::Skill)
                .cloned()
                .collect()
        })
}
fn skill_lines(view: &PlayerView, position: PositionV1) -> Vec<String> {
    skills_at(view, position)
        .into_iter()
        .enumerate()
        .map(|(index, skill)| format!("{}. {}", index + 1, skill.name))
        .collect()
}
fn card_next(
    revision: u64,
    view: &PlayerView,
    n: Option<usize>,
    lines: Vec<String>,
    next: impl FnOnce(u32) -> Form,
) -> (PromptState, Vec<PromptEffect>) {
    n.filter(|n| *n >= 1)
        .and_then(|n| view.hand.get(n - 1))
        .map_or_else(
            || invalid(revision),
            |card| {
                let form = next(card.instance);
                (
                    PromptState::Form { revision, form },
                    vec![PromptEffect::Render(lines)],
                )
            },
        )
}
fn slot_next(
    revision: u64,
    n: Option<usize>,
    next: impl FnOnce(BenchSlotV1) -> Form,
) -> (PromptState, Vec<PromptEffect>) {
    slot(n).map_or_else(
        || invalid(revision),
        |slot| {
            let form = next(slot);
            (
                PromptState::Form { revision, form },
                vec![PromptEffect::Render(mana_hint_lines())],
            )
        },
    )
}
fn position_next(
    revision: u64,
    n: Option<usize>,
    next: impl FnOnce(PositionV1) -> Form,
) -> (PromptState, Vec<PromptEffect>) {
    position(n).map_or_else(
        || invalid(revision),
        |p| {
            let form = next(p);
            (
                PromptState::Form { revision, form },
                vec![PromptEffect::Render(mana_hint_lines())],
            )
        },
    )
}
fn slot_submit(
    revision: u64,
    _: PlayerIdV1,
    _: u32,
    n: Option<usize>,
    make: impl FnOnce(BenchSlotV1) -> ActionV1,
) -> (PromptState, Vec<PromptEffect>) {
    slot(n).map_or_else(|| invalid(revision), |s| submit(revision, make(s)))
}
fn position_submit(
    revision: u64,
    _: PlayerIdV1,
    n: Option<usize>,
    make: impl FnOnce(PositionV1) -> ActionV1,
) -> (PromptState, Vec<PromptEffect>) {
    position(n).map_or_else(|| invalid(revision), |p| submit(revision, make(p)))
}
fn mana_submit(
    revision: u64,
    actor: PlayerIdV1,
    action: ManaAction,
    n: Option<usize>,
) -> (PromptState, Vec<PromptEffect>) {
    match action {
        ManaAction::Convert => mana(n).map_or_else(
            || invalid(revision),
            |mana_type| {
                submit(
                    revision,
                    ActionV1::ConvertCoin {
                        player: actor,
                        mana_type,
                    },
                )
            },
        ),
        ManaAction::Choose => mana(n).map_or_else(
            || invalid(revision),
            |mana_type| {
                submit(
                    revision,
                    ActionV1::ChooseManaType {
                        player: actor,
                        mana_type,
                    },
                )
            },
        ),
        ManaAction::Retreat(slot) => mana_hint(n).map_or_else(
            || invalid(revision),
            |mana_hint| {
                submit(
                    revision,
                    ActionV1::Retreat {
                        player: actor,
                        slot,
                        mana_hint,
                    },
                )
            },
        ),
        ManaAction::Attack(target) => mana_hint(n).map_or_else(
            || invalid(revision),
            |mana_hint| {
                submit(
                    revision,
                    ActionV1::DeclareAttack {
                        player: actor,
                        target,
                        mana_hint,
                    },
                )
            },
        ),
    }
}
fn confirm_submit(
    revision: u64,
    actor: PlayerIdV1,
    n: Option<usize>,
    action: ActionV1,
) -> (PromptState, Vec<PromptEffect>) {
    if n == Some(1) {
        submit(revision, action)
    } else {
        let _ = actor;
        invalid(revision)
    }
}
fn submit(revision: u64, action: ActionV1) -> (PromptState, Vec<PromptEffect>) {
    (
        PromptState::Menu { revision },
        vec![PromptEffect::Submit { revision, action }],
    )
}
fn invalid(revision: u64) -> (PromptState, Vec<PromptEffect>) {
    (
        PromptState::Menu { revision },
        vec![PromptEffect::Render(vec!["Invalid selection".to_string()])],
    )
}
fn player(seat: Seat) -> PlayerIdV1 {
    match seat {
        Seat::One => PlayerIdV1::One,
        Seat::Two => PlayerIdV1::Two,
    }
}
fn slot(n: Option<usize>) -> Option<BenchSlotV1> {
    match n {
        Some(1) => Some(BenchSlotV1::First),
        Some(2) => Some(BenchSlotV1::Second),
        Some(3) => Some(BenchSlotV1::Third),
        _ => None,
    }
}
fn position(n: Option<usize>) -> Option<PositionV1> {
    match n {
        Some(1) => Some(PositionV1::Main),
        Some(2) => Some(PositionV1::Bench {
            slot: BenchSlotV1::First,
        }),
        Some(3) => Some(PositionV1::Bench {
            slot: BenchSlotV1::Second,
        }),
        Some(4) => Some(PositionV1::Bench {
            slot: BenchSlotV1::Third,
        }),
        _ => None,
    }
}
fn mana(n: Option<usize>) -> Option<ManaTypeV1> {
    match n {
        Some(1) => Some(ManaTypeV1::Matter),
        Some(2) => Some(ManaTypeV1::Mind),
        Some(3) => Some(ManaTypeV1::Spirit),
        _ => None,
    }
}
fn mana_hint(n: Option<usize>) -> Option<Option<ManaTypeV1>> {
    match n {
        Some(1) => Some(Some(ManaTypeV1::Matter)),
        Some(2) => Some(Some(ManaTypeV1::Mind)),
        Some(3) => Some(Some(ManaTypeV1::Spirit)),
        Some(4) => Some(None),
        _ => None,
    }
}
fn hand_lines(view: &PlayerView) -> Vec<String> {
    view.hand
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", i + 1, c.card.name))
        .collect()
}
fn bench_lines() -> Vec<String> {
    vec![
        "1. Bench 1".to_string(),
        "2. Bench 2".to_string(),
        "3. Bench 3".to_string(),
    ]
}
fn position_lines() -> Vec<String> {
    vec![
        "1. Main".to_string(),
        "2. Bench 1".to_string(),
        "3. Bench 2".to_string(),
        "4. Bench 3".to_string(),
    ]
}
fn mana_lines() -> Vec<String> {
    vec![
        "1. Matter".to_string(),
        "2. Mind".to_string(),
        "3. Spirit".to_string(),
    ]
}
fn mana_hint_lines() -> Vec<String> {
    vec![
        "1. Matter".to_string(),
        "2. Mind".to_string(),
        "3. Spirit".to_string(),
        "4. No hint".to_string(),
    ]
}
fn target_lines() -> Vec<String> {
    vec![
        "1. Main".to_string(),
        "2. Bench 1".to_string(),
        "3. Bench 2".to_string(),
        "4. Bench 3".to_string(),
        "5. Done".to_string(),
        "6. Undo".to_string(),
        "7. Clear".to_string(),
    ]
}

pub fn forced_form(view: &PlayerView) -> Option<Form> {
    match view.pending.as_ref()?.kind {
        PendingKindView::ManaProduction => Some(Form::Mana {
            action: ManaAction::Choose,
        }),
        PendingKindView::Promotion => Some(Form::Promotion),
        PendingKindView::PrizePick => Some(Form::Prize),
    }
}

#[cfg(test)]
mod spell_skill_tests;
#[cfg(test)]
mod tests;
