pub(crate) mod dto;

use crate::{LoadedSet, LoadPhase, SemanticRule, SetLoadCause, SetLoadError};

pub(crate) fn load(_decoded: dto::Set) -> Result<LoadedSet, SetLoadError> {
    Err(SetLoadError::new(
        LoadPhase::Semantics,
        Some(1),
        "cards",
        SetLoadCause::InvalidSemantics {
            rule: SemanticRule::SetMustContainCards,
        },
    ))
}
