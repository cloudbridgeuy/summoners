pub(crate) mod dto;
pub(crate) mod model;

use crate::{LoadedSet, LoadPhase, SemanticRule, SetLoadCause, SetLoadError};

pub(crate) fn load(decoded: dto::Set) -> Result<LoadedSet, SetLoadError> {
    let _validated = model::parse(decoded)?;
    Err(SetLoadError::new(
        LoadPhase::Conversion,
        Some(1),
        "$",
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::SetMustContainCards,
        },
    ))
}
