#![allow(clippy::unwrap_used, clippy::expect_used)]

use summoners_match_log::wire::{BenchSlotV1, EntityIdV1, ManaTypeV1, PlayerIdV1, PositionV1};

use super::*;

fn action(input: &str) -> ActionV1 {
    match parse_line(input).unwrap_or_else(|error| panic!("{input:?} must parse: {error}")) {
        ShellInput::Action(action) => action,
        other => panic!("{input:?} must parse to an action, got {other:?}"),
    }
}

fn assert_forms_agree(human: &str, json_body: &str, expected: &ActionV1) {
    assert_eq!(&action(human), expected, "human form: {human:?}");
    let json_line = format!("json {json_body}");
    assert_eq!(&action(&json_line), expected, "json form: {json_line:?}");
}

#[test]
fn play_summon_human_and_json_forms_agree() {
    assert_forms_agree(
        "play-summon one 1 1",
        r#"{"kind":"play_summon","player":"one","card":1,"slot":"first"}"#,
        &ActionV1::PlaySummon {
            player: PlayerIdV1::One,
            card: 1,
            slot: BenchSlotV1::First,
        },
    );
}

#[test]
fn upgrade_summon_human_and_json_forms_agree() {
    assert_forms_agree(
        "upgrade-summon two 2 bench:2",
        r#"{"kind":"upgrade_summon","player":"two","card":2,"position":{"kind":"bench","slot":"second"}}"#,
        &ActionV1::UpgradeSummon {
            player: PlayerIdV1::Two,
            card: 2,
            position: PositionV1::Bench {
                slot: BenchSlotV1::Second,
            },
        },
    );
}

#[test]
fn upgrade_summon_accepts_a_main_position() {
    assert_eq!(
        action("upgrade-summon one 4 main"),
        ActionV1::UpgradeSummon {
            player: PlayerIdV1::One,
            card: 4,
            position: PositionV1::Main,
        }
    );
}

#[test]
fn cast_spell_human_and_json_forms_agree() {
    assert_forms_agree(
        "cast-spell one 3 main --mana mind",
        r#"{"kind":"cast_spell","player":"one","card":3,"targets":[{"kind":"main"}],"mana_hint":"mind"}"#,
        &ActionV1::CastSpell {
            player: PlayerIdV1::One,
            card: 3,
            targets: vec![PositionV1::Main],
            mana_hint: Some(ManaTypeV1::Mind),
        },
    );
}

#[test]
fn cast_spell_accepts_no_targets_and_no_mana_hint() {
    assert_eq!(
        action("cast-spell two 9"),
        ActionV1::CastSpell {
            player: PlayerIdV1::Two,
            card: 9,
            targets: vec![],
            mana_hint: None,
        }
    );
}

#[test]
fn cast_spell_accepts_multiple_targets() {
    assert_eq!(
        action("cast-spell one 3 main bench:1"),
        ActionV1::CastSpell {
            player: PlayerIdV1::One,
            card: 3,
            targets: vec![
                PositionV1::Main,
                PositionV1::Bench {
                    slot: BenchSlotV1::First
                }
            ],
            mana_hint: None,
        }
    );
}

#[test]
fn activate_skill_human_and_json_forms_agree() {
    let ability = "01".repeat(16);
    let human = format!("activate-skill two main {ability} bench:3 --mana spirit");
    let json_body = format!(
        r#"{{"kind":"activate_skill","player":"two","position":{{"kind":"main"}},"ability":"{ability}","targets":[{{"kind":"bench","slot":"third"}}],"mana_hint":"spirit"}}"#
    );
    assert_forms_agree(
        &human,
        &json_body,
        &ActionV1::ActivateSkill {
            player: PlayerIdV1::Two,
            position: PositionV1::Main,
            ability: EntityIdV1(ability.clone()),
            targets: vec![PositionV1::Bench {
                slot: BenchSlotV1::Third,
            }],
            mana_hint: Some(ManaTypeV1::Spirit),
        },
    );
}

#[test]
fn retreat_human_and_json_forms_agree() {
    assert_forms_agree(
        "retreat one 1",
        r#"{"kind":"retreat","player":"one","slot":"first","mana_hint":null}"#,
        &ActionV1::Retreat {
            player: PlayerIdV1::One,
            slot: BenchSlotV1::First,
            mana_hint: None,
        },
    );
}

#[test]
fn retreat_accepts_a_mana_hint() {
    assert_eq!(
        action("retreat two 3 --mana matter"),
        ActionV1::Retreat {
            player: PlayerIdV1::Two,
            slot: BenchSlotV1::Third,
            mana_hint: Some(ManaTypeV1::Matter),
        }
    );
}

#[test]
fn declare_attack_human_and_json_forms_agree() {
    assert_forms_agree(
        "declare-attack one main --mana matter",
        r#"{"kind":"declare_attack","player":"one","target":{"kind":"main"},"mana_hint":"matter"}"#,
        &ActionV1::DeclareAttack {
            player: PlayerIdV1::One,
            target: PositionV1::Main,
            mana_hint: Some(ManaTypeV1::Matter),
        },
    );
}

#[test]
fn end_turn_human_and_json_forms_agree() {
    assert_forms_agree(
        "end-turn one",
        r#"{"kind":"end_turn","player":"one"}"#,
        &ActionV1::EndTurn {
            player: PlayerIdV1::One,
        },
    );
}

#[test]
fn pass_priority_human_and_json_forms_agree() {
    assert_forms_agree(
        "pass-priority two",
        r#"{"kind":"pass_priority","player":"two"}"#,
        &ActionV1::PassPriority {
            player: PlayerIdV1::Two,
        },
    );
}

