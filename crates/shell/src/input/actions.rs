//! One parsing function per verb in the grammar table, each producing a
//! `ShellInput::Action` carrying the matching `ActionV1` shape.

use summoners_match_log::wire::{ActionV1, EntityIdV1};

use super::{
    InputError, ShellInput, Tokens, ensure_done, extract_mana_flag, next_token, parse_card,
    parse_mana, parse_player, parse_position, parse_prize_index, parse_slot,
};

pub(super) fn play_summon(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "play-summon";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let card = next_token(VERB, "card", &mut tokens)?;
    let slot = next_token(VERB, "slot", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::PlaySummon {
        player: parse_player(VERB, &player)?,
        card: parse_card(VERB, &card)?,
        slot: parse_slot(VERB, &slot)?,
    }))
}

pub(super) fn upgrade_summon(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "upgrade-summon";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let card = next_token(VERB, "card", &mut tokens)?;
    let position = next_token(VERB, "pos", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::UpgradeSummon {
        player: parse_player(VERB, &player)?,
        card: parse_card(VERB, &card)?,
        position: parse_position(VERB, &position)?,
    }))
}

pub(super) fn cast_spell(mut tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "cast-spell";
    let mana_hint = extract_mana_flag(VERB, &mut tokens)?;
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let card = next_token(VERB, "card", &mut tokens)?;

    let mut targets = Vec::new();
    for token in tokens {
        targets.push(parse_position(VERB, token)?);
    }

    Ok(ShellInput::Action(ActionV1::CastSpell {
        player: parse_player(VERB, &player)?,
        card: parse_card(VERB, &card)?,
        targets,
        mana_hint,
    }))
}

pub(super) fn activate_skill(mut tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "activate-skill";
    let mana_hint = extract_mana_flag(VERB, &mut tokens)?;
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let position = next_token(VERB, "pos", &mut tokens)?;
    let ability = next_token(VERB, "ability", &mut tokens)?;

    let mut targets = Vec::new();
    for token in tokens {
        targets.push(parse_position(VERB, token)?);
    }

    Ok(ShellInput::Action(ActionV1::ActivateSkill {
        player: parse_player(VERB, &player)?,
        position: parse_position(VERB, &position)?,
        ability: EntityIdV1(ability),
        targets,
        mana_hint,
    }))
}

pub(super) fn retreat(mut tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "retreat";
    let mana_hint = extract_mana_flag(VERB, &mut tokens)?;
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let slot = next_token(VERB, "slot", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::Retreat {
        player: parse_player(VERB, &player)?,
        slot: parse_slot(VERB, &slot)?,
        mana_hint,
    }))
}

pub(super) fn declare_attack(mut tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "declare-attack";
    let mana_hint = extract_mana_flag(VERB, &mut tokens)?;
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let target = next_token(VERB, "pos", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::DeclareAttack {
        player: parse_player(VERB, &player)?,
        target: parse_position(VERB, &target)?,
        mana_hint,
    }))
}

pub(super) fn end_turn(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "end-turn";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::EndTurn {
        player: parse_player(VERB, &player)?,
    }))
}

pub(super) fn pass_priority(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "pass-priority";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::PassPriority {
        player: parse_player(VERB, &player)?,
    }))
}

pub(super) fn convert_coin(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "convert-coin";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let mana = next_token(VERB, "mana", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::ConvertCoin {
        player: parse_player(VERB, &player)?,
        mana_type: parse_mana(VERB, &mana)?,
    }))
}

pub(super) fn choose_mana_type(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "choose-mana-type";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let mana = next_token(VERB, "mana", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::ChooseManaType {
        player: parse_player(VERB, &player)?,
        mana_type: parse_mana(VERB, &mana)?,
    }))
}

pub(super) fn choose_promotion(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "choose-promotion";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let slot = next_token(VERB, "slot", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::ChoosePromotion {
        player: parse_player(VERB, &player)?,
        slot: parse_slot(VERB, &slot)?,
    }))
}

pub(super) fn choose_prize(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "choose-prize";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    let index = next_token(VERB, "index", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::ChoosePrize {
        player: parse_player(VERB, &player)?,
        prize_index: parse_prize_index(VERB, &index)?,
    }))
}

pub(super) fn resign(tokens: Vec<&str>) -> Result<ShellInput, InputError> {
    const VERB: &str = "resign";
    let mut tokens: Tokens<'_> = tokens.into_iter();
    let player = next_token(VERB, "player", &mut tokens)?;
    ensure_done(VERB, &mut tokens)?;

    Ok(ShellInput::Action(ActionV1::Resign {
        player: parse_player(VERB, &player)?,
    }))
}
