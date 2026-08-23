//! Pure parsing of one operator-typed line into a shell instruction.
//!
//! `parse_line` never touches a file, a clock, or the game state; it only
//! turns text into `ShellInput`. A game rule (whether an action is legal
//! right now) is never checked here — that is `summoners_core::apply`'s
//! job, reached only once `play` submits the parsed action. A malformed
//! line is not a process failure: it is reported as a typed `InputError`
//! and the caller decides how to continue.

mod actions;

use std::fmt;

use summoners_match_log::wire::{ActionV1, BenchSlotV1, ManaTypeV1, PlayerIdV1, PositionV1};

/// The grammar table `help` prints, and the reference this module's parsing
/// follows.
pub const GRAMMAR_HELP: &str = "\
play-summon <player> <card> <slot>
upgrade-summon <player> <card> <pos>
cast-spell <player> <card> [<pos>...] [--mana <mana>]
activate-skill <player> <pos> <ability> [<pos>...] [--mana <mana>]
retreat <player> <slot> [--mana <mana>]
declare-attack <player> <pos> [--mana <mana>]
end-turn <player>
pass-priority <player>
convert-coin <player> <mana>
choose-mana-type <player> <mana>
choose-promotion <player> <slot>
choose-prize <player> <index>
resign <player>
json <ActionV1 JSON object>
help
state
state --json
quit

<player>  = one | two
<slot>    = 1 | 2 | 3
<pos>     = main | bench:<slot>
<mana>    = matter | mind | spirit
<card>    = the instance ID shown in hand
<ability> = the ability entity ID shown on the Summon
<index>   = a prize index";

/// One parsed operator instruction: either a game action, still in its wire
/// shape, or one of the shell's own commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    /// An action ready to submit, once the shell converts it to a
    /// `GameAction` at the point of submission.
    Action(ActionV1),
    /// Print the grammar table.
    Help,
    /// Reprint the current state, as the compact view or as JSON.
    State { json: bool },
    /// End the session without recording a completed match.
    Quit,
}

/// Why one typed line could not be parsed as a shell instruction.
///
/// This is never a game rule and never a process failure: every variant
/// describes something wrong with the text itself, not with the game.
#[derive(Debug)]
pub enum InputError {
    /// The first word did not name any known verb.
    UnknownVerb { verb: String },
    /// A verb's grammar names an argument this line did not supply.
    MissingArgument {
        verb: &'static str,
        name: &'static str,
    },
    /// An argument was present but did not match its grammar.
    InvalidArgument {
        verb: &'static str,
        name: &'static str,
        value: String,
    },
    /// The line supplied more tokens than the verb's grammar accepts.
    TrailingTokens { verb: &'static str },
    /// The `json` form's argument was not a valid `ActionV1` document.
    Json(serde_json::Error),
}

impl fmt::Display for InputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownVerb { verb } if verb.is_empty() => {
                formatter.write_str("no verb given; type help for the grammar")
            }
            Self::UnknownVerb { verb } => {
                write!(
                    formatter,
                    "unknown verb: {verb} (type help for the grammar)"
                )
            }
            Self::MissingArgument { verb, name } => {
                write!(formatter, "{verb}: missing argument: {name}")
            }
            Self::InvalidArgument { verb, name, value } => {
                write!(formatter, "{verb}: invalid {name}: {value}")
            }
            Self::TrailingTokens { verb } => {
                write!(formatter, "{verb}: unexpected trailing tokens")
            }
            Self::Json(source) => write!(formatter, "json: {source}"),
        }
    }
}

impl std::error::Error for InputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(source) => Some(source),
            _ => None,
        }
    }
}

/// Parse one line the operator typed into a shell instruction.
///
/// Whitespace separates every token; leading and trailing whitespace is
/// ignored. The `json` verb is the one exception: everything after it,
/// verbatim, is handed to `serde_json` so a JSON value's own internal
/// whitespace is never disturbed.
pub fn parse_line(line: &str) -> Result<ShellInput, InputError> {
    let trimmed = line.trim();
    let verb_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    let verb = &trimmed[..verb_end];
    let rest = trimmed[verb_end..].trim_start();

    if verb.is_empty() {
        return Err(InputError::UnknownVerb {
            verb: String::new(),
        });
    }

    if verb == "json" {
        return parse_json(rest);
    }

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    match verb {
        "help" => finish_unit(&tokens, "help", ShellInput::Help),
        "quit" => finish_unit(&tokens, "quit", ShellInput::Quit),
        "state" => parse_state(&tokens),
        "play-summon" => actions::play_summon(tokens),
        "upgrade-summon" => actions::upgrade_summon(tokens),
        "cast-spell" => actions::cast_spell(tokens),
        "activate-skill" => actions::activate_skill(tokens),
        "retreat" => actions::retreat(tokens),
        "declare-attack" => actions::declare_attack(tokens),
        "end-turn" => actions::end_turn(tokens),
        "pass-priority" => actions::pass_priority(tokens),
        "convert-coin" => actions::convert_coin(tokens),
        "choose-mana-type" => actions::choose_mana_type(tokens),
        "choose-promotion" => actions::choose_promotion(tokens),
        "choose-prize" => actions::choose_prize(tokens),
        "resign" => actions::resign(tokens),
        other => Err(InputError::UnknownVerb {
            verb: other.to_string(),
        }),
    }
}

