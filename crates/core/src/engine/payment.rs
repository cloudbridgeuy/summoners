//! The cost deduction policy (design decision 14).
//!
//! Typed cost components always consume their matching pools; if the bank
//! cannot cover them, nothing else is even attempted. A Generic component
//! follows an optional `mana_hint` when the caller supplies one: it is paid
//! entirely from the hinted pool. Without a hint, Generic Mana is paid one
//! unit at a time from the currently largest remaining pool; on a tie the
//! policy prefers a pool the cost's typed part does not name; a further tie
//! falls back to the fixed order Matter, then Mind, then Spirit.
//!
//! This module is a pure function over its three inputs. It never reads or
//! mutates a `GameState`; callers own applying the returned bank.

use crate::domain::cards::Cost;
use crate::domain::ids::ManaType;
use crate::domain::state::ManaBank;

/// The three typed pools in the final tiebreak order (decision 14: "if
/// still tied, the order is Matter, Mind, Spirit").
const POOL_ORDER: [ManaType; 3] = [ManaType::Matter, ManaType::Mind, ManaType::Spirit];

/// One typed pool's fixed slot in the three-element arrays this module
/// simulates with.
fn slot(mana_type: ManaType) -> usize {
    match mana_type {
        ManaType::Matter => 0,
        ManaType::Mind => 1,
        ManaType::Spirit => 2,
    }
}

fn to_array(bank: ManaBank) -> [i64; 3] {
    [
        i64::from(bank.matter),
        i64::from(bank.mind),
        i64::from(bank.spirit),
    ]
}

/// How much of a negative simulated pool is owed, as an unsigned shortfall.
fn unsigned_deficit(value: i64) -> u32 {
    if value >= 0 {
        0
    } else {
        u32::try_from(-value).unwrap_or(u32::MAX)
    }
}

/// Choose which pool the next unhinted Generic unit is paid from: the
/// numerically largest remaining pool; ties prefer a pool `typed_named`
/// marks `false`; further ties fall back to Matter, Mind, Spirit (the fixed
/// order already walked by `POOL_ORDER`, so keeping the earlier index on a
/// tie is exactly that fallback).
fn pick_largest_pool(remaining: [i64; 3], typed_named: [bool; 3]) -> ManaType {
    let mut best = ManaType::Matter;
    for candidate in POOL_ORDER {
        let candidate_value = remaining[slot(candidate)];
        let best_value = remaining[slot(best)];
        let candidate_wins = match candidate_value.cmp(&best_value) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Equal => !typed_named[slot(candidate)] && typed_named[slot(best)],
            std::cmp::Ordering::Less => false,
        };
        if candidate_wins {
            best = candidate;
        }
    }
    best
}

/// One typed pool actually drawn from to pay a cost, enough to build a
/// `GameEvent::ManaDeducted` fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deduction {
    pub mana_type: ManaType,
    pub amount: u32,
}

/// A completed payment: the bank after deduction, and one `Deduction` per
/// pool actually drawn from, in Matter, Mind, Spirit order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Payment {
    pub bank: ManaBank,
    pub deductions: Vec<Deduction>,
}

/// Why a payment could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaymentError {
    /// The bank cannot cover `cost`, with or without applying `mana_hint`;
    /// `short` names how much of which typed pools were missing.
    Insufficient(ManaBank),
    /// A `mana_hint` was supplied but cannot pay any Generic component of
    /// this cost: either the cost carries no Generic component at all, or
    /// the hinted pool has nothing left in it once the typed components are
    /// set aside.
    InvalidHint,
}

