#![allow(clippy::expect_used)]

mod support;

use summoners_cards::built_in_catalog;
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::{Attack, DamageConstraints, EffectLeaf, Trigger, TriggerEvent},
        errors::ActionError,
        events::{
            BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource,
            DamageStage, GameEvent,
        },
        ids::{BenchSlot, PlayerId, Position},
        state::{
            GameOutcome, GameStatus, LossReason, ManaSource, PendingInput, Phase, StackItem,
            StackWindow,
        },
    },
    engine::apply::apply,
    scenario::ScenarioSummon,
};

use support::{PhysicalCards, player, state};

#[test]
fn destruction_chain_resolves_choices_triggers_healing_and_third_main_loss() {
    let catalog = built_in_catalog().expect("the built-in catalog is valid");
    let mut physical = PhysicalCards::new(catalog.library(), 1);
    let warden_initiate = physical.one("foundations/warden-initiate");
    let sow_piglet = physical.one("foundations/sow-piglet");
    let set_paths = physical.deck(catalog.set_paths());
    let barrow_herd = physical.deck(catalog.barrow_herd());

    // These physical cards come from the real Deck recipes in authored
    // order. The two Hearth Wardens are distinct copies of one definition.
    let first_hearth = barrow_herd[4];
    let second_hearth = barrow_herd[5];
    let ash_shepherd = barrow_herd[10];
    let recovered_prize = barrow_herd[12];
    let renewing_balm = barrow_herd[14];
    let ember_lance = barrow_herd[16];
    let one_draws = vec![set_paths[2], set_paths[3], set_paths[6]];
    let two_draws = vec![barrow_herd[0], barrow_herd[1], barrow_herd[2]];

    let mut one = player(ScenarioSummon {
        chain: vec![warden_initiate],
        damage: 0,
        ready: true,
    });
    one.deck = one_draws;

    let mut two = player(ScenarioSummon {
        chain: vec![sow_piglet],
        damage: 40,
        ready: true,
    });
    two.deck = two_draws;
    two.prizes = vec![recovered_prize];
    two.discard = vec![renewing_balm, ember_lance];
    two.bench = [
        Some(ScenarioSummon {
            chain: vec![ash_shepherd],
            damage: 30,
            ready: true,
        }),
        Some(ScenarioSummon {
            chain: vec![first_hearth],
            // Promotion fires the printed 15-point heal before the next
            // attack. The result is exactly 50 Damage on 60 Life.
            damage: 65,
            ready: true,
        }),
        Some(ScenarioSummon {
            chain: vec![second_hearth],
            damage: 15,
            ready: true,
        }),
    ];

    let game = state(&catalog, one, two, PlayerId::One).expect("the destruction state is valid");
    assert_eq!(catalog.set_paths().body().len(), 20);
    assert_eq!(catalog.barrow_herd().body().len(), 20);

    let attack_id = game
        .cards
        .get(warden_initiate.def)
        .and_then(|entity| entity.get::<Attack>())
        .expect("Warden Initiate prints one Attack")
        .id;
    let ash_trigger_id = game
        .cards
        .get(ash_shepherd.def)
        .and_then(|entity| entity.get::<Trigger>())
        .expect("Ash Shepherd prints one Trigger")
        .id;
    let hearth_trigger_id = game
        .cards
        .get(first_hearth.def)
        .and_then(|entity| entity.get::<Trigger>())
        .expect("Hearth Warden prints one Trigger")
        .id;
    let attack_context = DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability: attack_id,
        },
        target: BattlefieldTarget {
            controller: PlayerId::Two,
            position: Position::Main,
        },
    };
    let ash_stack = StackItem::Trigger {
        controller: PlayerId::Two,
        source: Position::Bench(BenchSlot::First),
        ability: ash_trigger_id,
        event: TriggerEvent::AnySummonDestroyed,
        targets: vec![Position::Bench(BenchSlot::First)],
        effects: vec![EffectLeaf::ReturnSpellFromDiscard],
    };

    let first_declared = apply(
        &game,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("Warden Initiate's normal Attack is free");
    assert_eq!(
        first_declared.events,
        vec![GameEvent::AttackDeclared {
            player: PlayerId::One,
            target: Position::Main,
        }]
    );
    assert_eq!(
        first_declared.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        })
    );
    let first_defender_passed = apply(
        &first_declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Attack Priority first");
    let first_destroyed = apply(
        &first_defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the second pass resolves the first lethal Attack");
    assert_eq!(
        first_destroyed.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            GameEvent::DamageCalculationStarted {
                context: attack_context,
                base: 10,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context: attack_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(attack_id),
                input: 10,
                output: 10,
            },
            GameEvent::DamageApplied {
                context: attack_context,
                amount: 10,
                before: 40,
                after: 50,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Bench(BenchSlot::First),
                event: TriggerEvent::AnySummonDestroyed,
                ability: ash_trigger_id,
            },
        ]
    );
    assert_eq!(first_destroyed.state.stack, vec![ash_stack.clone()]);
    assert_eq!(first_destroyed.state.stack_segment_bases, vec![0]);
    assert_eq!(
        first_destroyed.state.turn.window,
        Some(StackWindow {
            holder: PlayerId::One,
            prior_pass: false,
        })
    );
    assert_eq!(first_destroyed.state.players.two.main_losses, 0);
    assert_eq!(first_destroyed.state.players.two.main, None);
    assert_eq!(first_destroyed.state.pending, None);

    let before_wrong_trigger_actor = first_destroyed.state.clone();
    assert_eq!(
        apply(
            &first_destroyed.state,
            &GameAction::PassPriority {
                player: PlayerId::Two,
            },
        ),
        Err(ActionError::NotYourDecision)
    );
    assert_eq!(first_destroyed.state, before_wrong_trigger_actor);

    let first_trigger_opponent_passed = apply(
        &first_destroyed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Ash Shepherd's opponent holds Trigger Priority first");
    assert_eq!(
        first_trigger_opponent_passed.events,
        vec![GameEvent::PriorityPassed {
            player: PlayerId::One,
        }]
    );
    let first_trigger_resolved = apply(
        &first_trigger_opponent_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the controller's pass resolves Ash Shepherd and resumes destruction");
    assert_eq!(
        first_trigger_resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::StackItemResolved {
                item: ash_stack.clone(),
            },
        ]
    );
    assert!(first_trigger_resolved.state.stack.is_empty());
    assert!(first_trigger_resolved.state.stack_segment_bases.is_empty());
    assert_eq!(
        first_trigger_resolved.state.pending,
        Some(PendingInput::PrizePick {
            chooser: PlayerId::One,
        })
    );
    assert_eq!(first_trigger_resolved.state.players.two.main_losses, 1);
    assert_eq!(
        first_trigger_resolved.state.players.two.hand,
        vec![renewing_balm]
    );
    assert_eq!(
        first_trigger_resolved.state.players.two.discard,
        vec![ember_lance, sow_piglet]
    );

    let before_wrong_prize_actor = first_trigger_resolved.state.clone();
    assert_eq!(
        apply(
            &first_trigger_resolved.state,
            &GameAction::ChoosePrize {
                player: PlayerId::Two,
                prize_index: 0,
            },
        ),
        Err(ActionError::NotYourDecision)
    );
    assert_eq!(first_trigger_resolved.state, before_wrong_prize_actor);

    let prize_chosen = apply(
        &first_trigger_resolved.state,
        &GameAction::ChoosePrize {
            player: PlayerId::One,
            prize_index: 0,
        },
    )
    .expect("the opponent chooses Player Two's Prize");
    assert_eq!(
        prize_chosen.events,
        vec![GameEvent::PrizeRecovered {
            player: PlayerId::Two,
            card: recovered_prize.instance,
        }]
    );
    assert_eq!(prize_chosen.state.players.two.prizes, vec![]);
    assert_eq!(
        prize_chosen.state.players.two.hand,
        vec![renewing_balm, recovered_prize]
    );
    assert_eq!(
        prize_chosen.state.pending,
        Some(PendingInput::Promotion {
            player: PlayerId::Two,
        })
    );
    assert!(
        prize_chosen
            .state
            .players
            .two
            .bench
            .iter()
            .all(Option::is_some)
    );

    let before_wrong_promotion_actor = prize_chosen.state.clone();
    assert_eq!(
        apply(
            &prize_chosen.state,
            &GameAction::ChoosePromotion {
                player: PlayerId::One,
                slot: BenchSlot::Second,
            },
        ),
        Err(ActionError::NotYourDecision)
    );
    assert_eq!(prize_chosen.state, before_wrong_promotion_actor);

    let first_promoted = apply(
        &prize_chosen.state,
        &GameAction::ChoosePromotion {
            player: PlayerId::Two,
            slot: BenchSlot::Second,
        },
    )
    .expect("the owner chooses Hearth Warden from three candidates");
    assert_eq!(
        first_promoted.events,
        vec![
            GameEvent::SummonPromoted {
                player: PlayerId::Two,
                from: BenchSlot::Second,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Main,
                event: TriggerEvent::EntersMain,
                ability: hearth_trigger_id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ]
    );
    let first_healed = first_promoted
        .state
        .players
        .two
        .main
        .as_ref()
        .expect("Hearth Warden fills Main");
    assert_eq!(first_healed.chain.top(), first_hearth);
    assert_eq!(first_healed.damage, 50);
    assert!(first_healed.entered_main_this_turn);
    assert_eq!(first_promoted.state.players.two.main_losses, 1);
    assert_eq!(first_promoted.state.pending, None);
    assert_eq!(first_promoted.state.status, GameStatus::Playing);

    let first_turn_end = apply(
        &first_promoted.state,
        &GameAction::EndTurn {
            player: PlayerId::One,
        },
    )
    .expect("Player One can end Combat after the first Attack");
    let two_final_pass = apply(
        &first_turn_end.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two holds final Priority");
    let two_turn = apply(
        &two_final_pass.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player Two's turn begins");
    assert_eq!(two_turn.state.turn.active_player, PlayerId::Two);
    assert_eq!(two_turn.state.turn.phase, Phase::Main);
    assert_eq!(two_turn.state.players.two.main_losses, 1);
    assert_eq!(two_turn.state.players.two.mana.spirit, 1);
    assert!(two_turn.events.contains(&GameEvent::ManaProduced {
        player: PlayerId::Two,
        source: ManaSource::Player,
        mana_type: summoners_core::domain::ids::ManaType::Spirit,
    }));

    let two_turn_end = apply(
        &two_turn.state,
        &GameAction::EndTurn {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two can end the turn");
    let one_final_pass = apply(
        &two_turn_end.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player One holds final Priority");
    let one_turn = apply(
        &one_final_pass.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player One reaches a new Main Phase");
    assert_eq!(one_turn.state.turn.active_player, PlayerId::One);
    assert_eq!(one_turn.state.turn.phase, Phase::Main);
    assert!(!one_turn.state.turn.normal_attack_used);

    let second_declared = apply(
        &one_turn.state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("the new turn permits a second normal Attack");
    let second_defender_passed = apply(
        &second_declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two holds Attack Priority");
    let second_destroyed = apply(
        &second_defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the second Attack destroys the promoted Hearth Warden");
    assert_eq!(
        second_destroyed.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            GameEvent::DamageCalculationStarted {
                context: attack_context,
                base: 10,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context: attack_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(attack_id),
                input: 10,
                output: 10,
            },
            GameEvent::DamageApplied {
                context: attack_context,
                amount: 10,
                before: 50,
                after: 60,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Bench(BenchSlot::First),
                event: TriggerEvent::AnySummonDestroyed,
                ability: ash_trigger_id,
            },
        ]
    );
    assert_eq!(second_destroyed.state.stack, vec![ash_stack.clone()]);
    assert_eq!(second_destroyed.state.players.two.main_losses, 1);

    let second_trigger_opponent_passed = apply(
        &second_destroyed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player One passes on the second Ash Shepherd Trigger");
    let second_trigger_resolved = apply(
        &second_trigger_opponent_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the second Ash Shepherd Trigger resolves");
    assert_eq!(
        second_trigger_resolved.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::Two,
            },
            GameEvent::StackItemResolved { item: ash_stack },
        ]
    );
    assert_eq!(
        second_trigger_resolved.state.pending,
        Some(PendingInput::Promotion {
            player: PlayerId::Two,
        })
    );
    assert_eq!(second_trigger_resolved.state.players.two.main_losses, 2);
    assert!(
        second_trigger_resolved
            .state
            .players
            .two
            .hand
            .contains(&ember_lance)
    );
    assert_eq!(
        second_trigger_resolved.state.players.two.discard,
        vec![sow_piglet, first_hearth]
    );

    let second_promoted = apply(
        &second_trigger_resolved.state,
        &GameAction::ChoosePromotion {
            player: PlayerId::Two,
            slot: BenchSlot::First,
        },
    )
    .expect("the owner chooses Ash Shepherd from two candidates");
    assert_eq!(
        second_promoted.events,
        vec![GameEvent::SummonPromoted {
            player: PlayerId::Two,
            from: BenchSlot::First,
        }]
    );
    assert_eq!(
        second_promoted
            .state
            .players
            .two
            .main
            .as_ref()
            .map(|summon| summon.chain.top()),
        Some(ash_shepherd)
    );
    assert_eq!(second_promoted.state.players.two.main_losses, 2);

    let second_turn_end = apply(
        &second_promoted.state,
        &GameAction::EndTurn {
            player: PlayerId::One,
        },
    )
    .expect("Player One can end Combat after the second Attack");
    let two_final_pass = apply(
        &second_turn_end.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two holds final Priority");
    let two_turn = apply(
        &two_final_pass.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player Two's next turn begins");
    assert_eq!(two_turn.state.turn.active_player, PlayerId::Two);
    assert_eq!(two_turn.state.turn.phase, Phase::Main);

    let two_turn_end = apply(
        &two_turn.state,
        &GameAction::EndTurn {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two can end the turn");
    let one_final_pass = apply(
        &two_turn_end.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("Player One holds final Priority");
    let one_turn = apply(
        &one_final_pass.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player One reaches the third Attack turn");
    assert_eq!(one_turn.state.turn.active_player, PlayerId::One);
    assert_eq!(one_turn.state.turn.phase, Phase::Main);
    assert!(!one_turn.state.turn.normal_attack_used);

    let third_declared = apply(
        &one_turn.state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("the third turn permits the terminal normal Attack");
    let third_defender_passed = apply(
        &third_declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("Player Two holds Attack Priority");
    let terminal = apply(
        &third_defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the third Main destruction ends the game");
    assert_eq!(
        terminal.events,
        vec![
            GameEvent::PriorityPassed {
                player: PlayerId::One,
            },
            GameEvent::StackItemResolved {
                item: StackItem::Attack {
                    attacker: PlayerId::One,
                    target: Position::Main,
                },
            },
            GameEvent::DamageCalculationStarted {
                context: attack_context,
                base: 10,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context: attack_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(attack_id),
                input: 10,
                output: 10,
            },
            GameEvent::DamageApplied {
                context: attack_context,
                amount: 10,
                before: 30,
                after: 40,
            },
            GameEvent::SummonDestroyed {
                position: Position::Main,
                owner: PlayerId::Two,
            },
            GameEvent::SummonPromoted {
                player: PlayerId::Two,
                from: BenchSlot::Third,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Main,
                event: TriggerEvent::EntersMain,
                ability: hearth_trigger_id,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
            GameEvent::GameEnded {
                winner: PlayerId::One,
                reason: LossReason::ThirdMainLoss,
            },
        ]
    );
    assert_eq!(terminal.state.players.two.main_losses, 3);
    assert_eq!(
        terminal
            .state
            .players
            .two
            .main
            .as_ref()
            .map(|summon| (summon.chain.top(), summon.damage)),
        Some((second_hearth, 0))
    );
    assert_eq!(
        terminal.state.players.two.discard,
        vec![sow_piglet, first_hearth, ash_shepherd]
    );
    assert_eq!(
        terminal.state.status,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::ThirdMainLoss,
        })
    );

    let terminal_snapshot = terminal.state.clone();
    assert_eq!(
        apply(
            &terminal.state,
            &GameAction::EndTurn {
                player: PlayerId::Two,
            },
        ),
        Err(ActionError::GameAlreadyOver)
    );
    assert_eq!(terminal.state, terminal_snapshot);
}
