#![allow(clippy::expect_used)]

mod support;

use summoners_cards::built_in_catalog;
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::{Attack, DamageConstraints, Skill, Trigger, TriggerEvent},
        events::{
            BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource,
            DamageStage, GameEvent,
        },
        ids::{BenchSlot, ManaType, PlayerId, Position},
        state::{
            Coin, GameOutcome, GameStatus, LossReason, ManaBank, ManaSource, PendingInput, Phase,
            Readiness, StackItem, StackWindow, UpgradeActivity,
        },
    },
    engine::apply::apply,
    scenario::ScenarioSummon,
};

use support::{PhysicalCards, player, state_with_coin};

#[test]
fn g1_economy_and_development_reaches_no_promotion_loss_through_public_actions() {
    let catalog = built_in_catalog().expect("the built-in catalog is valid");
    let mut physical = PhysicalCards::new(catalog.library(), 1);
    let barrow_starter = physical.one("foundations/sow-piglet");
    let set_paths_starter = physical.one("foundations/warden-initiate");
    let set_paths = physical.deck(catalog.set_paths());
    let barrow_herd = physical.deck(catalog.barrow_herd());
    assert_eq!(barrow_starter.def, catalog.barrow_herd().starter());
    assert_eq!(set_paths_starter.def, catalog.set_paths().starter());

    // Use the physical cards expanded from the real Deck recipes. Their
    // positions follow each Deck document's authored order.
    let pathkeeper = set_paths[0];
    let quarry_scout = set_paths[4];
    let well_tender = set_paths[8];
    let second_wind = set_paths[16];
    let set_paths_draws = vec![set_paths[14], set_paths[15], set_paths[18]];
    let barrow_draws = vec![barrow_herd[0], barrow_herd[1], barrow_herd[2]];

    let mut one = player(ScenarioSummon {
        chain: vec![barrow_starter],
        damage: 40,
        readiness: Readiness::Ready,
    });
    one.deck = barrow_draws;

    let mut two = player(ScenarioSummon {
        chain: vec![set_paths_starter],
        damage: 0,
        readiness: Readiness::Ready,
    });
    two.deck = set_paths_draws;
    two.hand = vec![well_tender, quarry_scout, second_wind, pathkeeper];
    let mut game = state_with_coin(&catalog, one, two, PlayerId::Two, Some(Coin))
        .expect("the G1 state is valid");
    assert_eq!(catalog.set_paths().body().len(), 20);
    assert_eq!(catalog.barrow_herd().body().len(), 20);

    let coin = apply(
        &game,
        &GameAction::ConvertCoin {
            player: PlayerId::Two,
            mana_type: ManaType::Matter,
        },
    )
    .expect("the second player can convert the Coin to an anchored Type");
    assert_eq!(
        coin.events,
        vec![GameEvent::CoinConverted {
            player: PlayerId::Two,
            mana_type: ManaType::Matter,
        }]
    );
    assert_eq!(
        coin.state.players.two.mana,
        ManaBank {
            matter: 1,
            mind: 0,
            spirit: 0,
        }
    );
    assert!(coin.state.coin.is_none());
    game = coin.state;

    let tender_played = apply(
        &game,
        &GameAction::PlaySummon {
            player: PlayerId::Two,
            card: well_tender.instance,
            slot: BenchSlot::First,
        },
    )
    .expect("Quarry Well-Tender is a Base Summon in hand");
    assert_eq!(
        tender_played.events,
        vec![GameEvent::SummonPlayed {
            player: PlayerId::Two,
            card: well_tender.instance,
            slot: BenchSlot::First,
        }]
    );
    assert!(
        tender_played.state.players.two.bench[0]
            .as_ref()
            .is_some_and(|summon| {
                summon.readiness == Readiness::Exhausted
                    && summon.turn.upgrade == UpgradeActivity::PlayedThisTurn
            })
    );
    game = tender_played.state;

    let scout_played = apply(
        &game,
        &GameAction::PlaySummon {
            player: PlayerId::Two,
            card: quarry_scout.instance,
            slot: BenchSlot::Second,
        },
    )
    .expect("Quarry Scout is a Base Summon in hand");
    assert_eq!(
        scout_played.events,
        vec![GameEvent::SummonPlayed {
            player: PlayerId::Two,
            card: quarry_scout.instance,
            slot: BenchSlot::Second,
        }]
    );
    assert!(scout_played.state.players.two.hand.contains(&second_wind));
    assert!(scout_played.state.players.two.hand.contains(&pathkeeper));
    game = scout_played.state;

    let end_two = apply(
        &game,
        &GameAction::EndTurn {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two can end the development turn");
    assert!(end_two.events.is_empty());
    assert_eq!(
        end_two.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        })
    );
    let one_passes = apply(
        &end_two.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player One holds final Priority");
    assert_eq!(
        one_passes.events,
        vec![GameEvent::PriorityPassed {
            player: PlayerId::One,
        }]
    );
    let one_begins = apply(
        &one_passes.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the second pass hands the turn to Player One");
    assert_eq!(
        one_begins.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::TurnBegan {
                player: PlayerId::One,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::One,
                positions: vec![Position::Main],
            },
            GameEvent::CardDrawn {
                player: PlayerId::One,
                card: barrow_herd[0].instance,
            },
            GameEvent::ManaProduced {
                player: PlayerId::One,
                source: ManaSource::Player,
                mana_type: ManaType::Spirit,
            },
        ]
    );
    assert_eq!(one_begins.state.turn.phase, Phase::Main);
    game = one_begins.state;

    let end_one = apply(
        &game,
        &GameAction::EndTurn {
            player: PlayerId::One,
        },
    )
    .expect("Player One can end the turn");
    let two_passes = apply(
        &end_one.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two holds final Priority");
    let two_begins = apply(
        &two_passes.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the second pass starts Player Two's Upkeep");
    assert_eq!(
        two_begins.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![
                    Position::Main,
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Second),
                ],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: set_paths[14].instance,
            },
        ]
    );
    assert_eq!(
        two_begins.state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Player,
        })
    );
    assert_eq!(two_begins.state.turn.phase, Phase::Upkeep);

    let chose_natural_mind = apply(
        &two_begins.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Mind,
        },
    )
    .expect("Matter and Mind are both anchored to Player Two's board");
    assert_eq!(
        chose_natural_mind.events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Player,
            mana_type: ManaType::Mind,
        }]
    );
    assert_eq!(chose_natural_mind.state.pending, None);
    assert_eq!(chose_natural_mind.state.turn.phase, Phase::Main);
    assert_eq!(
        chose_natural_mind.state.players.two.mana,
        ManaBank {
            matter: 1,
            mind: 1,
            spirit: 0,
        }
    );
    game = chose_natural_mind.state;

    let tender_skill = game
        .cards
        .get(well_tender.def)
        .and_then(|entity| entity.get::<Skill>())
        .expect("the real Quarry Well-Tender prints one Skill")
        .id;
    let tender_activated = apply(
        &game,
        &GameAction::ActivateSkill {
            player: PlayerId::Two,
            position: Position::Bench(BenchSlot::First),
            ability: tender_skill,
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    )
    .expect("the readied Well-Tender can activate");
    assert_eq!(
        tender_activated.events,
        vec![GameEvent::SkillActivated {
            player: PlayerId::Two,
            position: Position::Bench(BenchSlot::First),
            ability: tender_skill,
        }]
    );
    assert_eq!(
        tender_activated.state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Bench(BenchSlot::First)),
        })
    );
    assert!(
        tender_activated.state.players.two.bench[0]
            .as_ref()
            .is_some_and(|summon| summon.readiness == Readiness::Exhausted)
    );

    let chose_tender_matter = apply(
        &tender_activated.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Matter,
        },
    )
    .expect("the Well-Tender produces either of its printed Types");
    assert_eq!(
        chose_tender_matter.events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Bench(BenchSlot::First)),
            mana_type: ManaType::Matter,
        }]
    );
    assert_eq!(chose_tender_matter.state.players.one.mana.spirit, 1);
    assert_eq!(chose_tender_matter.state.players.two.mana.matter, 2);
    game = chose_tender_matter.state;

    let wind_cast = apply(
        &game,
        &GameAction::CastSpell {
            player: PlayerId::Two,
            card: second_wind.instance,
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    )
    .expect("Second Wind is an affordable Support Spell");
    assert_eq!(
        wind_cast.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SpellCast {
                player: PlayerId::Two,
                card: second_wind.instance,
                targets: vec![Position::Bench(BenchSlot::First)],
            },
        ]
    );
    let wind_stack = StackItem::Spell {
        caster: PlayerId::Two,
        card: second_wind,
        targets: vec![Position::Bench(BenchSlot::First)],
    };
    assert_eq!(wind_cast.state.stack, vec![wind_stack.clone()]);
    let wind_one_pass = apply(
        &wind_cast.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player One can pass on Second Wind");
    let wind_resolved = apply(
        &wind_one_pass.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the second pass resolves Second Wind");
    assert_eq!(
        wind_resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::StackItemResolved { item: wind_stack },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![Position::Bench(BenchSlot::First)],
            },
        ]
    );
    assert!(wind_resolved.state.stack.is_empty());
    assert!(
        wind_resolved
            .state
            .players
            .two
            .discard
            .contains(&second_wind)
    );
    assert!(
        wind_resolved.state.players.two.bench[0]
            .as_ref()
            .is_some_and(|summon| summon.readiness == Readiness::Ready)
    );
    game = wind_resolved.state;

    let tender_activated_again = apply(
        &game,
        &GameAction::ActivateSkill {
            player: PlayerId::Two,
            position: Position::Bench(BenchSlot::First),
            ability: tender_skill,
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    )
    .expect("Second Wind lets the same Well-Tender activate again");
    assert_eq!(
        tender_activated_again.events,
        vec![GameEvent::SkillActivated {
            player: PlayerId::Two,
            position: Position::Bench(BenchSlot::First),
            ability: tender_skill,
        }]
    );
    let chose_tender_mind = apply(
        &tender_activated_again.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Mind,
        },
    )
    .expect("the second activation keeps its controller and source");
    assert_eq!(
        chose_tender_mind.events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Bench(BenchSlot::First)),
            mana_type: ManaType::Mind,
        }]
    );
    game = chose_tender_mind.state;

    let scout_skill = game
        .cards
        .get(quarry_scout.def)
        .and_then(|entity| entity.get::<Skill>())
        .expect("the real Quarry Scout prints one Skill")
        .id;
    let scout_moved = apply(
        &game,
        &GameAction::ActivateSkill {
            player: PlayerId::Two,
            position: Position::Bench(BenchSlot::Second),
            ability: scout_skill,
            targets: vec![
                Position::Bench(BenchSlot::Second),
                Position::Bench(BenchSlot::Third),
            ],
            mana_hint: None,
        },
    )
    .expect("Quarry Scout can move itself to the empty third Bench slot");
    assert_eq!(
        scout_moved.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Mind,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::Two,
                position: Position::Bench(BenchSlot::Second),
                ability: scout_skill,
            },
        ]
    );
    assert!(scout_moved.state.players.two.bench[1].is_none());
    assert_eq!(
        scout_moved.state.players.two.bench[2]
            .as_ref()
            .map(|summon| summon.chain.top()),
        Some(quarry_scout)
    );
    assert!(
        scout_moved.state.players.two.bench[2]
            .as_ref()
            .is_some_and(|summon| summon.readiness == Readiness::Exhausted)
    );
    game = scout_moved.state;

    let end_two = apply(
        &game,
        &GameAction::EndTurn {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two can end the second development turn");
    let one_passes = apply(
        &end_two.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player One holds Priority");
    let one_begins = apply(
        &one_passes.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the next turn begins for Player One");
    assert_eq!(one_begins.state.turn.active_player, PlayerId::One);
    assert_eq!(one_begins.state.turn.phase, Phase::Main);
    assert_eq!(one_begins.state.players.one.mana.spirit, 2);

    let end_one = apply(
        &one_begins.state,
        &GameAction::EndTurn {
            player: PlayerId::One,
        },
    )
    .expect("Player One can end the turn");
    let two_passes = apply(
        &end_one.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two holds Priority");
    let two_begins = apply(
        &two_passes.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player Two reaches a later Upkeep");
    assert_eq!(
        two_begins.state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Player,
        })
    );
    assert_eq!(
        two_begins.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::TurnBegan {
                player: PlayerId::Two,
            },
            GameEvent::SummonsReadied {
                player: PlayerId::Two,
                positions: vec![
                    Position::Main,
                    Position::Bench(BenchSlot::First),
                    Position::Bench(BenchSlot::Third),
                ],
            },
            GameEvent::CardDrawn {
                player: PlayerId::Two,
                card: set_paths[15].instance,
            },
        ]
    );
    let chose_later_matter = apply(
        &two_begins.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Matter,
        },
    )
    .expect("Player Two chooses Matter on the later turn");
    assert_eq!(chose_later_matter.state.turn.phase, Phase::Main);
    assert_eq!(
        chose_later_matter.state.players.two.mana,
        ManaBank {
            matter: 2,
            mind: 1,
            spirit: 0,
        }
    );

    let upgraded = apply(
        &chose_later_matter.state,
        &GameAction::UpgradeSummon {
            player: PlayerId::Two,
            card: pathkeeper.instance,
            position: Position::Main,
        },
    )
    .expect("Warden Pathkeeper climbs the Starter on a later turn");
    assert_eq!(
        upgraded.events,
        vec![GameEvent::SummonUpgraded {
            player: PlayerId::Two,
            card: pathkeeper.instance,
            position: Position::Main,
        }]
    );
    let upgraded_main = upgraded
        .state
        .players
        .two
        .main
        .as_ref()
        .expect("the upgraded Main remains in play");
    assert_eq!(upgraded_main.chain.base(), set_paths_starter);
    assert_eq!(upgraded_main.chain.top(), pathkeeper);
    assert_eq!(
        upgraded_main.turn.upgrade,
        UpgradeActivity::UpgradedThisTurn
    );
    assert_eq!(upgraded_main.readiness, Readiness::Exhausted);

    let tender_leave_bench_trigger = upgraded
        .state
        .cards
        .get(well_tender.def)
        .and_then(|entity| entity.get::<Trigger>())
        .expect("the real Well-Tender prints one Trigger");
    assert_eq!(
        tender_leave_bench_trigger.get::<TriggerEvent>(),
        Some(&TriggerEvent::LeavesBench)
    );
    let tender_leave_bench_trigger_id = tender_leave_bench_trigger.id;

    let retreated = apply(
        &upgraded.state,
        &GameAction::Retreat {
            player: PlayerId::Two,
            slot: BenchSlot::First,
            mana_hint: None,
        },
    )
    .expect("the bank covers Warden Pathkeeper's normal Retreat");
    assert_eq!(
        retreated.events,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
                amount: 2,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::Two,
                main: BenchSlot::First,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Main,
                event: TriggerEvent::LeavesBench,
                ability: tender_leave_bench_trigger_id,
            },
        ]
    );
    assert!(retreated.state.turn.normal_retreat_used);
    assert_eq!(
        retreated
            .state
            .players
            .two
            .main
            .as_ref()
            .map(|summon| summon.chain.top()),
        Some(well_tender)
    );
    assert_eq!(
        retreated.state.players.two.bench[0]
            .as_ref()
            .map(|summon| summon.chain.top()),
        Some(pathkeeper)
    );
    assert_eq!(
        retreated.state.pending,
        Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Main),
        })
    );

    let retreat_mana = apply(
        &retreated.state,
        &GameAction::ChooseManaType {
            player: PlayerId::Two,
            mana_type: ManaType::Mind,
        },
    )
    .expect("the Well-Tender trigger still produces for its controller");
    assert_eq!(
        retreat_mana.events,
        vec![GameEvent::ManaProduced {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Main),
            mana_type: ManaType::Mind,
        }]
    );
    assert!(
        retreat_mana
            .state
            .players
            .two
            .main
            .as_ref()
            .is_some_and(|summon| summon.turn.main_entry.is_some())
    );

    let tender_attack = retreat_mana
        .state
        .cards
        .get(well_tender.def)
        .and_then(|entity| entity.get::<Attack>())
        .expect("the real Well-Tender prints one Attack")
        .id;
    let declared = apply(
        &retreat_mana.state,
        &GameAction::DeclareAttack {
            player: PlayerId::Two,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("the Well-Tender's normal attack is free");
    assert_eq!(
        declared.events,
        vec![GameEvent::AttackDeclared {
            player: PlayerId::Two,
            target: Position::Main,
        }]
    );
    assert_eq!(
        declared.state.stack,
        vec![StackItem::Attack {
            attacker: PlayerId::Two,
            target: Position::Main,
        }]
    );
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the defending player holds Priority first");
    let lethal = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the second pass resolves the lethal normal attack");
    let damage_context = DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::Two,
            position: Position::Main,
            ability: tender_attack,
        },
        target: BattlefieldTarget {
            controller: PlayerId::One,
            position: Position::Main,
        },
    };
    assert_eq!(
        lethal.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::Two,
                    target: Position::Main,
                },
            },
            GameEvent::DamageCalculationStarted {
                context: damage_context,
                base: 10,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context: damage_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(tender_attack),
                input: 10,
                output: 10,
            },
            GameEvent::DamageApplied {
                context: damage_context,
                amount: 10,
                before: 40,
                after: 50,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::One,
            },
            GameEvent::GameEnded {
                winner: PlayerId::Two,
                reason: LossReason::NoPromotionAvailable,
            },
        ]
    );
    assert_eq!(lethal.state.players.one.main, None);
    assert!(lethal.state.players.one.bench.iter().all(Option::is_none));
    assert_eq!(lethal.state.players.one.main_losses, 1);
    assert_eq!(lethal.state.players.one.discard, vec![barrow_starter]);
    assert_eq!(
        lethal.state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::Two,
            reason: LossReason::NoPromotionAvailable,
        })
    );
}
