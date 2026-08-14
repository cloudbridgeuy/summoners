//! The card container (`entity`, `set`) and, alongside it, the provisional
//! card representation (design decision 6) that most of the engine still
//! reads through, by way of `shim::find_def`.
//!
//! `entity` and `set` hold the settled container: `Entity`, `Component`, and
//! `CardSet`. A `GameState` carries one `Arc<CardSet>`; every card fact a
//! rule needs comes from reading it, directly or through the shim.
//!
//! Everything below `CardDef` is the provisional tree: a card is a tree of
//! typed nodes and effect leaves, reached only through `CardDef::find`.
//! `shim::find_def` builds one on demand by projecting an `Entity`'s
//! components into it, so a call site written against `CardDef` never has to
//! know whether the fact it read came straight off an `Entity` or through
//! this tree. `CardDef`, `CardNode`, `Query`, `QueryResult`, and `find` stay
//! `pub(crate)` because no code outside this crate should depend on this
//! provisional shape. `Form`, `Cost`, `Modifier`, `TriggerEvent`, and
//! `EffectLeaf` are exceptions: the container's `Component` enum names them
//! directly, so they are `pub`.
//!
//! `fixtures` (test-only) holds the entity data every test in this crate
//! builds a `CardSet` against: the same vanilla and signature cards this
//! module used to hold as `CardDef` literals, now authored as `Entity`
//! values instead.

use crate::domain::actions::SkillIndex;
use crate::domain::ids::ManaType;

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

/// One typed fact or ability printed on a card. The vanilla registry only
/// ever builds `Life`, `Produces`, `RetreatCost`, and `Form`; the ability
/// node shapes (`Attack`, `Skill`, `Trigger`, `Passive`) are settled now so
/// later fixtures share this tree instead of growing a second one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CardNode {
    Life(u32),
    Produces(Vec<ManaType>),
    RetreatCost(u32),
    Form(Form),
    Attack {
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
    Skill {
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
    Trigger {
        event: TriggerEvent,
        respondable: bool,
        effects: Vec<EffectLeaf>,
    },
    Passive(Modifier),
    Spell {
        timing: SpellTiming,
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
    /// A vanilla Enchantment's persistent effect leaves (rules §44). No
    /// Attack, Skill, or condition text yet — the vanilla fixture below
    /// carries an empty `effects` list and stays in play doing nothing but
    /// existing, which is enough to prove casting and persistence.
    Enchantment {
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
}

/// A question `CardDef::find` can answer about one printed card. `Skill`
/// backs the printed cost and effects lookup for one of the topmost card's
/// Skill nodes, addressed by its `SkillIndex`, in `engine::skills` (rules
/// §15); `Trigger` backs the printed event, respondability, and effects
/// lookup in `engine::triggers`. More variants arrive alongside the handler
/// that first needs them, matching the rest of this crate's stubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Query {
    Skill(SkillIndex),
    Trigger,
}

/// One answer `CardDef::find` can return, matching the `Query` asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueryResult {
    Skill {
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
    Trigger {
        event: TriggerEvent,
        respondable: bool,
        effects: Vec<EffectLeaf>,
    },
}

/// One printed card, projected from an `Entity` (see `shim`): its id, its
/// display name, its family, and its nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardDef {
    pub id: entity::EntityId,
    pub name: String,
    pub kind: CardKind,
    pub nodes: Vec<CardNode>,
}

impl CardDef {
    /// The only way to read a card's characteristics. Callers never match on
    /// `nodes` directly, so the tree's shape can change later without
    /// touching call sites.
    pub(crate) fn find(&self, query: Query) -> Option<QueryResult> {
        // `Skill` is addressed by index rather than by node shape (a card may
        // print more than one), so it is answered separately from the
        // one-node-per-query lookups below: the `index`-th `Skill` node
        // found while walking `nodes` in print order (rules §15's "the order
        // `CardDef::find` discovers them on its topmost card").
        if let Query::Skill(SkillIndex(index)) = query {
            return self
                .nodes
                .iter()
                .filter_map(|node| match node {
                    CardNode::Skill { cost, effects } => Some(QueryResult::Skill {
                        cost: *cost,
                        effects: effects.clone(),
                    }),
                    _ => None,
                })
                .nth(index);
        }
        self.nodes.iter().find_map(|node| match (query, node) {
            (
                Query::Trigger,
                CardNode::Trigger {
                    event,
                    respondable,
                    effects,
                },
            ) => Some(QueryResult::Trigger {
                event: *event,
                respondable: *respondable,
                effects: effects.clone(),
            }),
            _ => None,
        })
    }
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

/// A temporary reader that projects one `Entity` from a `CardSet` into the
/// `CardDef` tree above, so every call site that already reads a `CardDef`
/// keeps working unchanged while it moves onto the container at its own
/// pace.
mod shim;
pub(crate) use shim::find_def;

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
    fn every_card_node_variant_constructs() {
        let nodes = [
            CardNode::Life(10),
            CardNode::Produces(vec![ManaType::Matter]),
            CardNode::RetreatCost(1),
            CardNode::Form(Form::Base),
            CardNode::Attack {
                cost: Cost::default(),
                effects: vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            },
            CardNode::Skill {
                cost: Cost::default(),
                effects: vec![EffectLeaf::Heal { amount: 10 }],
            },
            CardNode::Trigger {
                event: TriggerEvent::YourUpkeep,
                respondable: false,
                effects: vec![EffectLeaf::Heal { amount: 10 }],
            },
            CardNode::Passive(Modifier::OpposingRetreatCostDelta(1)),
            CardNode::Spell {
                timing: SpellTiming::Support,
                cost: Cost::default(),
                effects: vec![EffectLeaf::Heal { amount: 10 }],
            },
            CardNode::Enchantment {
                cost: Cost::default(),
                effects: vec![],
            },
        ];
        assert_eq!(nodes.len(), 10);
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

    /// `find` never matches a query against a node this `CardDef` does not
    /// carry — the projection shim's own tests cover reading real fixture
    /// data; this proves the fallback stays `None` on a bare, hand-built
    /// def, independent of any entity.
    #[test]
    fn find_returns_none_for_an_absent_node() {
        let bare = CardDef {
            id: EntityId::parse(&"0".repeat(32)).expect("valid fixture id"),
            name: "Bare".to_string(),
            kind: CardKind::Summon,
            nodes: vec![],
        };
        assert_eq!(bare.find(Query::Trigger), None);
    }
}
