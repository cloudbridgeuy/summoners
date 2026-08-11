//! The provisional card representation (design decision 6).
//!
//! A card is a tree of typed nodes and effect leaves, reached only through
//! `CardDef::find`. The final Card container is a separate, future design;
//! when it lands, `CardDef` and `find` change but the engine keeps this
//! query surface. Everything below `CardDefId` stays `pub(crate)` because no
//! code outside this crate should depend on today's shape.
//!
//! The fixture registry at the bottom is a minimal set of vanilla Summons —
//! `Life`, `Produces`, `RetreatCost`, `Form`, and a plain `Attack` node
//! only — sufficient for `scenario::from_scenario`'s chain-order and board
//! tests, and for a normal attack to have a cost and a Damage amount to
//! pay and apply. It also carries four vanilla Spells — one Attack Spell,
//! one Support Spell that heals, one Support Spell that draws, and one
//! Support Spell that readies an Exhausted Summon (rules §53) — enough for
//! `engine::stack::cast_spell` and `engine::resolution` to have a real cost,
//! timing family, and effect leaf to pay, gate, and resolve. Three of the
//! Summon fixtures each print one Skill (`MoveSummon`, `SwapPositions`, or
//! `ProduceMana`), enough for `engine::skills::activate_skill` to pay, gate,
//! and resolve a Skill through the same interpreter (rules §15, §43). The four
//! signature cards from `designs/types_archetypes.md` (Colossus of the
//! Quarry, Warden of Set Paths, Griefsinger, Old Sow of the Barrow), their
//! Base/Enhanced fixture lineage, and the vanilla Enchantment (design
//! decision 1) arrive with the work that first gives them abilities to
//! test; this module already settles the vocabulary those fixtures will
//! use (`EffectLeaf`, `CardNode`, `Modifier`), so a few variants below have
//! no production caller yet. Stats and names are test data, not final card
//! designs (decision 9).

use crate::domain::actions::SkillIndex;
use crate::domain::ids::ManaType;

/// A stable key for one printed card in the fixture registry. Scenarios
/// reference cards by this id; it is public because `Scenario` is public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardDefId(pub &'static str);

/// The three card families (rules §3). The registry below is Summons only;
/// `Spell` and `Enchantment` stay unconstructed until fixtures of those
/// kinds land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum CardKind {
    Summon,
    Spell,
    Enchantment,
}

/// A Summon's place in its upgrade chain (rules §20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Form {
    Base,
    Enhanced,
    Elite,
}

/// The two Spell timing families (rules §34). A Support Spell may be cast
/// proactively during its controller's own resting Main Phase or as a legal
/// response; an Attack Spell is tied to Combat and may only be cast as a
/// response while its caster holds Priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpellTiming {
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

/// A condition an effect leaf can test before applying a bonus. No fixture
/// in the vanilla registry carries a conditional effect yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum EffectCondition {
    DefenderEnteredMainThisTurn,
    SpellPlayedThisTurn,
}

/// The family of response an effect can block. No fixture in the vanilla
/// registry blocks a response yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ResponseBlock {
    AttackSpells,
}

/// A typed plus Generic Mana cost (rules §12). Every component may be zero;
/// a Skill's cost may be free (rules §15). No vanilla fixture carries a
/// costed node yet, so this struct has no production caller until one does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
pub(crate) struct Cost {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
    pub generic: u32,
}

/// The first fixed set of effect leaves, one per signature card ability
/// (design decision 1). Shapes here are provisional. None has a production
/// caller yet: the vanilla registry has no costed node to carry one, and
/// the interpreter that runs them belongs to later work.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum EffectLeaf {
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
    BlockResponses(ResponseBlock),
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
}

/// The first Modifier: a Passive's continuous adjustment (rules §16). No
/// vanilla fixture carries a Passive node yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Modifier {
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
    #[allow(dead_code)]
    Trigger {
        event: TriggerEvent,
        respondable: bool,
        effects: Vec<EffectLeaf>,
    },
    #[allow(dead_code)]
    Passive(Modifier),
    Spell {
        timing: SpellTiming,
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
}

