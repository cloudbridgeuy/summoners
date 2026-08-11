//! The fixture registry: every printed card this crate's tests and engine
//! demos build against, plus the lookup that reaches one by id.
//!
//! Split out of `cards/mod.rs` to keep both files under the file-length
//! cap; the vocabulary and the query machinery stay in the parent module,
//! and this module holds only the concrete card data. `mod.rs` re-exports
//! `registry` and `find_def` so callers keep using `crate::domain::cards::
//! find_def` unchanged.

use super::{
    CardDef, CardDefId, CardKind, CardNode, Cost, EffectLeaf, Form, SpellTiming, TriggerEvent,
};
use crate::domain::ids::ManaType;

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
        // Trigger fixtures — each a standalone Base Summon carrying exactly
        // one Trigger node, so `engine::triggers` has real discovery and
        // firing fixtures to exercise (rules §28, §36–41). Kept separate
        // from the two chains above, alongside the Skill fixtures.
        CardDef {
            id: CardDefId("hearth-warden"),
            name: "Hearth Warden",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(60),
                CardNode::Produces(vec![ManaType::Spirit]),
                CardNode::RetreatCost(1),
                CardNode::Form(Form::Base),
                CardNode::Attack {
                    cost: Cost::default(),
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                },
                CardNode::Trigger {
                    event: TriggerEvent::EntersMain,
                    respondable: false,
                    effects: vec![EffectLeaf::Heal { amount: 15 }],
                },
            ],
        },
        CardDef {
            id: CardDefId("spite-thorn"),
            name: "Spite Thorn",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(50),
                CardNode::Produces(vec![ManaType::Mind]),
                CardNode::RetreatCost(1),
                CardNode::Form(Form::Base),
                CardNode::Attack {
                    cost: Cost::default(),
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                },
                CardNode::Trigger {
                    event: TriggerEvent::AnySummonDestroyed,
                    respondable: true,
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 15,
                        immutable: false,
                    }],
                },
            ],
        },
        CardDef {
            id: CardDefId("dawn-tender"),
            name: "Dawn Tender",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(40),
                CardNode::Produces(vec![ManaType::Spirit]),
                CardNode::RetreatCost(1),
                CardNode::Form(Form::Base),
                CardNode::Attack {
                    cost: Cost::default(),
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 10,
                        immutable: false,
                    }],
                },
                CardNode::Trigger {
                    event: TriggerEvent::YourUpkeep,
                    respondable: false,
                    effects: vec![EffectLeaf::Heal { amount: 10 }],
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
