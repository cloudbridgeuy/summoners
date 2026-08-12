//! The fixture registry: every printed card this crate's tests and engine
//! demos build against, plus the lookup that reaches one by id.
//!
//! Split out of `cards/mod.rs` to keep both files under the file-length
//! cap; the vocabulary and the query machinery stay in the parent module,
//! and this module holds only the concrete card data. `mod.rs` re-exports
//! `registry` and `find_def` so callers keep using `crate::domain::cards::
//! find_def` unchanged.

use super::{
    CardDef, CardDefId, CardKind, CardNode, Cost, EffectCondition, EffectLeaf, Form, Modifier,
    ResponseBlock, SpellTiming, TriggerEvent,
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
                // Matter + Matter + 1 Generic: 90 Damage, no text
                // (`designs/types_archetypes.md` §3, the Matter benchmark).
                CardNode::Attack {
                    cost: Cost {
                        matter: 2,
                        generic: 1,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 90,
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
        // Signature chains — one three-step Base/Enhanced/Elite chain per
        // `designs/types_archetypes.md` §5 pair, climbing from a mono-type
        // Base to a dual-type Elite under the Mana-Type-superset upgrade
        // rule (rules §18). Base and Enhanced stats are test data invented
        // to fill the chain beneath each printed Elite; only the Elite
        // stats and text are drawn from the design document.
        //
        // Matter/Mind — the Warden of Set Paths.
        CardDef {
            id: CardDefId("warden-initiate"),
            name: "Warden Initiate",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(50),
                CardNode::Produces(vec![ManaType::Matter]),
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
            id: CardDefId("warden-pathkeeper"),
            name: "Warden Pathkeeper",
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
        CardDef {
            id: CardDefId("warden-of-set-paths"),
            name: "Warden of Set Paths",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(150),
                CardNode::Produces(vec![ManaType::Matter, ManaType::Mind]),
                CardNode::RetreatCost(3),
                CardNode::Form(Form::Elite),
                // Skill (Matter + Mind): Rearrange — exchange the opposing
                // Main with a Benched Summon of the acting player's choice.
                // The design's alternative branch (moving one opposing
                // Benched Summon to another Bench position) is not
                // represented; see `EffectLeaf::SwapOpposingPositions`.
                CardNode::Skill {
                    cost: Cost {
                        matter: 1,
                        mind: 1,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::SwapOpposingPositions],
                },
                // Passive: opposing Retreats cost 1 more.
                CardNode::Passive(Modifier::OpposingRetreatCostDelta(1)),
                // Attack (Matter + 2 Generic): 50 Damage, +30 if the
                // defending Summon entered Main this turn.
                CardNode::Attack {
                    cost: Cost {
                        matter: 1,
                        generic: 2,
                        ..Cost::default()
                    },
                    effects: vec![
                        EffectLeaf::DealDamage {
                            amount: 50,
                            immutable: false,
                        },
                        EffectLeaf::ConditionalBonus {
                            condition: EffectCondition::DefenderEnteredMainThisTurn,
                            amount: 30,
                        },
                    ],
                },
            ],
        },
        // Mind/Spirit — the Griefsinger.
        CardDef {
            id: CardDefId("griefsinger-wisp"),
            name: "Griefsinger Wisp",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(40),
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
            ],
        },
        CardDef {
            id: CardDefId("griefsinger-mourner"),
            name: "Griefsinger Mourner",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(80),
                CardNode::Produces(vec![ManaType::Mind, ManaType::Spirit]),
                CardNode::RetreatCost(1),
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
            id: CardDefId("griefsinger"),
            name: "Griefsinger",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(110),
                CardNode::Produces(vec![ManaType::Mind, ManaType::Spirit]),
                CardNode::RetreatCost(1),
                CardNode::Form(Form::Elite),
                // Trigger (Stack): when any Summon is destroyed, return one
                // Spell from the discard pile to hand. The design's "you
                // may" is simplified to an unconditional return; the engine
                // has no optional-sub-effect vocabulary yet, matching the
                // same simplification already made for other "may" text in
                // this registry.
                CardNode::Trigger {
                    event: TriggerEvent::AnySummonDestroyed,
                    respondable: true,
                    effects: vec![EffectLeaf::ReturnSpellFromDiscard],
                },
                // Skill (Mind + Spirit): Foresee — look at Prizes, draw a
                // card, then return one Spell from hand to the deck top.
                // The design's "you may return" is likewise simplified to
                // unconditional.
                CardNode::Skill {
                    cost: Cost {
                        mind: 1,
                        spirit: 1,
                        ..Cost::default()
                    },
                    effects: vec![
                        EffectLeaf::LookAtPrizes,
                        EffectLeaf::DrawCards { amount: 1 },
                        EffectLeaf::ReturnSpellToDeckTop,
                    ],
                },
                // Attack (Mind + Spirit + 1): 40 Damage; if a Spell was
                // played this turn, +40 and this attack cannot be
                // responded to by Attack Spells.
                CardNode::Attack {
                    cost: Cost {
                        mind: 1,
                        spirit: 1,
                        generic: 1,
                        ..Cost::default()
                    },
                    effects: vec![
                        EffectLeaf::DealDamage {
                            amount: 40,
                            immutable: false,
                        },
                        EffectLeaf::ConditionalBonus {
                            condition: EffectCondition::SpellPlayedThisTurn,
                            amount: 40,
                        },
                        EffectLeaf::BlockResponses {
                            condition: EffectCondition::SpellPlayedThisTurn,
                            block: ResponseBlock::AttackSpells,
                        },
                    ],
                },
            ],
        },
        // Matter/Spirit — the Old Sow of the Barrow.
        CardDef {
            id: CardDefId("sow-piglet"),
            name: "Sow Piglet",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(50),
                CardNode::Produces(vec![ManaType::Spirit]),
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
            id: CardDefId("sow-matriarch"),
            name: "Sow Matriarch",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(110),
                CardNode::Produces(vec![ManaType::Matter, ManaType::Spirit]),
                CardNode::RetreatCost(3),
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
            id: CardDefId("old-sow-of-the-barrow"),
            name: "Old Sow of the Barrow",
            kind: CardKind::Summon,
            nodes: vec![
                CardNode::Life(170),
                CardNode::Produces(vec![ManaType::Matter, ManaType::Spirit]),
                CardNode::RetreatCost(4),
                CardNode::Form(Form::Elite),
                // Passive: during your Upkeep, heal 10 Damage from this
                // Summon. Mechanically identical to the engine's existing
                // `YourUpkeep` Trigger fixture (Dawn Tender), so it is
                // represented the same way rather than as a `Modifier`: the
                // `Modifier` vocabulary has no "heal on a schedule" shape,
                // and a non-respondable Trigger already produces the exact
                // observable behavior the design's "Passive" text asks for.
                CardNode::Trigger {
                    event: TriggerEvent::YourUpkeep,
                    respondable: false,
                    effects: vec![EffectLeaf::Heal { amount: 10 }],
                },
                // Skill (Matter + Spirit): Root and Renew — heal 30 Damage
                // from this Summon; it cannot be moved out of Main by
                // opposing effects until the controller's next turn.
                CardNode::Skill {
                    cost: Cost {
                        matter: 1,
                        spirit: 1,
                        ..Cost::default()
                    },
                    effects: vec![
                        EffectLeaf::Heal { amount: 30 },
                        EffectLeaf::CannotBeMovedByOpponent,
                    ],
                },
                // Attack (Matter + Spirit + 2): 70 Damage. This damage
                // cannot be increased and cannot be prevented.
                CardNode::Attack {
                    cost: Cost {
                        matter: 1,
                        spirit: 1,
                        generic: 2,
                        ..Cost::default()
                    },
                    effects: vec![EffectLeaf::DealDamage {
                        amount: 70,
                        immutable: true,
                    }],
                },
            ],
        },
        // The vanilla Enchantment (design decision 1): no Attack, Skill, or
        // Passive text, an empty effects list. Enough to prove
        // `stack::cast_spell` accepts `CardKind::Enchantment` and
        // `resolution::resolve_spell` routes it to `PlayerState::enchantments`
        // instead of the discard pile (rules §44).
        CardDef {
            id: CardDefId("standing-ward"),
            name: "Standing Ward",
            kind: CardKind::Enchantment,
            nodes: vec![CardNode::Enchantment {
                cost: Cost {
                    generic: 1,
                    ..Cost::default()
                },
                effects: vec![],
            }],
        },
    ]
}

/// Look up one fixture by id.
pub(crate) fn find_def(id: CardDefId) -> Option<CardDef> {
    registry().into_iter().find(|def| def.id == id)
}
