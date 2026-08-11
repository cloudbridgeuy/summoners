//! Priority windows, passing, and declaring the normal attack (rules
//! §29–33, §46–47).
//!
//! A window opens with a named first holder and no pass recorded. Every
//! pass moves Priority to the other player; a pass while `prior_pass` is
//! already set is the second one in a row and closes the window (rules
//! §32–33). Closing the window is where control hands off: if the Stack
//! still holds something above the current segment, `engine::resolution`'s
//! drain picks it up next; if the Stack is completely empty, Combat (if it
//! ever began) is over and the turn hands to the opponent immediately
//! (rules §47–48), so `pass` calls straight into that handover rather than
//! leaving the state resting with nobody able to act.

use crate::domain::cards::{Query, QueryResult, find_def};
use crate::domain::errors::ActionError;
use crate::domain::events::GameEvent;
use crate::domain::ids::{ManaType, PlayerId, Position};
use crate::domain::state::{GameState, Phase, StackItem, StackWindow};
use crate::engine::apply::ActionOutcome;
use crate::engine::payment::{self, PaymentError};
use crate::engine::turn;

/// Declare the active player's one normal attack (rules §29–31). Nothing
/// requires the attacking Summon to be Ready: rules §15's Ready
/// requirement governs Skills only. Declaring begins Combat and pays the
/// printed Attack cost immediately, exactly like `engine::board::retreat`
/// pays a Retreat Cost; the attack itself goes onto the Stack rather than
/// resolving here, and the defending player receives Priority first (rules
/// §31, §46).
pub(crate) fn declare_attack(
    state: &GameState,
    player: PlayerId,
    target: Position,
    mana_hint: Option<ManaType>,
) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }
    // Checked before the phase/window guard so a second declaration is
    // reported as `NormalAttackAlreadyUsed` — the specific rule it broke —
    // even once the first declaration has already moved the phase to
    // Combat and closed the window behind it.
    if state.turn.normal_attack_used {
        return Err(ActionError::NormalAttackAlreadyUsed);
    }
    if state.turn.phase != Phase::Main || state.turn.window.is_some() {
        return Err(ActionError::WrongPhase);
    }
    if target != Position::Main {
        // Rules §30: a normal attack targets Main unless a card's text
        // grants another target; no vanilla fixture grants that yet.
        return Err(ActionError::InvalidTarget);
    }

    let player_state = state.players.get(player);
    let Some(main_summon) = &player_state.main else {
        return Err(ActionError::EmptyPosition);
    };
    let Some(top_def) = find_def(main_summon.chain.top().def) else {
        return Err(ActionError::UnknownCard);
    };
    let Some(QueryResult::Attack { cost, .. }) = top_def.find(Query::Attack) else {
        return Err(ActionError::UnknownCard);
    };

    let payment = match payment::deduct(player_state.mana, cost, mana_hint) {
        Ok(payment) => payment,
        Err(PaymentError::Insufficient(short)) => {
            return Err(ActionError::InsufficientMana { short });
        }
        Err(PaymentError::InvalidHint) => return Err(ActionError::InvalidManaHint),
    };

    let mut next = state.clone();
    next.turn.normal_attack_used = true;
    next.turn.phase = Phase::Combat;
    next.turn.window = Some(window_after_play(player));
    next.stack.push(StackItem::Attack {
        attacker: player,
        target,
    });
    next.players.get_mut(player).mana = payment.bank;

    let mut events: Vec<GameEvent> = payment
        .deductions
        .iter()
        .map(|deduction| GameEvent::ManaDeducted {
            player,
            mana_type: deduction.mana_type,
            amount: deduction.amount,
        })
        .collect();
    events.push(GameEvent::AttackDeclared { player, target });

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

/// The Priority window that follows `player` declaring an attack, ending
/// their turn without attacking, or playing an effect (rules §31, §32,
/// §47). Rules §32 states the general fact: "whenever a player plays a
/// Spell or passes, Priority moves to the other player." `pass` below is
/// the passing half of that sentence; this is the playing half, and it is
/// the same value in every case — the other player becomes the holder and
/// the pass streak starts clean — so `declare_attack` and
/// `engine::turn::end_turn`, which each open that window for their own
/// rule (§31 and §47 respectively), build it here instead of restating it
/// and risking disagreement on the pass streak. Casting a Spell,
/// activating a Skill that creates a Stack effect, and a trigger that
/// opens a response opportunity (rules §38) will call this too.
pub(crate) fn window_after_play(player: PlayerId) -> StackWindow {
    StackWindow {
        holder: player.opponent(),
        prior_pass: false,
    }
}

