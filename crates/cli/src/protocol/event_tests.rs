#![allow(clippy::expect_used, clippy::unwrap_used)]
use super::*;
use summoners_cards::built_in_catalog;
use summoners_core::domain::cards::DamageConstraints;
use summoners_core::domain::events::{
    BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource, DamageStage,
};

fn state_and_descriptions() -> (GameState, CardDescriptions) {
    let catalog = built_in_catalog().expect("catalog");
    let state = crate::setup::initial_state(
        catalog.library(),
        [catalog.set_paths(), catalog.barrow_herd()],
        4,
    )
    .expect("state");
    let descriptions = CardDescriptions::from_card_set(&state.cards);
    (state, descriptions)
}

fn probe_id() -> EntityId {
    EntityId::parse(&"ab".repeat(16)).expect("valid probe id")
}

fn sample_damage_context() -> DamageContext {
    DamageContext {
        source: DamageSource::Skill {
            controller: PlayerId::One,
            position: Position::Main,
            ability: probe_id(),
        },
        target: BattlefieldTarget {
            controller: PlayerId::Two,
            position: Position::Bench(BenchSlot::First),
        },
    }
}

fn assert_notice_same_for_both_viewers(
    event: &GameEvent,
    identities: &CardIdentityMap,
    expected: &str,
) {
    for viewer in [PlayerId::One, PlayerId::Two] {
        let notices = player_notices(std::slice::from_ref(event), identities, viewer);
        assert_eq!(notices[0].text, expected);
    }
}

#[test]
fn turn_began_notice_names_the_player() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::TurnBegan {
        player: PlayerId::One,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One began a turn");
}

#[test]
fn summons_readied_notice_lists_every_position_in_order() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::SummonsReadied {
        player: PlayerId::One,
        positions: vec![Position::Main, Position::Bench(BenchSlot::First)],
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One readied Main, Bench 1");
}

#[test]
fn card_drawn_notice_names_the_card_only_for_the_drawing_player_for_both_owners() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    for (player, deck) in [
        (PlayerId::One, &state.players.one.deck),
        (PlayerId::Two, &state.players.two.deck),
    ] {
        let card = deck[0].instance;
        let name = identities.name(card);
        let event = GameEvent::CardDrawn { player, card };
        let owner = player_notices(std::slice::from_ref(&event), &identities, player);
        let opponent = player_notices(std::slice::from_ref(&event), &identities, player.opponent());
        assert_eq!(
            owner[0].text,
            format!("{} drew {}", player_text(player), name)
        );
        assert_eq!(
            opponent[0].text,
            format!("{} drew a card", player_text(player))
        );
        assert!(!opponent[0].text.contains(&name));
    }
}

#[test]
fn mana_produced_notice_names_the_mana_type() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::ManaProduced {
        player: PlayerId::One,
        source: summoners_core::domain::state::ManaSource::Player,
        mana_type: ManaType::Matter,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One produced Matter Mana");
}

#[test]
fn summon_played_notice_names_the_card_and_the_bench_slot() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let card = state.players.one.hand[0].instance;
    let name = identities.name(card);
    let event = GameEvent::SummonPlayed {
        player: PlayerId::One,
        card,
        slot: BenchSlot::First,
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        &format!("Player One played {name} to Bench 1"),
    );
}

#[test]
fn summon_upgraded_notice_names_the_position_and_the_card() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let card = state.players.one.hand[1].instance;
    let name = identities.name(card);
    let event = GameEvent::SummonUpgraded {
        player: PlayerId::One,
        card,
        position: Position::Main,
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        &format!("Player One upgraded Main with {name}"),
    );
}

#[test]
fn spell_cast_notice_names_the_card() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let card = state.players.one.hand[2].instance;
    let name = identities.name(card);
    let event = GameEvent::SpellCast {
        player: PlayerId::One,
        card,
        targets: vec![Position::Main],
    };
    assert_notice_same_for_both_viewers(&event, &identities, &format!("Player One cast {name}"));
}

#[test]
fn skill_activated_notice_names_the_position() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::SkillActivated {
        player: PlayerId::One,
        position: Position::Main,
        ability: probe_id(),
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        "Player One activated a Skill at Main",
    );
}

#[test]
fn attack_declared_notice_names_the_target() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::AttackDeclared {
        player: PlayerId::One,
        target: Position::Bench(BenchSlot::Second),
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        "Player One declared an attack at Bench 2",
    );
}

