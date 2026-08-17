//! The card container (`entity`, `set`) and the card vocabulary its
//! `Component` variants name.
//!
//! `entity` and `set` hold the container itself: `Entity`, `Component`, and
//! `CardSet`. A `GameState` carries one `Arc<CardSet>`; every card fact a
//! rule needs comes from reading it directly — `get`, `all`, or `demand` on
//! an `Entity` or one of its nested ability entities.
//!
//! Everything above `entity` is vocabulary a `Component` variant names:
//! `Form`, `SpellTiming`, `TriggerEvent`, `EffectCondition`, `ResponseBlock`,
//! `Cost`, `EffectLeaf`, and `Modifier`. Most of it is `pub`, not
//! `pub(crate)`, because the container's public `Component` enum names it
//! directly.
//!
//! `fixtures` (test-only) holds the entity data every test in this crate
//! builds a `CardSet` against: the vanilla and signature cards, authored as
//! `Entity` values.

/// The three card families (rules §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CardKind {
    Summon,
    Spell,
    Enchantment,
}

/// The card family `entity` prints. Design decision 4 forbids a class field
/// on `Entity`, so a card declares its family the same way it declares any
/// other open category: as a tag inside its `Tags` component, not as a
/// dedicated field. `"spell"` and `"enchantment"` are the two tags this
/// crate's fixtures print; every other entity, tagged or not, is a Summon.
pub(crate) fn family(entity: &entity::Entity) -> CardKind {
    match entity.get::<entity::Tags>() {
        Some(tags) if tags.0.iter().any(|tag| tag == "spell") => CardKind::Spell,
        Some(tags) if tags.0.iter().any(|tag| tag == "enchantment") => CardKind::Enchantment,
        _ => CardKind::Summon,
    }
}

/// A Summon's place in its upgrade chain (rules §20). `pub`, not
/// `pub(crate)`: the new card container's `Component::Form` variant names
/// this type directly, and the card vocabulary is public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    Base,
    Enhanced,
    Elite,
}

/// The two Spell timing families (rules §34). A Support Spell may be cast
/// proactively during its controller's own resting Main Phase or as a legal
/// response; an Attack Spell is tied to Combat and may only be cast as a
/// response while its caster holds Priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellTiming {
    Support,
    Attack,
}

/// The event a `CardNode::Trigger` fires on. This is a starter vocabulary;
/// later work adds events as fixture cards need them. Unlike the rest of
/// this module it is `pub`, not `pub(crate)`: `GameEvent::TriggerFired` and
/// `WorkItem::FireTrigger` are public and both name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerEvent {
    YourUpkeep,
    EntersMain,
    EntersBench,
    LeavesMain,
    LeavesBench,
    AnySummonDestroyed,
}

/// A condition an effect leaf can test before applying a bonus. `pub` for
/// the same reason `EffectLeaf` is: `EffectLeaf::ConditionalBonus` names it
/// and `EffectLeaf` is reachable from the public `StackItem::Trigger`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectCondition {
    DefenderEnteredMainThisTurn,
    SpellPlayedThisTurn,
}

/// The family of response an effect can block. `pub` for the same reason
/// `EffectCondition` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseBlock {
    AttackSpells,
}

/// A typed plus Generic Mana cost (rules §12). Every component may be zero;
/// a Skill's cost may be free (rules §15). `pub`, not `pub(crate)`: the new
/// card container's `Component::Cost` variant names this type directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cost {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
    pub generic: u32,
}

/// The first fixed set of effect leaves, one per signature card ability
/// (design decision 1). Shapes here are provisional. Unlike the rest of
/// this module it is `pub`, not `pub(crate)`, for the same reason
/// `TriggerEvent` is: `StackItem::Trigger` is public and names it directly,
/// since a respondable trigger's effects wait on the Stack like any other
/// entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectLeaf {
    DealDamage {
        amount: u32,
        immutable: bool,
    },
    Heal {
        amount: u32,
    },
    MoveSummon,
    SwapPositions,
    ConditionalBonus {
        condition: EffectCondition,
        amount: u32,
    },
    BlockResponses {
        condition: EffectCondition,
        block: ResponseBlock,
    },
    ReturnSpellFromDiscard,
    LookAtPrizes,
    DrawCards {
        amount: u32,
    },
    ReturnSpellToDeckTop,
    ProduceMana,
    CannotBeMovedByOpponent,
    /// Turn the targeted Summon Ready (rules §53: an effect may Ready an
    /// Exhausted Summon outside Upkeep, letting it activate another Skill).
    /// Not part of the design document's first leaf set; added for the
    /// Ready-effect Spell fixture below.
    ReadySummon,
    /// The Warden of Set Paths' `Rearrange` (exchange branch): swap the
    /// opposing Main Summon with a Bench Summon of the acting player's
    /// choice. The design's alternative branch — moving one opposing
    /// Benched Summon to another Bench position — is not represented; the
    /// engine has no "choose one of two effect lists" vocabulary yet, so
    /// only the exchange branch is playable.
    SwapOpposingPositions,
}

