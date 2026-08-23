#![allow(clippy::expect_used)]

use std::collections::VecDeque;

use super::*;

fn entity_id(value: &str) -> EntityId {
    EntityId::parse(value).expect("the fixture entity ID is valid")
}

fn definition() -> EntityId {
    entity_id("01234567-89ab-cdef-0123-456789abcdef")
}

fn card(instance: u32) -> CardRef {
    CardRef {
        instance: CardInstanceId(instance),
        def: definition(),
    }
}

fn summon(
    instance: u32,
    readiness: Readiness,
    upgrade: UpgradeActivity,
    entered_main: bool,
) -> SummonInstance {
    SummonInstance {
        chain: UpgradeChain::new(card(instance), vec![card(instance + 1)]),
        damage: 17,
        readiness,
        owner: PlayerId::One,
        controller: PlayerId::Two,
        duration_markers: vec![DurationMarker::CannotBeMovedByOpponent],
        turn: SummonTurnRecord {
            upgrade,
            main_entry: entered_main.then_some(EnteredMain),
        },
    }
}

fn player(one: bool) -> PlayerState {
    PlayerState {
        main: Some(summon(
            if one { 1 } else { 11 },
            if one {
                Readiness::Ready
            } else {
                Readiness::Exhausted
            },
            if one {
                UpgradeActivity::PlayedThisTurn
            } else {
                UpgradeActivity::UpgradedThisTurn
            },
            one,
        )),
        bench: [
            Some(summon(
                21,
                Readiness::Ready,
                UpgradeActivity::Available,
                false,
            )),
            None,
            None,
        ],
        deck: vec![card(31), card(32)],
        hand: vec![card(33)],
        prizes: vec![card(34)],
        discard: vec![card(35)],
        mana: ManaBank {
            matter: 1,
            mind: 2,
            spirit: 3,
        },
        main_losses: if one { 1 } else { 2 },
        enchantments: vec![card(36)],
    }
}

fn every_effect() -> Vec<EffectLeaf> {
    vec![
        EffectLeaf::DealDamage(DamageEffect {
            base: 10,
            constraints: DamageConstraints::from([
                DamageConstraint::Unincreasable,
                DamageConstraint::Unpreventable,
            ]),
            additions: vec![
                DamageAddition {
                    amount: 2,
                    condition: EffectCondition::DefenderEnteredMainThisTurn,
                },
                DamageAddition {
                    amount: 3,
                    condition: EffectCondition::SpellPlayedThisTurn,
                },
            ],
        }),
        EffectLeaf::Heal {
            amount: 4,
            target: EffectTarget::Selected,
        },
        EffectLeaf::MoveSummon,
        EffectLeaf::SwapPositions,
        EffectLeaf::BlockResponses {
            condition: EffectCondition::SpellPlayedThisTurn,
            block: ResponseBlock::AttackSpells,
        },
        EffectLeaf::ReturnSpellFromDiscard,
        EffectLeaf::LookAtPrizes,
        EffectLeaf::DrawCards { amount: 2 },
        EffectLeaf::ReturnSpellToDeckTop,
        EffectLeaf::ProduceMana {
            target: EffectTarget::Source,
        },
        EffectLeaf::CannotBeMovedByOpponent {
            target: EffectTarget::Selected,
        },
        EffectLeaf::ReadySummon,
        EffectLeaf::SwapOpposingPositions,
    ]
}

fn every_stack_item() -> Vec<StackItem> {
    vec![
        StackItem::Attack {
            attacker: PlayerId::One,
            target: Position::Main,
        },
        StackItem::Spell {
            caster: PlayerId::Two,
            card: card(40),
            targets: vec![Position::Bench(BenchSlot::First)],
        },
        StackItem::Trigger {
            controller: PlayerId::One,
            source: Position::Bench(BenchSlot::Second),
            ability: definition(),
            event: TriggerEvent::AnySummonDestroyed,
            targets: vec![Position::Main, Position::Bench(BenchSlot::Third)],
            effects: every_effect(),
        },
    ]
}

fn every_work_item() -> Vec<WorkItem> {
    vec![
        WorkItem::DestructionCheck(Position::Main),
        WorkItem::DiscardDestroyedChain(Position::Bench(BenchSlot::First)),
        WorkItem::RecordMainLoss(PlayerId::One),
        WorkItem::RecoverPrize(PlayerId::Two),
        WorkItem::PromoteBenchSummon(PlayerId::One),
        WorkItem::ResolveMovementConsequences(PlayerId::Two),
        WorkItem::MovementTrigger(MovementStep::LeavingMain, PlayerId::One, Position::Main),
        WorkItem::FireTrigger(
            PlayerId::Two,
            Position::Bench(BenchSlot::Second),
            TriggerEvent::EntersBench,
            definition(),
        ),
        WorkItem::LossCheck(PlayerId::One),
        WorkItem::ReadyAll,
        WorkItem::DrawCard,
        WorkItem::ProduceMana {
            player: PlayerId::Two,
            source: ManaSource::Summon(Position::Bench(BenchSlot::Third)),
        },
        WorkItem::BeginMainPhase,
    ]
}