/// A question `CardDef::find` can answer about one printed card. `Attack`
/// backs the normal-attack cost and Damage lookup in `engine::stack` and
/// `engine::resolution` (rules §29–30); `CurrentForm` backs chain-order
/// validation in `scenario::from_scenario`; `Life` backs the rules §23
/// destruction threshold check in `engine::destruction`; `ProducedManaTypes`
/// backs the rules §18 Mana-Type-superset check in `engine::board`;
/// `RetreatCost` backs the printed Retreat Cost lookup there too; `Skill`
/// backs the printed cost and effects lookup for one of the topmost card's
/// Skill nodes, addressed by its `SkillIndex`, in `engine::skills` (rules
/// §15); `Spell` backs the printed timing family, cost, and effects lookup in
/// `engine::stack` and `engine::resolution` (rules §34). More variants
/// arrive alongside the handler that first needs them, matching the rest of
/// this crate's stubs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Query {
    Attack,
    CurrentForm,
    Life,
    ProducedManaTypes,
    RetreatCost,
    Skill(SkillIndex),
    Spell,
}

/// One answer `CardDef::find` can return, matching the `Query` asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum QueryResult {
    Attack {
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
    CurrentForm(Form),
    Life(u32),
    ProducedManaTypes(Vec<ManaType>),
    RetreatCost(u32),
    Skill {
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
    Spell {
        timing: SpellTiming,
        cost: Cost,
        effects: Vec<EffectLeaf>,
    },
}

/// One printed card: an id, a display name, its family, and its nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CardDef {
    pub id: CardDefId,
    pub name: &'static str,
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
            (Query::Attack, CardNode::Attack { cost, effects }) => Some(QueryResult::Attack {
                cost: *cost,
                effects: effects.clone(),
            }),
            (Query::CurrentForm, CardNode::Form(form)) => Some(QueryResult::CurrentForm(*form)),
            (Query::Life, CardNode::Life(life)) => Some(QueryResult::Life(*life)),
            (Query::ProducedManaTypes, CardNode::Produces(types)) => {
                Some(QueryResult::ProducedManaTypes(types.clone()))
            }
            (Query::RetreatCost, CardNode::RetreatCost(cost)) => {
                Some(QueryResult::RetreatCost(*cost))
            }
            (
                Query::Spell,
                CardNode::Spell {
                    timing,
                    cost,
                    effects,
                },
            ) => Some(QueryResult::Spell {
                timing: *timing,
                cost: *cost,
                effects: effects.clone(),
            }),
            _ => None,
        })
    }
}

/// A minimal registry of vanilla Summons — enough for `from_scenario`'s
/// board and chain-order tests, no more. One three-step chain and one
/// two-step chain, so tests can exercise both a full and a partial climb.
/// Fixture stats and names are test data, not final card designs.
pub(crate) fn registry() -> Vec<CardDef> {
    vec![
        // Chain 1 — three steps.
        CardDef {
            id: CardDefId("quarry-whelp"),
            name: "Quarry Whelp",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(40),
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
            ],
        },
        CardDef {
            id: CardDefId("quarry-brute"),
            name: "Quarry Brute",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(90),
                CardNode::Produces(vec![ManaType::Matter]),
                CardNode::RetreatCost(2),
                CardNode::Form(Form::Enhanced),
                CardNode::Attack {
                    cost: Cost {
                        generic: 1,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 20,
                        immutable: false,
                    }],
                },
            ],
        },
        CardDef {
            id: CardDefId("colossus-of-the-quarry"),
            name: "Colossus of the Quarry",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(180),
                CardNode::Produces(vec![ManaType::Matter]),
                CardNode::RetreatCost(3),
                CardNode::Form(Form::Elite),
                CardNode::Attack {
                    cost: Cost {
                        generic: 2,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 40,
                        immutable: false,
                    }],
                },
            ],
        },
        // Chain 2 — two steps.
        CardDef {
            id: CardDefId("set-path-adept"),
            name: "Set-Path Adept",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(50),
                CardNode::Produces(vec![ManaType::Matter, ManaType::Mind]),
                CardNode::RetreatCost(2),
                CardNode::Form(Form::Base),
                CardNode::Attack {
                    cost: Cost::default(),
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                },
            ],
        },
        CardDef {
            id: CardDefId("set-path-warden"),
            name: "Set-Path Warden",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(100),
                CardNode::Produces(vec![ManaType::Matter, ManaType::Mind]),
                CardNode::RetreatCost(2),
                CardNode::Form(Form::Enhanced),
                CardNode::Attack {
                    cost: Cost {
                        generic: 1,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 20,
                        immutable: false,
                    }],
                },
            ],
        },
        // Skill fixtures — each a standalone Base Summon carrying exactly
        // one Skill node, so `engine::skills::activate_skill` has a real
        // cost, Ready gate, and effect leaf to pay, exhaust, and resolve
        // (rules §15). Kept separate from the two chains above rather than
        // added to them.
        CardDef {
            id: CardDefId("quarry-scout"),
            name: "Quarry Scout",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(30),
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
                    cost: Cost {
                        generic: 1,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::MoveSummon],
                },
            ],
        },
        CardDef {
            id: CardDefId("quarry-warden-guard"),
            name: "Quarry Warden-Guard",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(30),
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
                    effects: vec![EffectLeaf::SwapPositions],
                },
            ],
        },
        CardDef {
            id: CardDefId("quarry-well-tender"),
            name: "Quarry Well-Tender",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(30),
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
                    effects: vec![EffectLeaf::ProduceMana],
                },
            ],
        },
        // Spells.
        CardDef {
            id: CardDefId("ember-lance"),
            name: "Ember Lance",
            kind: CardKind::Spell,
            nodes: vec![CardNode::Spell {
                timing: SpellTiming::Attack,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            }],
        },
        CardDef {
            id: CardDefId("renewing-balm"),
            name: "Renewing Balm",
            kind: CardKind::Spell,
            nodes: vec![CardNode::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::Heal { amount: 20 }],
            }],
        },
        CardDef {
            id: CardDefId("scrying-glass"),
            name: "Scrying Glass",
            kind: CardKind::Spell,
            nodes: vec![CardNode::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::DrawCards { amount: 1 }],
            }],
        },
        CardDef {
            id: CardDefId("second-wind"),
            name: "Second Wind",
            kind: CardKind::Spell,
            nodes: vec![CardNode::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::ReadySummon],
            }],
        },
    ]
}

