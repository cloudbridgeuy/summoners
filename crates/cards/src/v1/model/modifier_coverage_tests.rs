#![allow(clippy::unwrap_used)]

use super::*;

fn assert_rule(error: SetLoadError, rule: SemanticRule) {
    let SetLoadError {
        phase, path, cause, ..
    } = error;
    assert_eq!(phase, LoadPhase::Semantics);
    assert_eq!(path, "modifier.amount");
    assert_eq!(cause, SetLoadCause::InvalidSemantics { rule });
}

#[test]
fn modifier_policy_covers_both_variants_zero_and_range() {
    let valid = vec![
        dto::Modifier::OpposingRetreatCost { amount: 1 },
        dto::Modifier::IncomingAttackDamageReduction { amount: 10 },
    ];
    assert_eq!(parse_modifiers(valid, "modifiers").unwrap().len(), 2);
    for modifier in [
        dto::Modifier::OpposingRetreatCost { amount: 0 },
        dto::Modifier::IncomingAttackDamageReduction { amount: 0 },
    ] {
        assert_rule(
            parse_modifier(modifier, "modifier").unwrap_err(),
            SemanticRule::AmountMustBePositive,
        );
    }
    let error = parse_modifiers(
        vec![dto::Modifier::IncomingAttackDamageReduction { amount: 0 }],
        "modifiers",
    )
    .unwrap_err();
    assert_eq!(error.path, "modifiers[0].amount");
    assert_eq!(
        error.cause,
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::AmountMustBePositive,
        }
    );
    assert_rule(
        parse_modifier(
            dto::Modifier::OpposingRetreatCost {
                amount: i32::MAX as u32 + 1,
            },
            "modifier",
        )
        .unwrap_err(),
        SemanticRule::ModifierAmountOutOfRange,
    );
}