fn rich_state(cards: Arc<CardSet>) -> GameState {
    GameState {
        players: PerPlayer::new(player(true), player(false)),
        coin: Some(Coin),
        turn: TurnState {
            active_player: PlayerId::Two,
            phase: Phase::Combat,
            window: Some(StackWindow {
                holder: PlayerId::One,
                prior_pass: true,
            }),
            normal_attack_used: true,
            normal_retreat_used: true,
            spell_played_this_turn: PerPlayer::new(true, false),
        },
        stack: every_stack_item(),
        stack_segment_bases: vec![0, 2],
        work: VecDeque::from(every_work_item()),
        pending: Some(PendingInput::ManaProduction {
            player: PlayerId::Two,
            source: ManaSource::Player,
        }),
        status: GameStatus::Broken(Breakage {
            rule: "destruction",
            entity: definition(),
            expected: ComponentKind::Life,
        }),
        cards,
    }
}

fn minimal_projection() -> StateProjectionV1 {
    let player = PlayerStateV1 {
        main: None,
        bench: [None, None, None],
        deck: vec![],
        hand: vec![],
        prizes: vec![],
        discard: vec![],
        mana: ManaBankV1 {
            matter: 0,
            mind: 0,
            spirit: 0,
        },
        main_losses: 0,
        enchantments: vec![],
    };
    StateProjectionV1 {
        players: PlayersV1 {
            one: player.clone(),
            two: player,
        },
        coin: false,
        turn: TurnStateV1 {
            active_player: PlayerIdV1::One,
            phase: PhaseV1::Main,
            window: None,
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: PerPlayerBoolV1 {
                one: false,
                two: false,
            },
        },
        stack: vec![],
        stack_segment_bases: vec![],
        work: vec![],
        pending: None,
        status: GameStatusV1::Playing,
    }
}

#[test]
fn projection_rebuilds_every_state_field_and_reuses_the_card_pool() {
    let cards = Arc::new(CardSet::new(vec![]));
    let original = rich_state(Arc::clone(&cards));

    let rebuilt = StateProjectionV1::from_state(&original)
        .into_game_state(Arc::clone(&cards))
        .expect("the complete projection rebuilds");

    assert_eq!(rebuilt, original);
    assert!(Arc::ptr_eq(&rebuilt.cards, &cards));
}

#[test]
fn simple_identity_and_position_variants_round_trip() {
    for player in [PlayerId::One, PlayerId::Two] {
        assert_eq!(PlayerId::from(PlayerIdV1::from(player)), player);
    }
    for slot in [BenchSlot::First, BenchSlot::Second, BenchSlot::Third] {
        assert_eq!(BenchSlot::from(BenchSlotV1::from(slot)), slot);
    }
    for position in [
        Position::Main,
        Position::Bench(BenchSlot::First),
        Position::Bench(BenchSlot::Second),
        Position::Bench(BenchSlot::Third),
    ] {
        assert_eq!(Position::from(PositionV1::from(position)), position);
    }
}

#[test]
fn every_turn_and_pending_variant_round_trips() {
    for phase in [Phase::Upkeep, Phase::Main, Phase::Combat] {
        assert_eq!(Phase::from(PhaseV1::from(phase)), phase);
    }
    for source in [ManaSource::Player, ManaSource::Summon(Position::Main)] {
        assert_eq!(ManaSource::from(ManaSourceV1::from(source)), source);
    }
    for pending in [
        PendingInput::ManaProduction {
            player: PlayerId::One,
            source: ManaSource::Player,
        },
        PendingInput::Promotion {
            player: PlayerId::Two,
        },
        PendingInput::PrizePick {
            chooser: PlayerId::One,
        },
    ] {
        assert_eq!(PendingInput::from(PendingInputV1::from(pending)), pending);
    }
}

#[test]
fn every_summon_marker_variant_round_trips() {
    for readiness in [Readiness::Ready, Readiness::Exhausted] {
        assert_eq!(Readiness::from(ReadinessV1::from(readiness)), readiness);
    }
    for activity in [
        UpgradeActivity::Available,
        UpgradeActivity::PlayedThisTurn,
        UpgradeActivity::UpgradedThisTurn,
    ] {
        assert_eq!(
            UpgradeActivity::from(UpgradeActivityV1::from(activity)),
            activity
        );
    }
    let marker = DurationMarker::CannotBeMovedByOpponent;
    assert_eq!(DurationMarker::from(DurationMarkerV1::from(marker)), marker);
}

