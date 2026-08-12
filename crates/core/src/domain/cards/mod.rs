//! The provisional card representation (design decision 6).
//!
//! A card is a tree of typed nodes and effect leaves, reached only through
//! `CardDef::find`. The final Card container is a separate, future design;
//! when it lands, `CardDef` and `find` change but the engine keeps this
//! query surface. Everything below `CardDefId` stays `pub(crate)` because no
//! code outside this crate should depend on today's shape.
//!
//! The fixture registry in `registry.rs` is a minimal set of vanilla
//! Summons — `Life`, `Produces`, `RetreatCost`, `Form`, and a plain `Attack`
//! node only — sufficient for `scenario::from_scenario`'s chain-order and board
//! tests, and for a normal attack to have a cost and a Damage amount to
//! pay and apply. It also carries four vanilla Spells — one Attack Spell,
//! one Support Spell that heals, one Support Spell that draws, and one
//! Support Spell that readies an Exhausted Summon (rules §53) — enough for
//! `engine::stack::cast_spell` and `engine::resolution` to have a real cost,
//! timing family, and effect leaf to pay, gate, and resolve. Three of the
//! Summon fixtures each print one Skill (`MoveSummon`, `SwapPositions`, or
//! `ProduceMana`), enough for `engine::skills::activate_skill` to pay, gate,
//! and resolve a Skill through the same interpreter (rules §15, §43). Two
//! further Summons each carry one `CardNode::Trigger`: Hearth Warden fires an
//! immediate Heal on entering Main (rules §36, §39), and Spite Thorn fires a
//! respondable Damage trigger whenever any Summon is destroyed (rules §37,
//! §41) — enough for `engine::triggers` to have one non-respondable and one
//! respondable fixture to discover and resolve. A third, Dawn Tender, fires
//! an immediate Heal at the start of its controller's own Upkeep (rules
//! §36), exercising the same discovery path `engine::turn::handover` queues
//! every turn. A fourth Spell carries a Ready effect (rules §53). The
//! registry also carries the four signature cards from
//! `designs/types_archetypes.md` (Colossus of the Quarry, Warden of Set
//! Paths, Griefsinger, Old Sow of the Barrow), each atop its own
//! Base/Enhanced/Elite fixture lineage, and the vanilla Enchantment (design
//! decision 1). Stats and names are test data, not final card designs
//! (decision 9).

use crate::domain::actions::SkillIndex;
use crate::domain::ids::ManaType;

/// A stable key for one printed card in the fixture registry. Scenarios
/// reference cards by this id; it is public because `Scenario` is public.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardDefId(pub &'static str);

/// The three card families (rules §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
/// a Skill's cost may be free (rules §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Cost {
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

/// The first Modifier: a Passive's continuous adjustment (rules §16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    Trigger,
    /// Rules §16: a printed Passive's continuous `Modifier`, read by
    /// `engine::board::opposing_retreat_cost_delta` when it scans the
    /// opponent's in-play card trees for `Modifier::OpposingRetreatCostDelta`.
    Passive,
    /// Rules §44: a printed Enchantment's cost and persistent effect leaves.
    Enchantment,
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
    Trigger {
        event: TriggerEvent,
        respondable: bool,
        effects: Vec<EffectLeaf>,
    },
    Passive(Modifier),
    Enchantment {
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
            (Query::Passive, CardNode::Passive(modifier)) => Some(QueryResult::Passive(*modifier)),
            (Query::Enchantment, CardNode::Enchantment { cost, effects }) => {
                Some(QueryResult::Enchantment {
                    cost: *cost,
                    effects: effects.clone(),
                })
            }
            _ => None,
        })
    }
}

/// The concrete fixture data: every printed `CardDef` this crate builds
/// against, and the lookup that reaches one by id. Kept in its own module so
/// this file stays the vocabulary and the query machinery only.
mod registry;
pub(crate) use registry::find_def;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::registry::registry;
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
    fn find_reads_the_trigger_event_respondability_and_effects() {
        let hearth_warden = find_def(CardDefId("hearth-warden")).expect("fixture exists");
        assert_eq!(
            hearth_warden.find(Query::Trigger),
            Some(QueryResult::Trigger {
                event: TriggerEvent::EntersMain,
                respondable: false,
                effects: vec![EffectLeaf::Heal { amount: 15 }],
            })
        );

        let spite_thorn = find_def(CardDefId("spite-thorn")).expect("fixture exists");
        assert_eq!(
            spite_thorn.find(Query::Trigger),
            Some(QueryResult::Trigger {
                event: TriggerEvent::AnySummonDestroyed,
                respondable: true,
                effects: vec![EffectLeaf::DealDamage {
                    amount: 15,
                    immutable: false,
                }],
            })
        );

        let whelp = find_def(CardDefId("quarry-whelp")).expect("fixture exists");
        assert_eq!(whelp.find(Query::Trigger), None);
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
        // Five original chain-fixture Summons, plus the three
        // single-Skill fixtures (Quarry Scout, Quarry Warden-Guard, Quarry
        // Well-Tender), plus the three single-Trigger fixtures (Hearth
        // Warden, Spite Thorn, Dawn Tender), plus three new three-step
        // signature chains (Warden of Set Paths, Griefsinger, Old Sow of
        // the Barrow) at three Summons each.
        assert_eq!(summons.count(), 20);
    }

    #[test]
    fn registry_holds_the_four_vanilla_spells() {
        let defs = registry();
        let spells = defs.iter().filter(|def| def.kind == CardKind::Spell);
        assert_eq!(spells.count(), 4);
    }

    #[test]
    fn registry_holds_the_one_vanilla_enchantment() {
        let defs = registry();
        let enchantments = defs.iter().filter(|def| def.kind == CardKind::Enchantment);
        assert_eq!(enchantments.count(), 1);
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

        let warden_chain = [
            "warden-initiate",
            "warden-pathkeeper",
            "warden-of-set-paths",
        ];
        let forms: Vec<Form> = warden_chain.iter().map(|id| form_of(id)).collect();
        assert_eq!(forms, vec![Form::Base, Form::Enhanced, Form::Elite]);

        let griefsinger_chain = ["griefsinger-wisp", "griefsinger-mourner", "griefsinger"];
        let forms: Vec<Form> = griefsinger_chain.iter().map(|id| form_of(id)).collect();
        assert_eq!(forms, vec![Form::Base, Form::Enhanced, Form::Elite]);

        let sow_chain = ["sow-piglet", "sow-matriarch", "old-sow-of-the-barrow"];
        let forms: Vec<Form> = sow_chain.iter().map(|id| form_of(id)).collect();
        assert_eq!(forms, vec![Form::Base, Form::Enhanced, Form::Elite]);
    }

    #[test]
    fn find_reads_the_passive_modifier() {
        let warden = find_def(CardDefId("warden-of-set-paths")).expect("fixture exists");
        assert_eq!(
            warden.find(Query::Passive),
            Some(QueryResult::Passive(Modifier::OpposingRetreatCostDelta(1)))
        );

        let whelp = find_def(CardDefId("quarry-whelp")).expect("fixture exists");
        assert_eq!(whelp.find(Query::Passive), None);
    }

    #[test]
    fn find_reads_the_enchantment_cost_and_effects() {
        let ward = find_def(CardDefId("standing-ward")).expect("fixture exists");
        assert_eq!(
            ward.find(Query::Enchantment),
            Some(QueryResult::Enchantment {
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![],
            })
        );
    }
}