/// Apply decision 14's policy: deduct `cost` from `bank`, steering the
/// Generic component with `hint` when one is given. Pure — never mutates
/// its inputs; the caller applies `Payment::bank` itself.
pub(crate) fn deduct(
    bank: ManaBank,
    cost: Cost,
    hint: Option<ManaType>,
) -> Result<Payment, PaymentError> {
    let typed_short = ManaBank {
        matter: cost.matter.saturating_sub(bank.matter),
        mind: cost.mind.saturating_sub(bank.mind),
        spirit: cost.spirit.saturating_sub(bank.spirit),
    };
    if typed_short.matter > 0 || typed_short.mind > 0 || typed_short.spirit > 0 {
        return Err(PaymentError::Insufficient(typed_short));
    }

    let mut remaining = to_array(bank);
    remaining[slot(ManaType::Matter)] -= i64::from(cost.matter);
    remaining[slot(ManaType::Mind)] -= i64::from(cost.mind);
    remaining[slot(ManaType::Spirit)] -= i64::from(cost.spirit);

    if let Some(hint_type) = hint {
        let nothing_to_pay = cost.generic == 0;
        let hint_pool_empty = remaining[slot(hint_type)] == 0;
        if nothing_to_pay || hint_pool_empty {
            return Err(PaymentError::InvalidHint);
        }
    }

    let typed_named = [cost.matter > 0, cost.mind > 0, cost.spirit > 0];
    let mut generic_spent = [0i64; 3];
    if let Some(hint_type) = hint {
        remaining[slot(hint_type)] -= i64::from(cost.generic);
        generic_spent[slot(hint_type)] += i64::from(cost.generic);
    } else {
        for _ in 0..cost.generic {
            let chosen = pick_largest_pool(remaining, typed_named);
            remaining[slot(chosen)] -= 1;
            generic_spent[slot(chosen)] += 1;
        }
    }

    if remaining.iter().any(|&value| value < 0) {
        let short = ManaBank {
            matter: unsigned_deficit(remaining[slot(ManaType::Matter)]),
            mind: unsigned_deficit(remaining[slot(ManaType::Mind)]),
            spirit: unsigned_deficit(remaining[slot(ManaType::Spirit)]),
        };
        return Err(PaymentError::Insufficient(short));
    }

    let total_spent = [
        i64::from(cost.matter) + generic_spent[0],
        i64::from(cost.mind) + generic_spent[1],
        i64::from(cost.spirit) + generic_spent[2],
    ];
    let deductions = POOL_ORDER
        .into_iter()
        .filter_map(|mana_type| {
            let amount = total_spent[slot(mana_type)];
            (amount > 0).then(|| Deduction {
                mana_type,
                amount: u32::try_from(amount).unwrap_or(u32::MAX),
            })
        })
        .collect();

    let new_bank = ManaBank {
        matter: unsigned_nonnegative(remaining[slot(ManaType::Matter)]),
        mind: unsigned_nonnegative(remaining[slot(ManaType::Mind)]),
        spirit: unsigned_nonnegative(remaining[slot(ManaType::Spirit)]),
    };

    Ok(Payment {
        bank: new_bank,
        deductions,
    })
}