#[test]
fn every_loss_status_and_component_kind_variant_round_trips() {
    for reason in [
        LossReason::ThirdMainLoss,
        LossReason::NoPromotionAvailable,
        LossReason::EmptyDeckDraw,
        LossReason::Resignation,
    ] {
        assert_eq!(LossReason::from(LossReasonV1::from(reason)), reason);
    }
    let statuses = [
        GameStatus::Playing,
        GameStatus::Ended(GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::ThirdMainLoss,
        }),
        GameStatus::Broken(Breakage {
            rule: "retreat",
            entity: definition(),
            expected: ComponentKind::RetreatCost,
        }),
    ];
    for status in statuses {
        assert_eq!(
            GameStatus::try_from(GameStatusV1::from(status)).expect("the status rebuilds"),
            status
        );
    }
    let kinds = [
        ComponentKind::Name,
        ComponentKind::AccountingId,
        ComponentKind::Life,
        ComponentKind::RetreatCost,
        ComponentKind::Form,
        ComponentKind::Produces,
        ComponentKind::Tags,
        ComponentKind::Cost,
        ComponentKind::Skill,
        ComponentKind::Attack,
        ComponentKind::Trigger,
        ComponentKind::Effect,
        ComponentKind::Passive,
        ComponentKind::Timing,
        ComponentKind::Event,
        ComponentKind::Respondable,
        ComponentKind::Persistent,
    ];
    for kind in kinds {
        assert_eq!(ComponentKind::from(ComponentKindV1::from(kind)), kind);
    }
}

#[test]
fn every_trigger_event_and_movement_step_variant_round_trips() {
    for event in [
        TriggerEvent::YourUpkeep,
        TriggerEvent::EntersMain,
        TriggerEvent::EntersBench,
        TriggerEvent::LeavesMain,
        TriggerEvent::LeavesBench,
        TriggerEvent::AnySummonDestroyed,
    ] {
        assert_eq!(TriggerEvent::from(TriggerEventV1::from(event)), event);
    }
    for step in [
        MovementStep::LeavingMain,
        MovementStep::EnteringBench,
        MovementStep::LeavingBench,
        MovementStep::EnteringMain,
    ] {
        assert_eq!(MovementStep::from(MovementStepV1::from(step)), step);
    }
}

#[test]
fn every_effect_leaf_variant_round_trips() {
    for effect in every_effect() {
        assert_eq!(EffectLeaf::from(EffectLeafV1::from(&effect)), effect);
    }
}

#[test]
fn every_stack_item_variant_round_trips() {
    for item in every_stack_item() {
        assert_eq!(
            StackItem::try_from(StackItemV1::from(&item)).expect("the item rebuilds"),
            item
        );
    }
}

#[test]
fn every_work_item_variant_round_trips() {
    for item in every_work_item() {
        assert_eq!(
            WorkItem::try_from(WorkItemV1::from(&item)).expect("the item rebuilds"),
            item
        );
    }
}

#[test]
fn canonical_state_bytes_have_an_exact_vector() {
    let bytes = minimal_projection()
        .canonical_bytes()
        .expect("the projection is serializable");
    assert_eq!(
        String::from_utf8(bytes).expect("canonical JSON is UTF-8"),
        "{\"projection_version\":1,\"state\":{\"players\":{\"one\":{\"main\":null,\"bench\":[null,null,null],\"deck\":[],\"hand\":[],\"prizes\":[],\"discard\":[],\"mana\":{\"matter\":0,\"mind\":0,\"spirit\":0},\"main_losses\":0,\"enchantments\":[]},\"two\":{\"main\":null,\"bench\":[null,null,null],\"deck\":[],\"hand\":[],\"prizes\":[],\"discard\":[],\"mana\":{\"matter\":0,\"mind\":0,\"spirit\":0},\"main_losses\":0,\"enchantments\":[]}},\"coin\":false,\"turn\":{\"active_player\":\"one\",\"phase\":\"main\",\"window\":null,\"normal_attack_used\":false,\"normal_retreat_used\":false,\"spell_played_this_turn\":{\"one\":false,\"two\":false}},\"stack\":[],\"stack_segment_bases\":[],\"work\":[],\"pending\":null,\"status\":{\"kind\":\"playing\"}}}"
    );
}

#[test]
fn state_digest_has_an_exact_sha_256_vector() {
    let digest =
        StateDigestV1::compute(&minimal_projection()).expect("the projection is serializable");
    assert_eq!(
        digest,
        StateDigestV1(
            "sha256:814c2a07f135c91346400e5196fb5b820e2b09f60180a925c3865ef25c906520".to_string()
        )
    );
}

#[test]
fn rebuilding_rejects_an_empty_upgrade_chain() {
    let error = UpgradeChain::try_from(UpgradeChainV1 { layers: vec![] })
        .expect_err("a chain needs a base");
    assert_eq!(error, StateRebuildError::EmptyUpgradeChain);
}

#[test]
fn rebuilding_rejects_an_invalid_entity_id() {
    let error =
        EntityId::try_from(EntityIdV1("not-a-uuid".to_string())).expect_err("the ID is invalid");
    assert_eq!(
        error,
        StateRebuildError::InvalidEntityId {
            value: "not-a-uuid".to_string()
        }
    );
}

#[test]
fn rebuilding_rejects_an_unknown_breakage_rule() {
    let error = GameStatus::try_from(GameStatusV1::Broken {
        breakage: BreakageV1 {
            rule: "unknown".to_string(),
            entity: definition().into(),
            expected: ComponentKindV1::Life,
        },
    })
    .expect_err("the rule cannot produce a static core value");
    assert_eq!(
        error,
        StateRebuildError::UnsupportedBreakageRule {
            rule: "unknown".to_string()
        }
    );
}