/// The first Modifier: a Passive's continuous adjustment (rules §16). `pub`,
/// not `pub(crate)`: the new card container's `Component::Passive` variant
/// names this type directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    OpposingRetreatCostDelta(i32),
}

/// The card container: `Entity`, `Component`, and the typed reads over them.
mod entity;
pub use entity::{
    AccountingId, Attack, Breakage, Component, ComponentField, ComponentKind, Entity, EntityId,
    EntityIdParseError, Life, ManaTypes, Name, Persistent, Respondable, RetreatCost, Skill, Tags,
    Trigger,
};

/// `CardSet`: the indexed collection of top-level entities.
mod set;
pub use set::CardSet;

/// The fixture entities this crate's tests build against, plus the shared
/// `CardSet` and lookups that reach them. Test-only: nothing outside
/// `#[cfg(test)]` may depend on this data's shape.
#[cfg(test)]
#[allow(clippy::expect_used)]
pub(crate) mod fixtures;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn card_kind_variants_construct() {
        let kinds = [CardKind::Summon, CardKind::Spell, CardKind::Enchantment];
        assert_eq!(kinds.len(), 3);
    }

    #[test]
    fn form_orders_base_below_enhanced_below_elite() {
        assert!(Form::Base < Form::Enhanced);
        assert!(Form::Enhanced < Form::Elite);
    }

    #[test]
    fn trigger_event_variants_construct() {
        let events = [
            TriggerEvent::YourUpkeep,
            TriggerEvent::EntersMain,
            TriggerEvent::EntersBench,
            TriggerEvent::LeavesMain,
            TriggerEvent::LeavesBench,
            TriggerEvent::AnySummonDestroyed,
        ];
        assert_eq!(events.len(), 6);
    }

    #[test]
    fn effect_condition_variants_construct() {
        let conditions = [
            EffectCondition::DefenderEnteredMainThisTurn,
            EffectCondition::SpellPlayedThisTurn,
        ];
        assert_eq!(conditions.len(), 2);
    }

    #[test]
    fn response_block_variants_construct() {
        assert_eq!(ResponseBlock::AttackSpells, ResponseBlock::AttackSpells);
    }

    #[test]
    fn cost_defaults_to_free() {
        assert_eq!(
            Cost::default(),
            Cost {
                matter: 0,
                mind: 0,
                spirit: 0,
                generic: 0,
            }
        );
    }

    #[test]
    fn every_effect_leaf_variant_constructs() {
        let leaves = vec![
            EffectLeaf::DealDamage {
                amount: 10,
                immutable: false,
            },
            EffectLeaf::Heal { amount: 10 },
            EffectLeaf::MoveSummon,
            EffectLeaf::SwapPositions,
            EffectLeaf::ConditionalBonus {
                condition: EffectCondition::SpellPlayedThisTurn,
                amount: 20,
            },
            EffectLeaf::BlockResponses {
                condition: EffectCondition::SpellPlayedThisTurn,
                block: ResponseBlock::AttackSpells,
            },
            EffectLeaf::ReturnSpellFromDiscard,
            EffectLeaf::LookAtPrizes,
            EffectLeaf::DrawCards { amount: 1 },
            EffectLeaf::ReturnSpellToDeckTop,
            EffectLeaf::ProduceMana,
            EffectLeaf::CannotBeMovedByOpponent,
            EffectLeaf::ReadySummon,
            EffectLeaf::SwapOpposingPositions,
        ];
        assert_eq!(leaves.len(), 14);
    }

    #[test]
    fn modifier_variants_construct() {
        assert_eq!(
            Modifier::OpposingRetreatCostDelta(1),
            Modifier::OpposingRetreatCostDelta(1)
        );
    }

    #[test]
    fn spell_timing_variants_construct() {
        assert_ne!(SpellTiming::Support, SpellTiming::Attack);
    }

    #[test]
    fn card_kind_covers_spell_and_enchantment_too() {
        assert_ne!(CardKind::Spell, CardKind::Enchantment);
        assert_ne!(CardKind::Summon, CardKind::Spell);
    }
}
