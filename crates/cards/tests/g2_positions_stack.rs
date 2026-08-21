#![allow(clippy::expect_used)]

mod support;

use summoners_cards::{CardLibrary, built_in_catalog};
use summoners_core::{
    domain::{
        actions::GameAction,
        cards::{
            Attack, DamageConstraint, DamageConstraints, EntityId, Skill, Trigger, TriggerEvent,
        },
        errors::ActionError,
        events::{
            BattlefieldTarget, DamageContext, DamageOperation, DamageOrigin, DamageSource,
            DamageStage, GameEvent,
        },
        ids::{BenchSlot, ManaType, PlayerId, Position},
        state::{
            CardRef, DurationMarker, GameState, ManaBank, ManaSource, Phase, Readiness, StackItem,
        },
    },
    engine::apply::apply,
    scenario::ScenarioSummon,
};

use support::{PhysicalCards, player, state};

const WARD: &str = "foundations/standing-ward";
const EMBER_LANCE: &str = "foundations/ember-lance";

struct G2 {
    state: GameState,
    warden_skill: EntityId,
    guard_skill: EntityId,
    sow_skill: EntityId,
    grazer_trigger: EntityId,
    hearth_trigger: EntityId,
    guard_trigger: EntityId,
    tender_trigger: EntityId,
    adept_attack: EntityId,
    sow_attack: EntityId,
    tender_attack: EntityId,
    one_ember: CardRef,
    one_scrying: CardRef,
    one_wards: [CardRef; 2],
    two_ember: CardRef,
    two_balm: CardRef,
    two_wards: [CardRef; 2],
}

fn take(cards: &mut Vec<CardRef>, library: &CardLibrary, qualified_key: &str) -> CardRef {
    let definition = library
        .card_id(qualified_key)
        .unwrap_or_else(|| panic!("real definition must exist: {qualified_key}"));
    let index = cards
        .iter()
        .position(|card| card.def == definition)
        .unwrap_or_else(|| panic!("resolved Deck must contain: {qualified_key}"));
    cards.remove(index)
}

fn chain(cards: impl IntoIterator<Item = CardRef>, damage: u32) -> ScenarioSummon {
    ScenarioSummon {
        chain: cards.into_iter().collect(),
        damage,
        readiness: Readiness::Ready,
    }
}

fn one_card(cards: &GameState, qualified_key: &str, library: &CardLibrary) -> EntityId {
    let id = library
        .card_id(qualified_key)
        .unwrap_or_else(|| panic!("real definition must exist: {qualified_key}"));
    cards
        .cards
        .get(id)
        .expect("definition must be in GameState")
        .id
}

fn only_skill(cards: &GameState, definition: EntityId) -> EntityId {
    let entity = cards.cards.get(definition).expect("card must exist");
    let skills = entity.all::<Skill>();
    assert_eq!(skills.len(), 1, "the selected printed card has one Skill");
    skills[0].id
}

fn attack_id(cards: &GameState, definition: EntityId) -> EntityId {
    cards
        .cards
        .get(definition)
        .expect("card must exist")
        .get::<Attack>()
        .expect("Summon must print an Attack")
        .id
}

fn only_trigger(cards: &GameState, definition: EntityId) -> EntityId {
    let entity = cards.cards.get(definition).expect("card must exist");
    let triggers = entity.all::<Trigger>();
    assert_eq!(
        triggers.len(),
        1,
        "the selected printed card has one Trigger"
    );
    triggers[0].id
}

