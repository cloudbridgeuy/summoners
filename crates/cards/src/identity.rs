use summoners_core::domain::cards::EntityId;
use uuid::Uuid;

use crate::{LoadPhase, SetLoadCause, SetLoadError};
use crate::v1::model::{AbilityRole, StableCode};

/// Fixed namespace for every authored Summoners Set identity.
const ID_NAMESPACE: Uuid = Uuid::from_bytes([
    0x7d, 0x15, 0xee, 0x70, 0x92, 0x65, 0x4d, 0x4b, 0xa2, 0x58, 0x98, 0x3d, 0x27, 0xc5,
    0x1f, 0x35,
]);

pub(crate) fn set_id(code: &StableCode) -> Result<EntityId, SetLoadError> {
    mint(&format!("set:{}", code.as_str()), "id")
}

pub(crate) fn card_id(
    set: &StableCode,
    card: &StableCode,
    path: &str,
) -> Result<EntityId, SetLoadError> {
    mint(
        &format!("set:{}/card:{}", set.as_str(), card.as_str()),
        path,
    )
}

pub(crate) fn ability_id(
    set: &StableCode,
    card: &StableCode,
    role: AbilityRole,
    ability: &StableCode,
    path: &str,
) -> Result<EntityId, SetLoadError> {
    mint(
        &format!(
            "set:{}/card:{}/ability:{}:{}",
            set.as_str(),
            card.as_str(),
            role.identity_name(),
            ability.as_str()
        ),
        path,
    )
}

fn mint(input: &str, path: &str) -> Result<EntityId, SetLoadError> {
    let uuid = Uuid::new_v5(&ID_NAMESPACE, input.as_bytes());
    let canonical = uuid.hyphenated().to_string();
    EntityId::parse(&canonical).map_err(|_| {
        SetLoadError::new(
            LoadPhase::Identity,
            Some(1),
            path,
            SetLoadCause::GeneratedIdRejected { id: canonical },
        )
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::StableKeyKind;

    fn code(value: &str) -> StableCode {
        StableCode::parse(value.to_string(), StableKeyKind::Card, "code")
            .expect("fixture code is valid")
    }

    #[test]
    fn mint_is_stable_for_the_same_identity_input() {
        let set = code("foundations");
        let card = code("ember-lance");
        assert_eq!(
            card_id(&set, &card, "cards[0].code"),
            card_id(&set, &card, "cards[9].code")
        );
    }

    #[test]
    fn role_is_part_of_an_ability_identity() {
        let set = code("foundations");
        let card = code("ember-lance");
        let ability = code("resolve");
        let attack = ability_id(&set, &card, AbilityRole::Attack, &ability, "ability");
        let trigger = ability_id(&set, &card, AbilityRole::Trigger, &ability, "ability");
        assert_ne!(attack, trigger);
    }

    #[test]
    fn foundations_identity_inputs_have_golden_uuid_values() {
        let set = code("foundations");
        let card = code("ember-lance");
        let ability = code("scorch");
        assert_eq!(
            Uuid::new_v5(&ID_NAMESPACE, b"set:foundations").to_string(),
            "90322d38-c099-5243-8016-61aff2241791"
        );
        assert_eq!(
            Uuid::new_v5(&ID_NAMESPACE, b"set:foundations/card:ember-lance").to_string(),
            "67505fc1-79c1-5229-a4c2-6c5f6659fb2d"
        );
        assert_eq!(
            ability_id(&set, &card, AbilityRole::Attack, &ability, "ability")
                .expect("minted id is valid"),
            EntityId::parse("cfcf8272-7949-50e3-9f4a-7da8fa987021")
                .expect("golden id is valid")
        );
    }
}