fn parse_json(rest: &str) -> Result<ShellInput, InputError> {
    if rest.is_empty() {
        return Err(InputError::MissingArgument {
            verb: "json",
            name: "action",
        });
    }
    let action: ActionV1 = serde_json::from_str(rest).map_err(InputError::Json)?;
    Ok(ShellInput::Action(action))
}

fn parse_state(tokens: &[&str]) -> Result<ShellInput, InputError> {
    match tokens {
        [] => Ok(ShellInput::State { json: false }),
        ["--json"] => Ok(ShellInput::State { json: true }),
        _ => Err(InputError::TrailingTokens { verb: "state" }),
    }
}

fn finish_unit(
    tokens: &[&str],
    verb: &'static str,
    value: ShellInput,
) -> Result<ShellInput, InputError> {
    if tokens.is_empty() {
        Ok(value)
    } else {
        Err(InputError::TrailingTokens { verb })
    }
}

/// The token stream one verb function consumes, in order, left to right.
type Tokens<'a> = std::vec::IntoIter<&'a str>;

fn next_token(
    verb: &'static str,
    name: &'static str,
    tokens: &mut Tokens<'_>,
) -> Result<String, InputError> {
    tokens
        .next()
        .map(str::to_string)
        .ok_or(InputError::MissingArgument { verb, name })
}

fn ensure_done(verb: &'static str, tokens: &mut Tokens<'_>) -> Result<(), InputError> {
    if tokens.next().is_some() {
        Err(InputError::TrailingTokens { verb })
    } else {
        Ok(())
    }
}

/// Remove the optional `--mana <mana>` flag from wherever it appears in
/// `tokens`, returning the mana type it named. `Ok(None)` means the line
/// did not include the flag at all.
fn extract_mana_flag(
    verb: &'static str,
    tokens: &mut Vec<&str>,
) -> Result<Option<ManaTypeV1>, InputError> {
    let Some(index) = tokens.iter().position(|token| *token == "--mana") else {
        return Ok(None);
    };
    if index + 1 >= tokens.len() {
        return Err(InputError::MissingArgument { verb, name: "mana" });
    }
    let value = tokens.remove(index + 1);
    tokens.remove(index);
    Ok(Some(parse_mana(verb, value)?))
}

fn parse_player(verb: &'static str, token: &str) -> Result<PlayerIdV1, InputError> {
    match token {
        "one" => Ok(PlayerIdV1::One),
        "two" => Ok(PlayerIdV1::Two),
        other => Err(InputError::InvalidArgument {
            verb,
            name: "player",
            value: other.to_string(),
        }),
    }
}

fn parse_slot(verb: &'static str, token: &str) -> Result<BenchSlotV1, InputError> {
    match token {
        "1" => Ok(BenchSlotV1::First),
        "2" => Ok(BenchSlotV1::Second),
        "3" => Ok(BenchSlotV1::Third),
        other => Err(InputError::InvalidArgument {
            verb,
            name: "slot",
            value: other.to_string(),
        }),
    }
}

fn parse_position(verb: &'static str, token: &str) -> Result<PositionV1, InputError> {
    if token == "main" {
        return Ok(PositionV1::Main);
    }
    if let Some(slot) = token.strip_prefix("bench:") {
        return Ok(PositionV1::Bench {
            slot: parse_slot(verb, slot)?,
        });
    }
    Err(InputError::InvalidArgument {
        verb,
        name: "pos",
        value: token.to_string(),
    })
}

fn parse_mana(verb: &'static str, token: &str) -> Result<ManaTypeV1, InputError> {
    match token {
        "matter" => Ok(ManaTypeV1::Matter),
        "mind" => Ok(ManaTypeV1::Mind),
        "spirit" => Ok(ManaTypeV1::Spirit),
        other => Err(InputError::InvalidArgument {
            verb,
            name: "mana",
            value: other.to_string(),
        }),
    }
}

fn parse_card(verb: &'static str, token: &str) -> Result<u32, InputError> {
    token
        .parse::<u32>()
        .map_err(|_| InputError::InvalidArgument {
            verb,
            name: "card",
            value: token.to_string(),
        })
}

fn parse_prize_index(verb: &'static str, token: &str) -> Result<u64, InputError> {
    token
        .parse::<u64>()
        .map_err(|_| InputError::InvalidArgument {
            verb,
            name: "index",
            value: token.to_string(),
        })
}

#[cfg(test)]
mod tests;