#[test]
fn convert_coin_human_and_json_forms_agree() {
    assert_forms_agree(
        "convert-coin two spirit",
        r#"{"kind":"convert_coin","player":"two","mana_type":"spirit"}"#,
        &ActionV1::ConvertCoin {
            player: PlayerIdV1::Two,
            mana_type: ManaTypeV1::Spirit,
        },
    );
}

#[test]
fn choose_mana_type_human_and_json_forms_agree() {
    assert_forms_agree(
        "choose-mana-type one matter",
        r#"{"kind":"choose_mana_type","player":"one","mana_type":"matter"}"#,
        &ActionV1::ChooseManaType {
            player: PlayerIdV1::One,
            mana_type: ManaTypeV1::Matter,
        },
    );
}

#[test]
fn choose_promotion_human_and_json_forms_agree() {
    assert_forms_agree(
        "choose-promotion one 3",
        r#"{"kind":"choose_promotion","player":"one","slot":"third"}"#,
        &ActionV1::ChoosePromotion {
            player: PlayerIdV1::One,
            slot: BenchSlotV1::Third,
        },
    );
}

#[test]
fn choose_prize_human_and_json_forms_agree() {
    assert_forms_agree(
        "choose-prize two 7",
        r#"{"kind":"choose_prize","player":"two","prize_index":7}"#,
        &ActionV1::ChoosePrize {
            player: PlayerIdV1::Two,
            prize_index: 7,
        },
    );
}

#[test]
fn resign_human_and_json_forms_agree() {
    assert_forms_agree(
        "resign one",
        r#"{"kind":"resign","player":"one"}"#,
        &ActionV1::Resign {
            player: PlayerIdV1::One,
        },
    );
}

#[test]
fn help_parses_with_no_arguments() {
    assert_eq!(parse_line("help").expect("help parses"), ShellInput::Help);
}

#[test]
fn quit_parses_with_no_arguments() {
    assert_eq!(parse_line("quit").expect("quit parses"), ShellInput::Quit);
}

#[test]
fn state_parses_with_no_arguments() {
    assert_eq!(
        parse_line("state").expect("state parses"),
        ShellInput::State { json: false }
    );
}

#[test]
fn state_json_parses_the_json_flag() {
    assert_eq!(
        parse_line("state --json").expect("state --json parses"),
        ShellInput::State { json: true }
    );
}

#[test]
fn surrounding_whitespace_is_ignored() {
    assert_eq!(
        parse_line("   resign one   ").expect("padded input still parses"),
        ShellInput::Action(ActionV1::Resign {
            player: PlayerIdV1::One
        })
    );
}

#[test]
fn an_empty_line_is_an_unknown_verb_with_no_name() {
    let error = parse_line("").expect_err("an empty line has no verb");
    match error {
        InputError::UnknownVerb { verb } => assert_eq!(verb, ""),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn an_unrecognized_word_is_an_unknown_verb() {
    let error = parse_line("bogus").expect_err("bogus is not a verb");
    match error {
        InputError::UnknownVerb { verb } => assert_eq!(verb, "bogus"),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn a_verb_missing_a_required_argument_reports_it_by_name() {
    let error = parse_line("resign").expect_err("resign needs a player");
    match error {
        InputError::MissingArgument { verb, name } => {
            assert_eq!(verb, "resign");
            assert_eq!(name, "player");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn a_dangling_mana_flag_is_a_missing_argument() {
    let error = parse_line("retreat one --mana").expect_err("--mana needs a value");
    match error {
        InputError::MissingArgument { verb, name } => {
            assert_eq!(verb, "retreat");
            assert_eq!(name, "mana");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn an_invalid_argument_names_the_field_and_the_bad_value() {
    let error = parse_line("resign three").expect_err("three is not a player");
    match error {
        InputError::InvalidArgument { verb, name, value } => {
            assert_eq!(verb, "resign");
            assert_eq!(name, "player");
            assert_eq!(value, "three");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn an_invalid_position_is_an_invalid_argument() {
    let error = parse_line("declare-attack one nowhere").expect_err("nowhere is not a position");
    match error {
        InputError::InvalidArgument { verb, name, value } => {
            assert_eq!(verb, "declare-attack");
            assert_eq!(name, "pos");
            assert_eq!(value, "nowhere");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn trailing_tokens_are_reported_by_verb() {
    let error = parse_line("resign one two").expect_err("resign takes exactly one argument");
    match error {
        InputError::TrailingTokens { verb } => assert_eq!(verb, "resign"),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn state_with_an_unknown_flag_is_trailing_tokens() {
    let error = parse_line("state --verbose").expect_err("state accepts only --json");
    assert!(matches!(
        error,
        InputError::TrailingTokens { verb: "state" }
    ));
}

#[test]
fn malformed_json_is_a_json_error() {
    let error = parse_line("json not json").expect_err("not json is not valid JSON");
    assert!(matches!(error, InputError::Json(_)));
}

#[test]
fn json_with_no_body_is_a_missing_argument() {
    let error = parse_line("json").expect_err("json needs a body");
    match error {
        InputError::MissingArgument { verb, name } => {
            assert_eq!(verb, "json");
            assert_eq!(name, "action");
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn every_input_error_has_a_non_empty_display() {
    let errors: Vec<InputError> = vec![
        InputError::UnknownVerb {
            verb: String::new(),
        },
        InputError::UnknownVerb {
            verb: "bogus".to_string(),
        },
        InputError::MissingArgument {
            verb: "resign",
            name: "player",
        },
        InputError::InvalidArgument {
            verb: "resign",
            name: "player",
            value: "three".to_string(),
        },
        InputError::TrailingTokens { verb: "resign" },
        InputError::Json(serde_json::from_str::<ActionV1>("not json").unwrap_err()),
    ];

    for error in errors {
        assert!(!error.to_string().is_empty());
    }
}
