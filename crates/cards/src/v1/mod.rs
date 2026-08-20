mod convert;
pub(crate) mod dto;
pub(crate) mod model;

use crate::{LoadedSet, SetLoadError};

pub(crate) fn load(decoded: dto::Set) -> Result<LoadedSet, SetLoadError> {
    let validated = model::parse(decoded)?;
    convert::convert(validated)
}