#[test]
fn priority_passed_notice_names_the_player() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::PriorityPassed {
        player: PlayerId::One,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One passed Priority");
}

#[test]
fn stack_item_resolved_notice_is_fixed_text() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::StackItemResolved {
        item: StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        },
    };
    assert_notice_same_for_both_viewers(&event, &identities, "A Stack item resolved");
}

#[test]
fn damage_calculation_started_notice_names_the_base() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::DamageCalculationStarted {
        context: sample_damage_context(),
        base: 7,
        constraints: DamageConstraints::new(),
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Damage calculation began at 7");
}

#[test]
fn damage_adjustment_applied_notice_names_the_output() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::DamageAdjustmentApplied {
        context: sample_damage_context(),
        stage: DamageStage::Addition,
        operation: DamageOperation::Add(5),
        origin: DamageOrigin::PrintedAbility(probe_id()),
        input: 7,
        output: 12,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Damage adjusted to 12");
}

#[test]
fn damage_adjustment_skipped_notice_is_fixed_text() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::DamageAdjustmentSkipped {
        context: sample_damage_context(),
        stage: DamageStage::Clamp,
        operation: DamageOperation::ClampToZero,
        origin: DamageOrigin::PrintedAbility(probe_id()),
        input: 3,
        constraint: summoners_core::domain::cards::DamageConstraint::Unpreventable,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "A damage adjustment was skipped");
}

#[test]
fn damage_applied_notice_names_amount_before_and_after() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::DamageApplied {
        context: sample_damage_context(),
        amount: 5,
        before: 10,
        after: 5,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Damage 5 applied: 10 to 5");
}

#[test]
fn healed_notice_names_the_position_and_amount() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::Healed {
        position: Position::Main,
        amount: 4,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Main healed for 4");
}

#[test]
fn summon_destroyed_notice_names_the_position() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::SummonDestroyed {
        position: Position::Bench(BenchSlot::Third),
        owner: PlayerId::One,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Bench 3 was destroyed");
}

#[test]
fn prize_recovered_notice_names_the_card_only_for_the_recovering_player_for_both_owners() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    for (player, prizes) in [
        (PlayerId::One, &state.players.one.prizes),
        (PlayerId::Two, &state.players.two.prizes),
    ] {
        let card = prizes[0].instance;
        let name = identities.name(card);
        let event = GameEvent::PrizeRecovered { player, card };
        let owner = player_notices(std::slice::from_ref(&event), &identities, player);
        let opponent = player_notices(std::slice::from_ref(&event), &identities, player.opponent());
        assert_eq!(
            owner[0].text,
            format!("{} recovered a Prize {}", player_text(player), name)
        );
        assert_eq!(
            opponent[0].text,
            format!("{} recovered a Prize a card", player_text(player))
        );
        assert!(!opponent[0].text.contains(&name));
    }
}

#[test]
fn prizes_viewed_notice_names_cards_only_for_the_viewing_player_for_both_owners() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    for (player, prizes) in [
        (PlayerId::One, &state.players.one.prizes),
        (PlayerId::Two, &state.players.two.prizes),
    ] {
        let cards: Vec<CardInstanceId> = prizes.iter().map(|card| card.instance).collect();
        let names: Vec<String> = cards.iter().map(|card| identities.name(*card)).collect();
        let event = GameEvent::PrizesViewed {
            player,
            prizes: cards.clone(),
        };
        let owner = player_notices(std::slice::from_ref(&event), &identities, player);
        let opponent = player_notices(std::slice::from_ref(&event), &identities, player.opponent());
        assert_eq!(
            owner[0].text,
            format!(
                "{} viewed Prizes: {}",
                player_text(player),
                names.join(", ")
            )
        );
        assert_eq!(
            opponent[0].text,
            format!("{} viewed {} Prizes", player_text(player), cards.len())
        );
        for name in &names {
            assert!(!opponent[0].text.contains(name));
        }
    }
}

#[test]
fn summon_promoted_notice_names_the_bench_slot() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::SummonPromoted {
        player: PlayerId::One,
        from: BenchSlot::First,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One promoted Bench 1");
}

#[test]
fn summons_swapped_notice_names_the_bench_slot() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::SummonsSwapped {
        player: PlayerId::One,
        main: BenchSlot::Second,
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        "Player One swapped Main with Bench 2",
    );
}

