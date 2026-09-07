use summoners_match_log::{
    ActionV1,
    wire::{BenchSlotV1, ManaTypeV1, PlayerIdV1, PositionV1},
};

use crate::protocol::{PendingKindView, PlayerView, Seat};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptState {
    Menu { revision: u64 },
    Form { revision: u64, form: Form },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    PlayCard,
    PlaySlot { card: u32 },
    UpgradeCard,
    UpgradePosition { card: u32 },
    RetreatSlot,
    RetreatMana { slot: BenchSlotV1 },
    AttackTarget,
    AttackMana { target: PositionV1 },
    EndTurn,
    Mana { action: ManaAction },
    Promotion,
    Prize,
    Resign,
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
    let effects = if changed {
        vec![PromptEffect::Cancelled]
    } else {
        Vec::new()
    };
    (PromptState::Menu { revision }, effects)
}

pub fn prompt(view: &PlayerView, _revision: u64) -> Vec<String> {
    let mut lines = vec!["Actions: 1. Play Summon 2. Upgrade Summon 3. Retreat 4. Declare Attack 5. End Turn 6. Pass Priority 7. Convert Coin 8. Resign".to_string()];
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
        _ => return invalid(revision),
    };
    let lines = match form {
        Form::PlayCard | Form::UpgradeCard => hand_lines(view),
        Form::RetreatSlot | Form::Promotion => bench_lines(),
        Form::AttackTarget | Form::UpgradePosition { .. } => position_lines(),
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
        Form::PlayCard => card_next(revision, view, n, |card| Form::PlaySlot { card }),
        Form::PlaySlot { card } => {
            slot_submit(revision, actor, card, n, |slot| ActionV1::PlaySummon {
                player: actor,
                card,
                slot,
            })
        }
        Form::UpgradeCard => card_next(revision, view, n, |card| Form::UpgradePosition { card }),
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
        Form::RetreatMana { .. } | Form::AttackMana { .. } => invalid(revision),
    }
}
fn card_next(
    revision: u64,
    view: &PlayerView,
    n: Option<usize>,
    next: impl FnOnce(u32) -> Form,
) -> (PromptState, Vec<PromptEffect>) {
    n.and_then(|n| view.hand.get(n.saturating_sub(1)))
        .map_or_else(
            || invalid(revision),
            |card| {
                let form = next(card.instance);
                (
                    PromptState::Form { revision, form },
                    vec![PromptEffect::Render(position_lines())],
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
                vec![PromptEffect::Render(mana_lines())],
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
                vec![PromptEffect::Render(mana_lines())],
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
    mana(n).map_or_else(
        || invalid(revision),
        |mana_type| match action {
            ManaAction::Convert => submit(
                revision,
                ActionV1::ConvertCoin {
                    player: actor,
                    mana_type,
                },
            ),
            ManaAction::Choose => submit(
                revision,
                ActionV1::ChooseManaType {
                    player: actor,
                    mana_type,
                },
            ),
            ManaAction::Retreat(slot) => submit(
                revision,
                ActionV1::Retreat {
                    player: actor,
                    slot,
                    mana_hint: Some(mana_type),
                },
            ),
            ManaAction::Attack(target) => submit(
                revision,
                ActionV1::DeclareAttack {
                    player: actor,
                    target,
                    mana_hint: Some(mana_type),
                },
            ),
        },
    )
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
        "4 Bench 3".to_string(),
    ]
}
fn mana_lines() -> Vec<String> {
    vec![
        "1. Matter".to_string(),
        "2. Mind".to_string(),
        "3. Spirit".to_string(),
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
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use crate::protocol::{
        BoardView, CardDescription, HandCardView, ManaView, PhaseView, PlayerPublicView, SeatsView,
    };

    fn view() -> PlayerView {
        let player = PlayerPublicView {
            board: BoardView {
                main: None,
                bench: [None, None, None],
            },
            mana: ManaView {
                matter: 1,
                mind: 1,
                spirit: 1,
            },
            main_losses: 0,
            discard: Vec::new(),
            persistent: Vec::new(),
            deck_count: 3,
            prize_count: 2,
            hand_count: 1,
        };
        PlayerView {
            you: Seat::One,
            hand: vec![HandCardView {
                instance: 9,
                card: CardDescription {
                    name: "Base".to_string(),
                    life: None,
                    retreat_cost: None,
                    mana_types: Vec::new(),
                    cost: None,
                    abilities: Vec::new(),
                    effects: Vec::new(),
                },
            }],
            players: SeatsView {
                one: player.clone(),
                two: player,
            },
            coin: false,
            stack: Vec::new(),
            phase: PhaseView::Main,
            active_player: Seat::One,
            priority_holder: None,
            pending: None,
            outcome: None,
        }
    }
    #[test]
    fn play_form_maps_numbered_hand_and_bench_to_wire_action() {
        let view = view();
        let (state, _) = reduce(PromptState::Menu { revision: 4 }, &view, "1");
        let (state, _) = reduce(state, &view, "1");
        let (_, effects) = reduce(state, &view, "3");
        assert_eq!(
            effects,
            vec![PromptEffect::Submit {
                revision: 4,
                action: ActionV1::PlaySummon {
                    player: PlayerIdV1::One,
                    card: 9,
                    slot: BenchSlotV1::Third
                }
            }]
        );
    }
    #[test]
    fn prize_number_is_one_based_and_wire_index_is_zero_based() {
        let view = view();
        let (_, effects) = form_step(2, &view, PlayerIdV1::One, Form::Prize, Some(2));
        assert_eq!(
            effects,
            vec![PromptEffect::Submit {
                revision: 2,
                action: ActionV1::ChoosePrize {
                    player: PlayerIdV1::One,
                    prize_index: 1
                }
            }]
        );
    }
    #[test]
    fn revision_cancels_active_form() {
        let (state, effects) = revised(
            &PromptState::Form {
                revision: 1,
                form: Form::Prize,
            },
            2,
        );
        assert_eq!(state, PromptState::Menu { revision: 2 });
        assert_eq!(effects, vec![PromptEffect::Cancelled]);
    }
    #[test]
    fn position_order_is_main_then_bench() {
        assert_eq!(position(Some(1)), Some(PositionV1::Main));
        assert_eq!(
            position(Some(4)),
            Some(PositionV1::Bench {
                slot: BenchSlotV1::Third
            })
        );
    }
}