/// Convert a simulated pool value that is known to be non-negative at this
/// point in `deduct` back into the unsigned count `ManaBank` stores.
fn unsigned_nonnegative(value: i64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn bank(matter: u32, mind: u32, spirit: u32) -> ManaBank {
        ManaBank {
            matter,
            mind,
            spirit,
        }
    }

    fn generic_cost(amount: u32) -> Cost {
        Cost {
            generic: amount,
            ..Cost::default()
        }
    }

    #[test]
    fn typed_components_alone_are_paid_from_their_matching_pools() {
        let cost = Cost {
            matter: 2,
            mind: 1,
            spirit: 0,
            generic: 0,
        };
        let payment = deduct(bank(5, 5, 5), cost, None).expect("bank covers the typed cost");

        assert_eq!(payment.bank, bank(3, 4, 5));
        assert_eq!(
            payment.deductions,
            vec![
                Deduction {
                    mana_type: ManaType::Matter,
                    amount: 2
                },
                Deduction {
                    mana_type: ManaType::Mind,
                    amount: 1
                },
            ]
        );
    }

    #[test]
    fn a_zero_cost_deducts_nothing() {
        let payment =
            deduct(bank(1, 2, 3), Cost::default(), None).expect("a free cost always succeeds");

        assert_eq!(payment.bank, bank(1, 2, 3));
        assert!(payment.deductions.is_empty());
    }

    #[test]
    fn typed_shortfall_across_two_pools_is_reported_together() {
        let cost = Cost {
            matter: 3,
            mind: 2,
            spirit: 0,
            generic: 0,
        };
        let error =
            deduct(bank(1, 0, 9), cost, None).expect_err("the bank cannot pay Matter or Mind");

        assert_eq!(
            error,
            PaymentError::Insufficient(ManaBank {
                matter: 2,
                mind: 2,
                spirit: 0
            })
        );
    }

    #[test]
    fn typed_shortfall_is_reported_even_when_generic_could_not_be_evaluated_yet() {
        // Spirit alone could cover a Generic 1, but Matter's typed shortfall
        // is checked first and wins outright (decision 14: typed components
        // always consume their matching pools before Generic is touched).
        let cost = Cost {
            matter: 5,
            mind: 0,
            spirit: 0,
            generic: 1,
        };
        let error = deduct(bank(0, 0, 10), cost, None).expect_err("Matter alone is short");

        assert_eq!(
            error,
            PaymentError::Insufficient(ManaBank {
                matter: 5,
                mind: 0,
                spirit: 0
            })
        );
    }

    #[test]
    fn a_hint_pays_the_entire_generic_component_from_its_named_pool() {
        let payment = deduct(bank(1, 1, 5), generic_cost(3), Some(ManaType::Spirit))
            .expect("Spirit alone covers the Generic 3");

        assert_eq!(payment.bank, bank(1, 1, 2));
        assert_eq!(
            payment.deductions,
            vec![Deduction {
                mana_type: ManaType::Spirit,
                amount: 3
            }]
        );
    }

    #[test]
    fn a_hint_pool_that_overlaps_a_typed_component_aggregates_into_one_deduction() {
        let cost = Cost {
            matter: 1,
            mind: 0,
            spirit: 0,
            generic: 2,
        };
        let payment = deduct(bank(5, 0, 0), cost, Some(ManaType::Matter))
            .expect("Matter covers both the typed and the hinted Generic amount");

        assert_eq!(payment.bank, bank(2, 0, 0));
        assert_eq!(
            payment.deductions,
            vec![Deduction {
                mana_type: ManaType::Matter,
                amount: 3
            }]
        );
    }

    #[test]
    fn a_hint_given_for_a_cost_with_no_generic_component_is_invalid() {
        let cost = Cost {
            matter: 1,
            mind: 0,
            spirit: 0,
            generic: 0,
        };
        let error = deduct(bank(5, 5, 5), cost, Some(ManaType::Mind))
            .expect_err("there is no Generic component for the hint to pay");

        assert_eq!(error, PaymentError::InvalidHint);
    }

    #[test]
    fn a_hint_naming_an_empty_pool_is_invalid_even_when_other_pools_could_pay() {
        let error = deduct(bank(5, 0, 5), generic_cost(1), Some(ManaType::Mind))
            .expect_err("Mind has nothing in it for the hint to draw from");

        assert_eq!(error, PaymentError::InvalidHint);
    }

    #[test]
    fn a_hint_naming_a_pool_with_too_little_is_a_shortfall_not_an_invalid_hint() {
        let error = deduct(bank(5, 1, 5), generic_cost(3), Some(ManaType::Mind))
            .expect_err("Mind has 1 but the Generic component needs 3");

        assert_eq!(
            error,
            PaymentError::Insufficient(ManaBank {
                matter: 0,
                mind: 2,
                spirit: 0
            })
        );
    }

    #[test]
    fn without_a_hint_generic_is_paid_from_the_largest_pool_and_that_choice_can_change_mid_payment()
    {
        // Matter starts strictly largest (3 > 2 > 0) and pays unit 1,
        // dropping to 2 — tied with Mind. Neither pool is named by a typed
        // cost, so the tie falls to the fixed Matter-before-Mind order and
        // Matter pays unit 2 too, dropping to 1. Mind (still 2) is now
        // strictly largest and pays unit 3, dropping to 1 itself — tied
        // with Matter again, so the fixed order picks Matter one more time
        // for unit 4.
        let payment = deduct(bank(3, 2, 0), generic_cost(4), None)
            .expect("the bank has exactly enough spread across two pools");

        assert_eq!(payment.bank, bank(0, 1, 0));
        assert_eq!(
            payment.deductions,
            vec![
                Deduction {
                    mana_type: ManaType::Matter,
                    amount: 3
                },
                Deduction {
                    mana_type: ManaType::Mind,
                    amount: 1
                },
            ]
        );
    }

    #[test]
    fn a_tie_prefers_a_pool_not_named_in_the_typed_part_of_the_cost() {
        // Matter and Mind are tied at 2 after Matter's typed component is
        // set aside; Mind is not named by the typed cost, so it is preferred
        // over Matter even though Matter would win the final Matter/Mind
        // fallback order on its own.
        let cost = Cost {
            matter: 1,
            mind: 0,
            spirit: 0,
            generic: 1,
        };
        let payment =
            deduct(bank(3, 2, 2), cost, None).expect("both pools together cover the cost");

        assert_eq!(payment.bank, bank(2, 1, 2));
        assert_eq!(
            payment.deductions,
            vec![
                Deduction {
                    mana_type: ManaType::Matter,
                    amount: 1
                },
                Deduction {
                    mana_type: ManaType::Mind,
                    amount: 1
                },
            ]
        );
    }

    #[test]
    fn a_tie_among_equally_untyped_pools_falls_back_to_matter_mind_spirit() {
        let payment =
            deduct(bank(1, 1, 1), generic_cost(1), None).expect("any single pool covers 1");

        assert_eq!(payment.bank, bank(0, 1, 1));
        assert_eq!(
            payment.deductions,
            vec![Deduction {
                mana_type: ManaType::Matter,
                amount: 1
            }]
        );
    }

    #[test]
    fn a_tie_among_equally_untyped_pools_prefers_mind_over_spirit_once_matter_is_named() {
        let cost = Cost {
            matter: 1,
            mind: 0,
            spirit: 0,
            generic: 1,
        };
        let payment = deduct(bank(1, 1, 1), cost, None)
            .expect("Matter pays its typed unit and Mind or Spirit can pay the Generic unit");

        // Matter pays its own typed unit, leaving it at 0 — below the
        // Mind/Spirit tie at 1 each. Both are untyped, so the tie falls to
        // the fixed Matter, Mind, Spirit order: Mind wins over Spirit.
        assert_eq!(payment.bank, bank(0, 0, 1));
        assert_eq!(
            payment.deductions,
            vec![
                Deduction {
                    mana_type: ManaType::Matter,
                    amount: 1
                },
                Deduction {
                    mana_type: ManaType::Mind,
                    amount: 1
                },
            ]
        );
    }

    #[test]
    fn without_a_hint_an_unpayable_generic_amount_is_reported_across_every_pool_it_touched() {
        // No typed component; the bank is completely empty. Every unit of
        // Generic 2 is attributed by the same tiebreak order used when
        // pools are non-empty: Matter first, then Mind.
        let error = deduct(bank(0, 0, 0), generic_cost(2), None)
            .expect_err("an empty bank cannot pay any Generic Mana");

        assert_eq!(
            error,
            PaymentError::Insufficient(ManaBank {
                matter: 1,
                mind: 1,
                spirit: 0
            })
        );
    }
}