#[test]
fn trigger_fired_notice_names_the_position() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::TriggerFired {
        controller: PlayerId::One,
        position: Position::Main,
        event: summoners_core::domain::cards::TriggerEvent::YourUpkeep,
        ability: probe_id(),
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        "Player One triggered an ability at Main",
    );
}

#[test]
fn coin_converted_notice_names_the_mana_type() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::CoinConverted {
        player: PlayerId::Two,
        mana_type: ManaType::Spirit,
    };
    assert_notice_same_for_both_viewers(
        &event,
        &identities,
        "Player Two converted Coin to Spirit Mana",
    );
}

#[test]
fn mana_deducted_notice_names_the_amount_and_mana_type() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::ManaDeducted {
        player: PlayerId::One,
        mana_type: ManaType::Matter,
        amount: 3,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One spent 3 Matter Mana");
}

#[test]
fn game_ended_notice_names_the_winner() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let event = GameEvent::GameEnded {
        winner: PlayerId::One,
        reason: summoners_core::domain::state::LossReason::ThirdMainLoss,
    };
    assert_notice_same_for_both_viewers(&event, &identities, "Player One won the game");
}

#[test]
fn card_identity_map_resolves_deck_hand_prize_and_board_instances_and_unknown_falls_back() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);

    let deck_card = state.players.one.deck[0].instance;
    let hand_card = state.players.one.hand[0].instance;
    let prize_card = state.players.one.prizes[0].instance;
    let board_card = state
        .players
        .one
        .main
        .as_ref()
        .expect("opening Main Summon")
        .chain
        .layers()
        .next()
        .expect("chain has a base layer")
        .instance;

    assert_ne!(identities.name(deck_card), "Unknown card");
    assert_ne!(identities.name(hand_card), "Unknown card");
    assert_ne!(identities.name(prize_card), "Unknown card");
    assert_ne!(identities.name(board_card), "Unknown card");
    assert_eq!(identities.name(CardInstanceId(u32::MAX)), "Unknown card");
}

#[test]
fn a_card_drawn_later_from_the_opening_deck_still_resolves_a_name() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let deck_card = state.players.one.deck[3].instance;
    let name = identities.name(deck_card);
    assert_ne!(name, "Unknown card");
    let event = GameEvent::CardDrawn {
        player: PlayerId::One,
        card: deck_card,
    };
    let notice = player_notices(std::slice::from_ref(&event), &identities, PlayerId::One);
    assert_eq!(notice[0].text, format!("Player One drew {name}"));
}

#[test]
fn envelopes_carry_notices_in_event_order_and_hide_names_from_the_opponent() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let draw_card = state.players.one.deck[0].instance;
    let prize_card = state.players.one.prizes[0].instance;
    let draw_name = identities.name(draw_card);
    let prize_name = identities.name(prize_card);

    let events = vec![
        GameEvent::CardDrawn {
            player: PlayerId::One,
            card: draw_card,
        },
        GameEvent::PrizeRecovered {
            player: PlayerId::One,
            card: prize_card,
        },
        GameEvent::PrizesViewed {
            player: PlayerId::One,
            prizes: vec![prize_card],
        },
    ];

    let owner_notices = player_notices(&events, &identities, PlayerId::One);
    let opponent_notices = player_notices(&events, &identities, PlayerId::Two);

    let owner_notices_json = serde_json::to_string(&owner_notices).expect("owner notices json");
    let opponent_notices_json =
        serde_json::to_string(&opponent_notices).expect("opponent notices json");
    assert!(owner_notices_json.contains(&draw_name));
    assert!(owner_notices_json.contains(&prize_name));
    assert!(!opponent_notices_json.contains(&draw_name));
    assert!(!opponent_notices_json.contains(&prize_name));

    let view = player_view(&state, PlayerId::One, &descriptions);
    let update = ServerEnvelope::Update {
        revision: 1,
        view: view.clone(),
        notices: owner_notices.clone(),
        reply: None,
        result: None,
    };
    let update_json = serde_json::to_string(&update).expect("update json");
    assert!(update_json.contains(&draw_name));
    assert!(update_json.contains(&prize_name));
    let drew_index = update_json.find("drew").expect("drew marker");
    let recovered_index = update_json
        .find("recovered a Prize")
        .expect("recovered marker");
    let viewed_index = update_json.find("viewed Prizes").expect("viewed marker");
    assert!(drew_index < recovered_index);
    assert!(recovered_index < viewed_index);

    let finished = ServerEnvelope::Finished {
        outcome: OutcomeView {
            winner: Seat::One,
            reason: OutcomeReasonView::ThirdMainLoss,
        },
        view,
        notices: owner_notices,
        reply: None,
    };
    let finished_json = serde_json::to_string(&finished).expect("finished json");
    assert!(finished_json.contains(&draw_name));
    let finished_drew_index = finished_json.find("drew").expect("finished drew marker");
    let finished_recovered_index = finished_json
        .find("recovered a Prize")
        .expect("finished recovered marker");
    assert!(finished_drew_index < finished_recovered_index);
}