fn seeded_g2() -> G2 {
    let catalog = built_in_catalog().expect("built-in catalog is valid");
    let library = catalog.library();
    let mut physical = PhysicalCards::new(library, 1_000);
    let mut one_deck = physical.deck(catalog.set_paths());
    let mut two_deck = physical.deck(catalog.barrow_herd());

    let one_pathkeeper = take(&mut one_deck, library, "foundations/warden-pathkeeper");
    let one_warden = take(&mut one_deck, library, "foundations/warden-of-set-paths");
    let one_guard = take(&mut one_deck, library, "foundations/quarry-warden-guard");
    let one_tender = take(&mut one_deck, library, "foundations/quarry-well-tender");
    let one_adept = take(&mut one_deck, library, "foundations/set-path-adept");
    let one_ember = take(&mut one_deck, library, EMBER_LANCE);
    let one_scrying = take(&mut one_deck, library, "foundations/scrying-glass");
    let one_wards = [
        take(&mut one_deck, library, WARD),
        take(&mut one_deck, library, WARD),
    ];

    let two_matriarch = take(&mut two_deck, library, "foundations/sow-matriarch");
    let two_sow = take(&mut two_deck, library, "foundations/old-sow-of-the-barrow");
    let two_hearth = take(&mut two_deck, library, "foundations/hearth-warden");
    let two_grazer = take(&mut two_deck, library, "foundations/barrow-grazer");
    let two_balm = take(&mut two_deck, library, "foundations/renewing-balm");
    let two_ember = take(&mut two_deck, library, EMBER_LANCE);
    let two_wards = [
        take(&mut two_deck, library, WARD),
        take(&mut two_deck, library, WARD),
    ];

    let mut one = player(chain(
        [
            physical.one("foundations/warden-initiate"),
            one_pathkeeper,
            one_warden,
        ],
        10,
    ));
    one.bench = [
        Some(chain([one_guard], 20)),
        Some(chain([one_adept], 0)),
        Some(chain([one_tender], 0)),
    ];
    one.hand = vec![one_ember, one_scrying, one_wards[0], one_wards[1]];
    one.deck = one_deck;
    one.mana = ManaBank {
        matter: 20,
        mind: 20,
        spirit: 20,
    };

    let mut two = player(chain([two_grazer], 20));
    two.bench = [
        Some(chain([two_hearth], 30)),
        Some(chain(
            [
                physical.one("foundations/sow-piglet"),
                two_matriarch,
                two_sow,
            ],
            60,
        )),
        None,
    ];
    two.hand = vec![two_ember, two_balm, two_wards[0], two_wards[1]];
    two.deck = two_deck;
    two.mana = ManaBank {
        matter: 20,
        mind: 20,
        spirit: 20,
    };

    let state = state(&catalog, one, two, PlayerId::One).expect("G2 seed is valid");
    let warden = one_card(&state, "foundations/warden-of-set-paths", library);
    let guard = one_card(&state, "foundations/quarry-warden-guard", library);
    let adept = one_card(&state, "foundations/set-path-adept", library);
    let tender = one_card(&state, "foundations/quarry-well-tender", library);
    let sow = one_card(&state, "foundations/old-sow-of-the-barrow", library);
    let grazer = one_card(&state, "foundations/barrow-grazer", library);
    let hearth = one_card(&state, "foundations/hearth-warden", library);

    G2 {
        warden_skill: only_skill(&state, warden),
        guard_skill: only_skill(&state, guard),
        sow_skill: only_skill(&state, sow),
        grazer_trigger: only_trigger(&state, grazer),
        hearth_trigger: only_trigger(&state, hearth),
        guard_trigger: only_trigger(&state, guard),
        tender_trigger: only_trigger(&state, tender),
        adept_attack: attack_id(&state, adept),
        sow_attack: attack_id(&state, sow),
        tender_attack: attack_id(&state, tender),
        state,
        one_ember,
        one_scrying,
        one_wards,
        two_ember,
        two_balm,
        two_wards,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn step(state: &mut GameState, action: GameAction) -> Vec<GameEvent> {
    let outcome = apply(state, &action).expect("G2 action must be legal");
    *state = outcome.state;
    outcome.events
}

fn pass(state: &mut GameState, player: PlayerId) -> Vec<GameEvent> {
    step(state, GameAction::PassPriority { player })
}

fn cast(
    state: &mut GameState,
    player: PlayerId,
    card: CardRef,
    targets: Vec<Position>,
) -> Vec<GameEvent> {
    step(
        state,
        GameAction::CastSpell {
            player,
            card: card.instance,
            targets,
            mana_hint: Some(ManaType::Matter),
        },
    )
}

fn finish_turn(state: &mut GameState, player: PlayerId, mana_type: ManaType) -> Vec<GameEvent> {
    let mut events = step(state, GameAction::EndTurn { player });
    events.extend(pass(state, player.opponent()));
    events.extend(pass(state, player));
    if state.pending.is_some() {
        events.extend(step(
            state,
            GameAction::ChooseManaType {
                player: player.opponent(),
                mana_type,
            },
        ));
    }
    events
}

fn resolved_items(events: &[GameEvent]) -> Vec<StackItem> {
    events
        .iter()
        .filter_map(|event| match event {
            GameEvent::StackItemResolved { item } => Some(item.clone()),
            _ => None,
        })
        .collect()
}

fn damage_trace(events: &[GameEvent], context: DamageContext) -> Vec<GameEvent> {
    events
        .iter()
        .filter(|event| match event {
            GameEvent::DamageCalculationStarted { context: found, .. }
            | GameEvent::DamageAdjustmentApplied { context: found, .. }
            | GameEvent::DamageAdjustmentSkipped { context: found, .. }
            | GameEvent::DamageApplied { context: found, .. } => *found == context,
            _ => false,
        })
        .cloned()
        .collect()
}

#[test]
fn g2_positions_and_stack_follow_one_public_action_path() {
    let mut g2 = seeded_g2();

    let moved = step(
        &mut g2.state,
        GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Main,
            ability: g2.warden_skill,
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    );
    assert_eq!(
        moved,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Mind,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Main,
                ability: g2.warden_skill,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::Two,
                main: BenchSlot::First,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Bench(BenchSlot::First),
                event: TriggerEvent::EntersBench,
                ability: g2.grazer_trigger,
            },
            GameEvent::Healed {
                position: Position::Bench(BenchSlot::First),
                amount: 10,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::Two,
                position: Position::Main,
                event: TriggerEvent::EntersMain,
                ability: g2.hearth_trigger,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 15,
            },
        ]
    );

    let own_swap = step(
        &mut g2.state,
        GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: g2.guard_skill,
            targets: vec![Position::Bench(BenchSlot::Second)],
            mana_hint: None,
        },
    );
    assert_eq!(
        own_swap,
        vec![
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                ability: g2.guard_skill,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::Second,
            },
        ]
    );

    step(
        &mut g2.state,
        GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    );
    cast(
        &mut g2.state,
        PlayerId::Two,
        g2.two_wards[0],
        vec![Position::Main],
    );
    cast(
        &mut g2.state,
        PlayerId::One,
        g2.one_ember,
        vec![Position::Main],
    );
    cast(
        &mut g2.state,
        PlayerId::Two,
        g2.two_wards[1],
        vec![Position::Main],
    );
    cast(&mut g2.state, PlayerId::One, g2.one_scrying, vec![]);

    let protected = g2.state.clone();
    assert_eq!(
        apply(
            &g2.state,
            &GameAction::CastSpell {
                player: PlayerId::Two,
                card: g2.two_ember.instance,
                targets: vec![Position::Main],
                mana_hint: Some(ManaType::Matter),
            },
        ),
        Err(ActionError::WrongPhase),
        "Adept blocks the Attack Spell even with a Support Spell above its Attack"
    );
    assert_eq!(g2.state, protected, "a blocked response changes no state");

    cast(
        &mut g2.state,
        PlayerId::Two,
        g2.two_balm,
        vec![Position::Main],
    );
    cast(
        &mut g2.state,
        PlayerId::One,
        g2.one_wards[0],
        vec![Position::Main],
    );
    pass(&mut g2.state, PlayerId::Two);
    cast(
        &mut g2.state,
        PlayerId::One,
        g2.one_wards[1],
        vec![Position::Main],
    );
    pass(&mut g2.state, PlayerId::Two);
    let resolved = pass(&mut g2.state, PlayerId::One);

    assert_eq!(
        resolved_items(&resolved),
        vec![
            StackItem::Spell {
                caster: PlayerId::One,
                card: g2.one_wards[1],
                targets: vec![Position::Main],
            },
            StackItem::Spell {
                caster: PlayerId::One,
                card: g2.one_wards[0],
                targets: vec![Position::Main],
            },
            StackItem::Spell {
                caster: PlayerId::Two,
                card: g2.two_balm,
                targets: vec![Position::Main],
            },
            StackItem::Spell {
                caster: PlayerId::One,
                card: g2.one_scrying,
                targets: vec![],
            },
            StackItem::Spell {
                caster: PlayerId::Two,
                card: g2.two_wards[1],
                targets: vec![Position::Main],
            },
            StackItem::Spell {
                caster: PlayerId::One,
                card: g2.one_ember,
                targets: vec![Position::Main],
            },
            StackItem::Spell {
                caster: PlayerId::Two,
                card: g2.two_wards[0],
                targets: vec![Position::Main],
            },
            StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            },
        ],
        "responses resolve last in, first out before the protected Attack"
    );

    let adept_context = DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability: g2.adept_attack,
        },
        target: BattlefieldTarget {
            controller: PlayerId::Two,
            position: Position::Main,
        },
    };
    assert_eq!(
        damage_trace(&resolved, adept_context),
        vec![
            GameEvent::DamageCalculationStarted {
                context: adept_context,
                base: 20,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context: adept_context,
                stage: DamageStage::Addition,
                operation: DamageOperation::Add(20),
                origin: DamageOrigin::PrintedAbility(g2.adept_attack),
                input: 20,
                output: 40,
            },
            GameEvent::DamageAdjustmentApplied {
                context: adept_context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(g2.two_wards[1].instance),
                input: 40,
                output: 30,
            },
            GameEvent::DamageAdjustmentApplied {
                context: adept_context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(g2.two_wards[0].instance),
                input: 30,
                output: 20,
            },
            GameEvent::DamageAdjustmentApplied {
                context: adept_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(g2.adept_attack),
                input: 20,
                output: 20,
            },
            GameEvent::DamageApplied {
                context: adept_context,
                amount: 20,
                before: 0,
                after: 20,
            },
        ]
    );
    assert_eq!(
        g2.state.players.one.enchantments,
        vec![g2.one_wards[1], g2.one_wards[0]]
    );
    assert_eq!(
        g2.state.players.two.enchantments,
        vec![g2.two_wards[1], g2.two_wards[0]]
    );

    finish_turn(&mut g2.state, PlayerId::One, ManaType::Spirit);
    let retreat = step(
        &mut g2.state,
        GameAction::Retreat {
            player: PlayerId::Two,
            slot: BenchSlot::Second,
            mana_hint: Some(ManaType::Spirit),
        },
    );
    assert_eq!(
        retreat,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Spirit,
                amount: 2,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::Two,
                main: BenchSlot::Second,
            },
        ],
        "the Warden adds exactly one to Hearth Warden's printed Retreat"
    );

    let sow_skill = step(
        &mut g2.state,
        GameAction::ActivateSkill {
            player: PlayerId::Two,
            position: Position::Main,
            ability: g2.sow_skill,
            targets: vec![],
            mana_hint: None,
        },
    );
    assert_eq!(
        sow_skill,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::ManaDeducted {
                player: PlayerId::Two,
                mana_type: ManaType::Spirit,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::Two,
                position: Position::Main,
                ability: g2.sow_skill,
            },
            GameEvent::Healed {
                position: Position::Main,
                amount: 30,
            },
        ],
        "the source-bound protection is applied without an extra event"
    );
    assert!(
        g2.state
            .players
            .two
            .main
            .as_ref()
            .expect("Old Sow")
            .duration_markers
            .contains(&DurationMarker::CannotBeMovedByOpponent)
    );

    step(
        &mut g2.state,
        GameAction::DeclareAttack {
            player: PlayerId::Two,
            target: Position::Main,
            mana_hint: Some(ManaType::Spirit),
        },
    );
    pass(&mut g2.state, PlayerId::One);
    let sow_resolved = pass(&mut g2.state, PlayerId::Two);
    let sow_context = DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::Two,
            position: Position::Main,
            ability: g2.sow_attack,
        },
        target: BattlefieldTarget {
            controller: PlayerId::One,
            position: Position::Main,
        },
    };
    assert_eq!(
        damage_trace(&sow_resolved, sow_context),
        vec![
            GameEvent::DamageCalculationStarted {
                context: sow_context,
                base: 70,
                constraints: DamageConstraints::from([
                    DamageConstraint::Unpreventable,
                    DamageConstraint::Unincreasable,
                ]),
            },
            GameEvent::DamageAdjustmentSkipped {
                context: sow_context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(g2.one_wards[1].instance),
                input: 70,
                constraint: DamageConstraint::Unpreventable,
            },
            GameEvent::DamageAdjustmentSkipped {
                context: sow_context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(g2.one_wards[0].instance),
                input: 70,
                constraint: DamageConstraint::Unpreventable,
            },
            GameEvent::DamageAdjustmentApplied {
                context: sow_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(g2.sow_attack),
                input: 70,
                output: 70,
            },
            GameEvent::DamageApplied {
                context: sow_context,
                amount: 70,
                before: 0,
                after: 70,
            },
        ]
    );
    step(
        &mut g2.state,
        GameAction::ChoosePromotion {
            player: PlayerId::One,
            slot: BenchSlot::Second,
        },
    );

    finish_turn(&mut g2.state, PlayerId::Two, ManaType::Matter);
    let protected = g2.state.clone();
    assert_eq!(
        apply(
            &g2.state,
            &GameAction::ActivateSkill {
                player: PlayerId::One,
                position: Position::Main,
                ability: g2.warden_skill,
                targets: vec![Position::Bench(BenchSlot::First)],
                mana_hint: None,
            },
        ),
        Err(ActionError::InvalidTarget)
    );
    assert_eq!(
        g2.state, protected,
        "movement protection rejects without change"
    );

    let guard_to_main = step(
        &mut g2.state,
        GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: g2.guard_skill,
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    );
    assert_eq!(
        guard_to_main,
        vec![
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                ability: g2.guard_skill,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::First,
            },
        ]
    );
    let moved_own = step(
        &mut g2.state,
        GameAction::Retreat {
            player: PlayerId::One,
            slot: BenchSlot::Third,
            mana_hint: Some(ManaType::Matter),
        },
    );
    assert_eq!(
        moved_own,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::One,
                main: BenchSlot::Third,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Bench(BenchSlot::Third),
                event: TriggerEvent::LeavesMain,
                ability: g2.guard_trigger,
            },
            GameEvent::Healed {
                position: Position::Bench(BenchSlot::Third),
                amount: 10,
            },
            GameEvent::TriggerFired {
                controller: PlayerId::One,
                position: Position::Main,
                event: TriggerEvent::LeavesBench,
                ability: g2.tender_trigger,
            },
        ],
        "movement triggers fire once, immediately, and in printed order"
    );
    assert!(
        g2.state.pending.is_some(),
        "dual-type Tender requests its Mana type"
    );
    let produced = step(
        &mut g2.state,
        GameAction::ChooseManaType {
            player: PlayerId::One,
            mana_type: ManaType::Mind,
        },
    );
    assert_eq!(
        produced,
        vec![GameEvent::ManaProduced {
            player: PlayerId::One,
            source: ManaSource::Summon(Position::Main),
            mana_type: ManaType::Mind,
        }],
        "the immediate LeavesBench effect keeps its controller and source"
    );
    assert!(g2.state.turn.window.is_none());

    finish_turn(&mut g2.state, PlayerId::One, ManaType::Spirit);
    assert!(
        !g2.state
            .players
            .two
            .main
            .as_ref()
            .expect("Old Sow")
            .duration_markers
            .contains(&DurationMarker::CannotBeMovedByOpponent),
        "the protection expires only when Old Sow's controller starts their next turn"
    );
    finish_turn(&mut g2.state, PlayerId::Two, ManaType::Matter);

    let after_expiry = step(
        &mut g2.state,
        GameAction::ActivateSkill {
            player: PlayerId::One,
            position: Position::Bench(BenchSlot::First),
            ability: g2.warden_skill,
            targets: vec![Position::Bench(BenchSlot::First)],
            mana_hint: None,
        },
    );
    assert_eq!(
        after_expiry,
        vec![
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Matter,
                amount: 1,
            },
            GameEvent::ManaDeducted {
                player: PlayerId::One,
                mana_type: ManaType::Mind,
                amount: 1,
            },
            GameEvent::SkillActivated {
                player: PlayerId::One,
                position: Position::Bench(BenchSlot::First),
                ability: g2.warden_skill,
            },
            GameEvent::SummonsSwapped {
                player: PlayerId::Two,
                main: BenchSlot::First,
            },
        ],
        "the same Warden move resolves exactly once after protection expires"
    );

    assert_eq!(g2.state.turn.phase, Phase::Main);
    let declared = step(
        &mut g2.state,
        GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    );
    assert_eq!(
        declared,
        vec![GameEvent::AttackDeclared {
            player: PlayerId::One,
            target: Position::Main,
        }]
    );
    assert_eq!(
        pass(&mut g2.state, PlayerId::Two),
        vec![GameEvent::PriorityPassed {
            player: PlayerId::Two,
        }]
    );
    let final_combat = pass(&mut g2.state, PlayerId::One);
    let tender_context = DamageContext {
        source: DamageSource::Attack {
            controller: PlayerId::One,
            position: Position::Main,
            ability: g2.tender_attack,
        },
        target: BattlefieldTarget {
            controller: PlayerId::Two,
            position: Position::Main,
        },
    };
    assert_eq!(
        final_combat,
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
                context: tender_context,
                base: 10,
                constraints: DamageConstraints::new(),
            },
            GameEvent::DamageAdjustmentApplied {
                context: tender_context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(g2.two_wards[1].instance),
                input: 10,
                output: 0,
            },
            GameEvent::DamageAdjustmentApplied {
                context: tender_context,
                stage: DamageStage::PersistentReduction,
                operation: DamageOperation::Reduce(10),
                origin: DamageOrigin::PersistentCard(g2.two_wards[0].instance),
                input: 0,
                output: 0,
            },
            GameEvent::DamageAdjustmentApplied {
                context: tender_context,
                stage: DamageStage::Clamp,
                operation: DamageOperation::ClampToZero,
                origin: DamageOrigin::PrintedAbility(g2.tender_attack),
                input: 0,
                output: 0,
            },
            GameEvent::DamageApplied {
                context: tender_context,
                amount: 0,
                before: 10,
                after: 10,
            },
        ],
        "final Combat has no missing, extra, reordered, or wrong-source event"
    );
    assert_eq!(g2.state.turn.phase, Phase::Combat);
    assert!(g2.state.stack.is_empty());
    assert_eq!(
        g2.state
            .players
            .two
            .main
            .as_ref()
            .expect("Barrow Grazer")
            .damage,
        10
    );
    assert_eq!(
        g2.state.players.two.bench[0]
            .as_ref()
            .expect("Old Sow")
            .damage,
        10
    );
    assert!(g2.state.pending.is_none());
}