/// Look up one fixture by id.
pub(crate) fn find_def(id: CardDefId) -> Option<CardDef> {
    registry().into_iter().find(|def| def.id == id)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn card_def_id_constructs_and_compares() {
        assert_eq!(CardDefId("a"), CardDefId("a"));
        assert_ne!(CardDefId("a"), CardDefId("b"));
    }

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
            EffectLeaf::BlockResponses(ResponseBlock::AttackSpells),
            EffectLeaf::ReturnSpellFromDiscard,
            EffectLeaf::LookAtPrizes,
            EffectLeaf::DrawCards { amount: 1 },
            EffectLeaf::ReturnSpellToDeckTop,
            EffectLeaf::ProduceMana,
            EffectLeaf::CannotBeMovedByOpponent,
            EffectLeaf::ReadySummon,
        ];
        assert_eq!(leaves.len(), 13);
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
        ];
        assert_eq!(nodes.len(), 9);
    }

    #[test]
    fn spell_timing_variants_construct() {
        assert_ne!(SpellTiming::Support, SpellTiming::Attack);
    }

    #[test]
    fn card_kind_covers_spell_and_enchantment_too() {
        // The registry below is Summons only; this test is the only
        // production-adjacent proof that `Spell` and `Enchantment` still
        // construct and compare correctly.
        assert_ne!(CardKind::Spell, CardKind::Enchantment);
        assert_ne!(CardKind::Summon, CardKind::Spell);
    }

    #[test]
    fn find_reads_only_the_current_form() {
        let whelp = find_def(CardDefId("quarry-whelp")).expect("fixture exists");
        assert_eq!(
            whelp.find(Query::CurrentForm),
            Some(QueryResult::CurrentForm(Form::Base))
        );
    }

    #[test]
    fn find_reads_the_produced_mana_types() {
        let whelp = find_def(CardDefId("quarry-whelp")).expect("fixture exists");
        assert_eq!(
            whelp.find(Query::ProducedManaTypes),
            Some(QueryResult::ProducedManaTypes(vec![ManaType::Matter]))
        );

        let adept = find_def(CardDefId("set-path-adept")).expect("fixture exists");
        assert_eq!(
            adept.find(Query::ProducedManaTypes),
            Some(QueryResult::ProducedManaTypes(vec![
                ManaType::Matter,
                ManaType::Mind
            ]))
        );
    }

    #[test]
    fn find_reads_the_life() {
        let whelp = find_def(CardDefId("quarry-whelp")).expect("fixture exists");
        assert_eq!(whelp.find(Query::Life), Some(QueryResult::Life(40)));
    }

    #[test]
    fn find_reads_the_retreat_cost() {
        let brute = find_def(CardDefId("quarry-brute")).expect("fixture exists");
        assert_eq!(
            brute.find(Query::RetreatCost),
            Some(QueryResult::RetreatCost(2))
        );
    }

    #[test]
    fn find_returns_none_for_an_absent_node() {
        let bare = CardDef {
            id: CardDefId("bare"),
            name: "Bare",
            kind: CardKind::Summon,
            nodes: vec![],
        };
        assert_eq!(bare.find(Query::CurrentForm), None);
    }

    #[test]
    fn find_def_locates_a_registry_fixture_and_rejects_an_unknown_id() {
        assert!(find_def(CardDefId("quarry-whelp")).is_some());
        assert!(find_def(CardDefId("does-not-exist")).is_none());
    }

    #[test]
    fn registry_holds_a_two_step_and_a_three_step_chain() {
        let defs = registry();
        let summons = defs.iter().filter(|def| def.kind == CardKind::Summon);
        assert_eq!(summons.count(), 8);
    }

    #[test]
    fn registry_holds_the_four_vanilla_spells() {
        let defs = registry();
        let spells = defs.iter().filter(|def| def.kind == CardKind::Spell);
        assert_eq!(spells.count(), 4);
    }

    #[test]
    fn find_reads_one_skill_node_by_index() {
        let scout = find_def(CardDefId("quarry-scout")).expect("fixture exists");
        assert_eq!(
            scout.find(Query::Skill(SkillIndex(0))),
            Some(QueryResult::Skill {
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::MoveSummon],
            })
        );
        assert_eq!(scout.find(Query::Skill(SkillIndex(1))), None);

        let warden_guard = find_def(CardDefId("quarry-warden-guard")).expect("fixture exists");
        assert_eq!(
            warden_guard.find(Query::Skill(SkillIndex(0))),
            Some(QueryResult::Skill {
                cost: Cost::default(),
                effects: vec![EffectLeaf::SwapPositions],
            })
        );

        let well_tender = find_def(CardDefId("quarry-well-tender")).expect("fixture exists");
        assert_eq!(
            well_tender.find(Query::Skill(SkillIndex(0))),
            Some(QueryResult::Skill {
                cost: Cost::default(),
                effects: vec![EffectLeaf::ProduceMana],
            })
        );
    }

    #[test]
    fn find_reads_the_ready_effect_spell() {
        let second_wind = find_def(CardDefId("second-wind")).expect("fixture exists");
        assert_eq!(
            second_wind.find(Query::Spell),
            Some(QueryResult::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::ReadySummon],
            })
        );
    }

    #[test]
    fn find_reads_the_spell_timing_cost_and_effects() {
        let ember_lance = find_def(CardDefId("ember-lance")).expect("fixture exists");
        assert_eq!(
            ember_lance.find(Query::Spell),
            Some(QueryResult::Spell {
                timing: SpellTiming::Attack,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::DealDamage {
                    amount: 10,
                    immutable: false,
                }],
            })
        );

        let renewing_balm = find_def(CardDefId("renewing-balm")).expect("fixture exists");
        assert_eq!(
            renewing_balm.find(Query::Spell),
            Some(QueryResult::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::Heal { amount: 20 }],
            })
        );

        let scrying_glass = find_def(CardDefId("scrying-glass")).expect("fixture exists");
        assert_eq!(
            scrying_glass.find(Query::Spell),
            Some(QueryResult::Spell {
                timing: SpellTiming::Support,
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![EffectLeaf::DrawCards { amount: 1 }],
            })
        );
    }

    #[test]
    fn every_summon_chain_climbs_base_enhanced_elite() {
        fn form_of(id: &'static str) -> Form {
            let def = find_def(CardDefId(id)).expect("fixture exists");
            let Some(QueryResult::CurrentForm(form)) = def.find(Query::CurrentForm) else {
                panic!("every registry Summon fixture carries a Form node");
            };
            form
        }

        let three_step = ["quarry-whelp", "quarry-brute", "colossus-of-the-quarry"];
        let forms: Vec<Form> = three_step.iter().map(|id| form_of(id)).collect();
        assert_eq!(forms, vec![Form::Base, Form::Enhanced, Form::Elite]);

        let two_step = ["set-path-adept", "set-path-warden"];
        let forms: Vec<Form> = two_step.iter().map(|id| form_of(id)).collect();
        assert_eq!(forms, vec![Form::Base, Form::Enhanced]);
    }
}