#[test]
fn every_game_event_variant_projects_one_non_empty_notice_in_order_for_both_viewers() {
    let (state, descriptions) = state_and_descriptions();
    let identities = CardIdentityMap::from_initial_state(&state, &descriptions);
    let draw_card = state.players.one.deck[0].instance;
    let hand_card = state.players.one.hand[0].instance;
    let prize_card = state.players.one.prizes[0].instance;
    let context = sample_damage_context();

    let events = vec![
        GameEvent::TurnBegan {
            player: PlayerId::One,
        },
        GameEvent::SummonsReadied {
            player: PlayerId::One,
            positions: vec![Position::Main],
        },
        GameEvent::CardDrawn {
            player: PlayerId::One,
            card: draw_card,
        },
        GameEvent::ManaProduced {
            player: PlayerId::One,
            source: summoners_core::domain::state::ManaSource::Player,
            mana_type: ManaType::Matter,
        },
        GameEvent::SummonPlayed {
            player: PlayerId::One,
            card: hand_card,
            slot: BenchSlot::First,
        },
        GameEvent::SummonUpgraded {
            player: PlayerId::One,
            card: hand_card,
            position: Position::Main,
        },
        GameEvent::SpellCast {
            player: PlayerId::One,
            card: hand_card,
            targets: vec![Position::Main],
        },
        GameEvent::SkillActivated {
            player: PlayerId::One,
            position: Position::Main,
            ability: probe_id(),
        },
        GameEvent::AttackDeclared {
            player: PlayerId::One,
            target: Position::Main,
        },
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
            context,
            base: 1,
            constraints: DamageConstraints::new(),
        },
        GameEvent::DamageAdjustmentApplied {
            context,
            stage: DamageStage::Addition,
            operation: DamageOperation::Add(1),
            origin: DamageOrigin::PrintedAbility(probe_id()),
            input: 1,
            output: 2,
        },
        GameEvent::DamageAdjustmentSkipped {
            context,
            stage: DamageStage::Clamp,
            operation: DamageOperation::ClampToZero,
            origin: DamageOrigin::PrintedAbility(probe_id()),
            input: 1,
            constraint: summoners_core::domain::cards::DamageConstraint::Unincreasable,
        },
        GameEvent::DamageApplied {
            context,
            amount: 1,
            before: 2,
            after: 1,
        },
        GameEvent::Healed {
            position: Position::Main,
            amount: 1,
        },
        GameEvent::SummonDestroyed {
            position: Position::Main,
            owner: PlayerId::One,
        },
        GameEvent::PrizeRecovered {
            player: PlayerId::One,
            card: prize_card,
        },
        GameEvent::PrizesViewed {
            player: PlayerId::One,
            prizes: vec![prize_card],
        },
        GameEvent::SummonPromoted {
            player: PlayerId::One,
            from: BenchSlot::First,
        },
        GameEvent::SummonsSwapped {
            player: PlayerId::One,
            main: BenchSlot::First,
        },
        GameEvent::TriggerFired {
            controller: PlayerId::One,
            position: Position::Main,
            event: summoners_core::domain::cards::TriggerEvent::YourUpkeep,
            ability: probe_id(),
        },
        GameEvent::CoinConverted {
            player: PlayerId::Two,
            mana_type: ManaType::Spirit,
        },
        GameEvent::ManaDeducted {
            player: PlayerId::One,
            mana_type: ManaType::Matter,
            amount: 1,
        },
        GameEvent::GameEnded {
            winner: PlayerId::One,
            reason: summoners_core::domain::state::LossReason::ThirdMainLoss,
        },
    ];
    assert_eq!(events.len(), 25);

    for viewer in [PlayerId::One, PlayerId::Two] {
        let notices = player_notices(&events, &identities, viewer);
        assert_eq!(notices.len(), events.len());
        for notice in &notices {
            assert!(!notice.text.is_empty());
        }
    }
}