/// `PassPriority` (rules §32–33). Priority moves to the other player;
/// nothing else changes until a second consecutive pass closes the window.
/// On that second pass, if the Stack has nothing left above the current
/// segment, `engine::turn::handover` runs immediately (rules §47–48) —
/// otherwise the window simply closes and `engine::resolution::drain`
/// resolves what is left the next time `apply` calls it.
pub(crate) fn pass(state: &GameState, player: PlayerId) -> Result<ActionOutcome, ActionError> {
    if state.pending.is_some() {
        return Err(ActionError::PendingInputMismatch);
    }
    let Some(window) = state.turn.window else {
        return Err(ActionError::WrongPhase);
    };
    if window.holder != player {
        return Err(ActionError::NotYourDecision);
    }

    let mut next = state.clone();
    let mut events = vec![GameEvent::PriorityPassed { player }];

    if window.prior_pass {
        next.turn.window = None;
        // Whether every segment on the Stack is now settled, not just the
        // innermost one — a nested respondable-trigger segment closing is a
        // separate, later concern from the turn actually ending.
        if next.stack.is_empty() {
            let handover = turn::handover(&next);
            events.extend(handover.events);
            next = handover.state;
        }
    } else {
        next.turn.window = Some(StackWindow {
            holder: player.opponent(),
            prior_pass: true,
        });
    }

    Ok(ActionOutcome {
        state: next,
        events,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;
    use crate::domain::ids::{BenchSlot, CardInstanceId};
    use crate::domain::state::{
        CardRef, ManaBank, PendingInput, PerPlayer, PlayerState, SummonInstance, TurnState,
        UpgradeChain, WorkItem,
    };
    use std::collections::VecDeque;

    fn summon(owner: PlayerId, def: &'static str) -> SummonInstance {
        SummonInstance {
            chain: UpgradeChain::new(
                CardRef {
                    instance: CardInstanceId(1),
                    def: CardDefId(def),
                },
                vec![],
            ),
            damage: 0,
            ready: true,
            owner,
            controller: owner,
            duration_markers: vec![],
            played_this_turn: false,
            upgraded_this_turn: false,
            entered_main_this_turn: false,
        }
    }

    fn player_state(owner: PlayerId) -> PlayerState {
        PlayerState {
            main: Some(summon(owner, "quarry-whelp")),
            bench: [None, None, None],
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            has_coin: false,
        }
    }

    fn base_state() -> GameState {
        GameState {
            players: PerPlayer::new(player_state(PlayerId::One), player_state(PlayerId::Two)),
            turn: TurnState {
                active_player: PlayerId::One,
                phase: Phase::Main,
                window: None,
                normal_attack_used: false,
                normal_retreat_used: false,
                spell_played_this_turn: false,
            },
            stack: vec![],
            stack_segment_bases: vec![],
            work: VecDeque::new(),
            pending: None,
            outcome: None,
        }
    }

    // --- declare_attack -----------------------------------------------

    #[test]
    fn declare_attack_pays_the_free_cost_opens_a_window_and_pushes_the_stack() {
        let state = base_state();

        let outcome = declare_attack(&state, PlayerId::One, Position::Main, None)
            .expect("Quarry Whelp's Attack is free");

        assert!(outcome.state.turn.normal_attack_used);
        assert_eq!(outcome.state.turn.phase, Phase::Combat);
        assert_eq!(
            outcome.state.turn.window,
            Some(StackWindow {
                holder: PlayerId::Two,
                prior_pass: false,
            })
        );
        assert_eq!(
            outcome.state.stack,
            vec![StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            }]
        );
        assert_eq!(
            outcome.events,
            vec![GameEvent::AttackDeclared {
                player: PlayerId::One,
                target: Position::Main,
            }]
        );
    }

    #[test]
    fn declare_attack_pays_a_nonzero_cost_and_emits_mana_deducted_first() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));
        state.players.get_mut(PlayerId::One).mana = ManaBank {
            matter: 3,
            mind: 0,
            spirit: 0,
        };

        let outcome = declare_attack(&state, PlayerId::One, Position::Main, None)
            .expect("Quarry Brute's Attack costs one Generic and the bank covers it");

        assert_eq!(
            outcome.events,
            vec![
                GameEvent::ManaDeducted {
                    player: PlayerId::One,
                    mana_type: ManaType::Matter,
                    amount: 1,
                },
                GameEvent::AttackDeclared {
                    player: PlayerId::One,
                    target: Position::Main,
                },
            ]
        );
        assert_eq!(outcome.state.players.get(PlayerId::One).mana.matter, 2);
    }

    #[test]
    fn declare_attack_reports_a_mana_shortfall() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));

        let result = declare_attack(&state, PlayerId::One, Position::Main, None);

        assert_eq!(
            result,
            Err(ActionError::InsufficientMana {
                short: ManaBank {
                    matter: 1,
                    mind: 0,
                    spirit: 0,
                }
            })
        );
    }

    #[test]
    fn declare_attack_rejects_a_hint_naming_an_empty_pool() {
        let mut state = base_state();
        state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));
        state.players.get_mut(PlayerId::One).mana = ManaBank {
            matter: 3,
            mind: 0,
            spirit: 0,
        };

        let result = declare_attack(
            &state,
            PlayerId::One,
            Position::Main,
            Some(ManaType::Spirit),
        );

        assert_eq!(result, Err(ActionError::InvalidManaHint));
    }

    #[test]
    fn declare_attack_rejects_a_second_declaration_even_after_a_different_summon_moved_into_main() {
        let mut state = base_state();
        state.turn.normal_attack_used = true;
        state.turn.phase = Phase::Combat;
        // Simulate a different Summon now sitting in Main; the rejection
        // must still be the specific NormalAttackAlreadyUsed, not something
        // that depends on which Summon is there.
        state.players.get_mut(PlayerId::One).main = Some(summon(PlayerId::One, "quarry-brute"));

        let result = declare_attack(&state, PlayerId::One, Position::Main, None);

        assert_eq!(result, Err(ActionError::NormalAttackAlreadyUsed));
    }

    #[test]
    fn declare_attack_rejects_a_bench_target() {
        let state = base_state();

        let result = declare_attack(
            &state,
            PlayerId::One,
            Position::Bench(BenchSlot::First),
            None,
        );

        assert_eq!(result, Err(ActionError::InvalidTarget));
    }

    #[test]
    fn declare_attack_rejects_an_open_window() {
        let mut state = base_state();
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });

        let result = declare_attack(&state, PlayerId::One, Position::Main, None);

        assert_eq!(result, Err(ActionError::WrongPhase));
    }

    #[test]
    fn a_rejected_declaration_leaves_the_callers_state_untouched() {
        let mut state = base_state();
        state.turn.normal_attack_used = true;
        let before = state.clone();

        let _ = declare_attack(&state, PlayerId::One, Position::Main, None);

        assert_eq!(state, before);
    }

    // --- pass: single vs double, window holder gating -------------------

    #[test]
    fn a_single_pass_hands_priority_to_the_other_player_and_records_it() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        });
        state.stack.push(StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        });

        let outcome = pass(&state, PlayerId::Two).expect("Two holds Priority");

        assert_eq!(
            outcome.state.turn.window,
            Some(StackWindow {
                holder: PlayerId::One,
                prior_pass: true,
            })
        );
        assert_eq!(
            outcome.events,
            vec![GameEvent::PriorityPassed {
                player: PlayerId::Two
            }]
        );
    }

    #[test]
    fn only_the_window_holder_may_pass() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        });

        let result = pass(&state, PlayerId::One);

        assert_eq!(result, Err(ActionError::NotYourDecision));
    }

    #[test]
    fn a_double_pass_with_something_still_on_the_stack_just_closes_the_window() {
        let mut state = base_state();
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: true,
        });
        state.stack.push(StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        });

        let outcome = pass(&state, PlayerId::One).expect("One holds Priority");

        assert_eq!(outcome.state.turn.window, None);
        assert_eq!(outcome.state.stack.len(), 1, "left for resolution to drain");
        assert_eq!(
            outcome.events,
            vec![GameEvent::PriorityPassed {
                player: PlayerId::One
            }]
        );
    }

    // --- window_after_play: the other half of rules §32 ------------------

    #[test]
    fn window_after_play_names_the_other_player_as_holder_with_a_clean_streak() {
        assert_eq!(
            window_after_play(PlayerId::One),
            StackWindow {
                holder: PlayerId::Two,
                prior_pass: false,
            }
        );
    }

    #[test]
    fn window_after_play_never_names_the_acting_player_as_holder() {
        assert_ne!(window_after_play(PlayerId::One).holder, PlayerId::One);
        assert_ne!(window_after_play(PlayerId::Two).holder, PlayerId::Two);
    }

    #[test]
    fn an_intervening_played_effect_clears_prior_pass_so_only_the_final_two_passes_are_consecutive()
    {
        // Rules §33's worked example: the defender passes, the attacker
        // plays an effect through the real §32 bookkeeping instead of
        // passing, the defender passes again, then the attacker passes —
        // only that last pair is consecutive.
        let mut state = base_state();
        state.turn.phase = Phase::Combat;
        state.turn.window = Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        });
        state.stack.push(StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        });

        // Defender (Two) passes first.
        let after_first_pass = pass(&state, PlayerId::Two).expect("Two holds Priority");
        assert_eq!(
            after_first_pass.state.turn.window,
            Some(StackWindow {
                holder: PlayerId::One,
                prior_pass: true,
            })
        );

        // The attacker (One) plays an effect instead of passing: Priority
        // moves to Two and the pass streak clears (rules §32), through the
        // same `window_after_play` a future Spell cast would call.
        let mut after_played_effect = after_first_pass.state;
        after_played_effect.turn.window = Some(window_after_play(PlayerId::One));

        // Defender passes again — the first of a fresh streak.
        let after_second_pass =
            pass(&after_played_effect, PlayerId::Two).expect("Two holds Priority");
        assert_eq!(
            after_second_pass.state.turn.window,
            Some(StackWindow {
                holder: PlayerId::One,
                prior_pass: true,
            }),
            "this is only the first pass since the played effect"
        );

        // The attacker passes: now two in a row, so the window closes.
        let after_third_pass =
            pass(&after_second_pass.state, PlayerId::One).expect("One holds Priority");
        assert_eq!(after_third_pass.state.turn.window, None);
    }

    #[test]
    fn a_double_pass_with_an_empty_stack_hands_the_turn_over_immediately() {
        let mut state = base_state();
        state.turn.window = Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: true,
        });

        let outcome = pass(&state, PlayerId::Two).expect("Two holds Priority");

        assert_eq!(outcome.state.turn.window, None);
        assert_eq!(outcome.state.turn.active_player, PlayerId::Two);
        assert_eq!(outcome.state.turn.phase, Phase::Upkeep);
        assert_eq!(
            outcome.events,
            vec![
                GameEvent::PriorityPassed {
                    player: PlayerId::Two
                },
                GameEvent::TurnBegan {
                    player: PlayerId::Two
                },
            ]
        );
        assert_eq!(
            outcome.state.work,
            VecDeque::from(vec![
                WorkItem::ReadyAll,
                WorkItem::DrawCard,
                WorkItem::ProduceMana(crate::domain::state::ManaSource::Player),
            ])
        );
    }

    #[test]
    fn pass_rejects_when_no_window_is_open() {
        let state = base_state();

        let result = pass(&state, PlayerId::One);

        assert_eq!(result, Err(ActionError::WrongPhase));
    }

    #[test]
    fn pass_rejects_a_pending_decision() {
        let mut state = base_state();
        state.turn.window = Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        });
        state.pending = Some(PendingInput::Promotion {
            player: PlayerId::One,
        });

        let result = pass(&state, PlayerId::One);

        assert_eq!(result, Err(ActionError::PendingInputMismatch));
    }
}
