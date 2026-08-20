pub(crate) mod dto;
pub(crate) mod model;
mod convert;

use crate::{LoadedSet, SetLoadError};

pub(crate) fn load(decoded: dto::Set) -> Result<LoadedSet, SetLoadError> {
    let validated = model::parse(decoded)?;
    convert::convert(validated)
}
