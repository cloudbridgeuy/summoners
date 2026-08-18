//! `GameState`: one plain, cloneable value that is authoritative for the
//! whole match. Everything the engine does is a pure function from one
//! `GameState` plus one `GameAction` to the next `GameState` plus a batch of
//! `GameEvent`s (decision 3).

use std::collections::VecDeque;
use std::sync::Arc;

use crate::domain::cards::{Breakage, CardSet, EffectLeaf, EntityId, TriggerEvent};
use crate::domain::ids::{CardInstanceId, PlayerId, Position};

/// A card reference in a non-battlefield zone: the specific instance and the
/// card it prints. Deck, hand, prizes, and discard all hold these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardRef {
    pub instance: CardInstanceId,
    pub def: EntityId,
}

/// A Summon's upgrade chain, guaranteed non-empty by construction: the
/// engine can never represent a Summon with no printed characteristics
/// (rules §20; make impossible states impossible). Only `scenario::parse`
/// and later handlers build one, after validating every layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeChain {
    base: CardRef,
    rest: Vec<CardRef>,
}

impl UpgradeChain {
    /// Build a chain from its base card upward. `rest` holds any upgrade
    /// layers stacked above the base, in play order.
    pub fn new(base: CardRef, rest: Vec<CardRef>) -> Self {
        Self { base, rest }
    }

    /// The bottommost, original card.
    pub fn base(&self) -> CardRef {
        self.base
    }

    /// The topmost card: it alone defines the Summon's current Life, Mana
    /// Types, Retreat Cost, Skills, and other characteristics (rules §20).
    pub fn top(&self) -> CardRef {
        self.rest.last().copied().unwrap_or(self.base)
    }

    /// Every layer, base first.
    pub fn layers(&self) -> impl Iterator<Item = &CardRef> {
        std::iter::once(&self.base).chain(self.rest.iter())
    }
}

/// A duration marker attached to a Summon until its stated condition ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationMarker {
    /// The Old Sow's `Root and Renew`: immune to opponent-driven movement
    /// until its controller's next turn.
    CannotBeMovedByOpponent,
}

/// A Summon's mutually exclusive upgrade activity during the current turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeActivity {
    /// The Summon can be upgraded this turn.
    Available,
    /// The Summon entered play this turn and cannot be upgraded yet.
    PlayedThisTurn,
    /// The Summon already received its one upgrade for this turn.
    UpgradedThisTurn,
}

/// Proof that a Summon entered Main during the current turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnteredMain;

/// The independent per-turn facts held by one Summon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SummonTurnRecord {
    pub upgrade: UpgradeActivity,
    pub main_entry: Option<EnteredMain>,
}

impl SummonTurnRecord {
    /// Start a turn with upgrade activity available and no Main entry.
    pub const fn fresh() -> Self {
        Self {
            upgrade: UpgradeActivity::Available,
            main_entry: None,
        }
    }
}

/// One Summon in play: its upgrade chain, accumulated Damage, Ready state,
/// who owns and who controls it, any duration markers, and the per-turn
/// record that gates upgrading and records Main entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonInstance {
    pub chain: UpgradeChain,
    pub damage: u32,
    pub ready: bool,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub duration_markers: Vec<DurationMarker>,
    pub turn: SummonTurnRecord,
}

/// The three typed Mana pools a player has banked (rules §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ManaBank {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
}

/// The second player's one-use resource (rules §7).
///
/// The marker carries no data. Its presence at the game root means Player
/// Two holds it; its absence means it has left the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coin;

/// One player's complete zones and resources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    pub main: Option<SummonInstance>,
    pub bench: [Option<SummonInstance>; 3],
    /// Ordered top to bottom; drawing removes from the front.
    pub deck: Vec<CardRef>,
    pub hand: Vec<CardRef>,
    pub prizes: Vec<CardRef>,
    pub discard: Vec<CardRef>,
    pub mana: ManaBank,
    /// Rules §2: a player loses on their third Main Summon loss.
    pub main_losses: u8,
    /// Rules §44: Enchantments this player has cast, still in play. Cleared
    /// only by an effect that removes one; `scenario::from_scenario` cannot
    /// seed a starting Enchantment yet — `Scenario` carries no field for it.
    pub enchantments: Vec<CardRef>,
}

/// A homogeneous pair, one value per player. Every read or write goes
/// through `PlayerId`, so a caller can never mix up the seats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerPlayer<T> {
    pub one: T,
    pub two: T,
}

impl<T> PerPlayer<T> {
    pub fn new(one: T, two: T) -> Self {
        Self { one, two }
    }

    pub fn get(&self, player: PlayerId) -> &T {
        match player {
            PlayerId::One => &self.one,
            PlayerId::Two => &self.two,
        }
    }

    pub fn get_mut(&mut self, player: PlayerId) -> &mut T {
        match player {
            PlayerId::One => &mut self.one,
            PlayerId::Two => &mut self.two,
        }
    }
}

/// Who holds Priority in an open window, and whether the last action was a
/// pass (rules §31–33). A second consecutive pass with `prior_pass` set
/// starts Stack resolution; any played effect clears it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackWindow {
    pub holder: PlayerId,
    pub prior_pass: bool,
}

/// The turn's resting shape (rules §9): `Upkeep` rests only while a
/// production choice is pending, `Main` is free play, `Combat` is after an
/// attack has been declared. A Priority window can open during any of the
/// three (a Spell cast proactively in Main, a respondable trigger during
/// Upkeep or Main resolution, or a declared attack in Combat), so the
/// window lives on `TurnState` as its own field rather than as a payload of
/// one phase — the phase to rest in after the window closes is never lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Upkeep,
    Main,
    Combat,
}

/// The active player, the current phase, any open Priority window, and the
/// per-turn flags (rules §26, §29, and the Griefsinger's conditional attack
/// bonus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnState {
    pub active_player: PlayerId,
    pub phase: Phase,
    /// Set while a Priority window is open (design's "Phases and Priority":
    /// a declared attack, an attackless end of turn, a proactive Spell
    /// cast, or a respondable trigger mid-resolution). `None` when nobody
    /// currently holds Priority.
    pub window: Option<StackWindow>,
    pub normal_attack_used: bool,
    pub normal_retreat_used: bool,
    /// Set for the rest of the turn once any Spell is cast. Read by a
    /// conditional attack bonus that checks whether its controller played a
    /// Spell this turn; cleared when the turn ends.
    pub spell_played_this_turn: bool,
}

/// Where a Mana-production choice comes from: the player's own natural
/// production (anchored to every Summon they control) or one specific
/// Summon's own printed production (rules §11–12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaSource {
    Player,
    Summon(Position),
}

/// A paused decision, held as plain data rather than a closure (decision
/// 2). While set, the actor gate accepts only the named player's matching
/// answer (decision 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInput {
    ManaProduction {
        player: PlayerId,
        source: ManaSource,
    },
    Promotion {
        player: PlayerId,
    },
    PrizePick {
        chooser: PlayerId,
    },
}

/// Why a player lost (rules §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossReason {
    ThirdMainLoss,
    NoPromotionAvailable,
    EmptyDeckDraw,
}

/// The match's final result, set once. Every action after this point is
/// rejected with `ActionError::GameAlreadyOver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameOutcome {
    pub winner: PlayerId,
    pub reason: LossReason,
}

/// Where the match currently stands. `Playing` is the only status under
/// which an action can be accepted; `Ended` and `Broken` are both terminal,
/// through the same one field — a match cannot be both won and broken at
/// once, and a single field makes that impossible to represent rather than
/// merely undesirable. `Ended` carries the same `GameOutcome` the field it
/// replaced used to hold, so the winner and the reason are never lost.
/// `Broken` carries the `Breakage` a failed `Entity::demand` produced:
/// which rule demanded, which entity it asked, and which component it
/// expected and did not find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameStatus {
    Playing,
    Ended(GameOutcome),
    Broken(Breakage),
}

impl GameStatus {
    /// Whether the match can still accept and resolve an action. `false`
    /// once the match has ended in a win or broken on a demanded fact no
    /// entity printed — both stop the resolution loop and the actor gate
    /// the same way.
    pub fn is_playing(&self) -> bool {
        matches!(self, GameStatus::Playing)
    }
}

/// One entry on the last-in-first-out Stack: an attack or a Spell effect
/// (rules §31, §34).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StackItem {
    Attack {
        attacker: PlayerId,
        target: Position,
    },
    Spell {
        caster: PlayerId,
        card: CardRef,
        targets: Vec<Position>,
    },
    /// Rules §37–38: a respondable Triggered Ability, waiting to resolve
    /// like any other Stack entry.
    Trigger {
        controller: PlayerId,
        source: Position,
        event: TriggerEvent,
        targets: Vec<Position>,
        effects: Vec<EffectLeaf>,
    },
}

/// The fixed order movement triggers resolve in whenever Main and a Bench
/// Summon exchange positions (rules §28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementStep {
    LeavingMain,
    EnteringBench,
    LeavingBench,
    EnteringMain,
}

/// One deterministic consequence waiting to run. `work` is a queue, separate
/// from the Stack; the resolution loop drains it before it ever looks at the
/// Stack (see the design's resolution loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkItem {
    /// Rules §23: has this position's accumulated Damage reached its Life?
    DestructionCheck(Position),
    /// Rules §24 step 1: discard the destroyed Summon and its chain.
    DiscardDestroyedChain(Position),
    /// Rules §24 step 2: record one Main Summon loss.
    RecordMainLoss(PlayerId),
    /// Rules §24 step 3, §25: recover one Prize; the opponent chooses which.
    RecoverPrize(PlayerId),
    /// Rules §24 step 4: promote a Benched Summon to the empty Main.
    PromoteBenchSummon(PlayerId),
    /// Rules §24 step 5: resolve the consequences of that promotion.
    ResolveMovementConsequences(PlayerId),
    /// Rules §28: one step of the fixed movement-trigger order, for the
    /// Summon this player controls at `Position`.
    MovementTrigger(MovementStep, PlayerId, Position),
    /// Rules §36–38: a Triggered Ability fires for the Summon this player
    /// controls at `Position`. Names the ability by its `EntityId`, since a
    /// card may print more than one Trigger matching the same `TriggerEvent`
    /// — one `FireTrigger` item is queued per matching ability, so the item
    /// must say which one it means.
    FireTrigger(PlayerId, Position, TriggerEvent, EntityId),
    /// Rules §24 step 6, §2: check whether this player has now lost.
    LossCheck(PlayerId),
    /// Rules §10: ready every Summon the new active player controls.
    ReadyAll,
    /// Rules §10: draw one card; an empty deck is checked via `LossCheck`.
    DrawCard,
    /// Rules §10–12: generate Mana from this source; may set `pending`.
    ProduceMana(ManaSource),
    /// Rules §9: once every other Upkeep step has drained, the turn moves
    /// forward into the Main Phase on its own — phases only move forward,
    /// so nothing else ever leaves `Phase::Upkeep`. Queued last by
    /// `engine::turn::handover`, after `ReadyAll`, any `YourUpkeep`
    /// triggers, `DrawCard`, and `ProduceMana(ManaSource::Player)`, so it
    /// always lands after a `ManaProduction` pause and its answer too — the
    /// drain loop resumes the same queue where it left off once `pending`
    /// clears.
    BeginMainPhase,
}

/// One plain, cloneable value: the whole match. The same state and the same
/// action always produce the same outcome (the design's transition
/// contract).
///
/// `PartialEq`/`Eq` are hand-written, not derived: every field but
/// `cards` compares by value, as `#[derive]` would; `cards` compares by
/// handle (`Arc::ptr_eq`) instead. A `CardSet` has no meaningful notion of
/// content equality here — two states sharing one authored card pool are the
/// same game shape, and comparing millions of bytes of card data on every
/// state comparison would be wasteful even if it were defined. Test
/// fixtures that want two independently built `GameState`s to compare equal
/// must share one `Arc<CardSet>` handle (see `cards::fixtures::card_set`).
#[derive(Debug, Clone)]
pub struct GameState {
    pub players: PerPlayer<PlayerState>,
    /// `Some(Coin)` means Player Two holds the Coin. `None` means the Coin
    /// has left the game (rules §7).
    pub coin: Option<Coin>,
    pub turn: TurnState,
    pub stack: Vec<StackItem>,
    /// Segment base indices into `stack` (design's "Phases and Priority").
    /// When a respondable trigger creates a Stack effect mid-drain, its
    /// index in `stack` is pushed here. Responses build above that index; a
    /// double pass drains only down to it, then it pops and the drain that
    /// was interrupted resumes below it. Empty outside a mid-drain segment;
    /// a `Vec` because a trigger can itself land while another segment is
    /// still open, nesting one base above the last. No behavior reads or
    /// writes this yet — it is storage only.
    pub stack_segment_bases: Vec<usize>,
    pub work: VecDeque<WorkItem>,
    pub pending: Option<PendingInput>,
    pub status: GameStatus,
    /// The authored card pool this match reads facts from. Never
    /// mutated after `scenario::from_scenario` builds the state; every
    /// holder of the same `Arc` sees the same cards.
    pub cards: Arc<CardSet>,
}

impl PartialEq for GameState {
    fn eq(&self, other: &Self) -> bool {
        self.players == other.players
            && self.coin == other.coin
            && self.turn == other.turn
            && self.stack == other.stack
            && self.stack_segment_bases == other.stack_segment_bases
            && self.work == other.work
            && self.pending == other.pending
            && self.status == other.status
            && Arc::ptr_eq(&self.cards, &other.cards)
    }
}

impl Eq for GameState {}

impl GameState {
    /// Write a terminal broken status, carrying `breakage`'s rule, entity,
    /// and expected component. Nothing else about `self` changes and
    /// nothing rolls back — any work already computed and queued when the
    /// failing read happened stays exactly where it was, still readable in
    /// the returned state.
    pub(crate) fn break_game(&self, breakage: Breakage) -> GameState {
        let mut next = self.clone();
        next.status = GameStatus::Broken(breakage);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cards::{ComponentKind, fixtures};

    fn card_ref(id: u32) -> CardRef {
        CardRef {
            instance: CardInstanceId(id),
            def: fixtures::id("quarry-whelp"),
        }
    }

    fn minimal_player_state() -> PlayerState {
        PlayerState {
            main: None,
            bench: [None, None, None],
            deck: vec![],
            hand: vec![],
            prizes: vec![],
            discard: vec![],
            mana: ManaBank::default(),
            main_losses: 0,
            enchantments: vec![],
        }
    }

    fn minimal_state() -> GameState {
        GameState {
            players: PerPlayer::new(minimal_player_state(), minimal_player_state()),
            coin: None,
            turn: TurnState {
                active_player: PlayerId::One,
                phase: Phase::Main,
                window: None,
                normal_attack_used: false,
                normal_retreat_used: false,
                spell_played_this_turn: false,
            },
            stack: vec![],
            stack_segment_bases: vec![],
            work: VecDeque::from(vec![WorkItem::LossCheck(PlayerId::One)]),
            pending: None,
            status: GameStatus::Playing,
            cards: fixtures::card_set(),
        }
    }

    #[test]
    fn break_game_sets_broken_status_and_leaves_the_rest_untouched() {
        let state = minimal_state();

        let broken = state.break_game(breakage());

        assert_eq!(broken.status, GameStatus::Broken(breakage()));
        assert_eq!(broken.work, state.work);
        assert_eq!(broken.players, state.players);
        assert_eq!(broken.pending, state.pending);
    }

    #[test]
    fn upgrade_chain_top_falls_back_to_base_when_no_upgrades_exist() {
        let chain = UpgradeChain::new(card_ref(1), vec![]);
        assert_eq!(chain.top(), card_ref(1));
        assert_eq!(chain.base(), card_ref(1));
        assert_eq!(chain.layers().count(), 1);
    }

    #[test]
    fn upgrade_chain_top_is_the_last_layer_stacked_on() {
        let chain = UpgradeChain::new(card_ref(1), vec![card_ref(2), card_ref(3)]);
        assert_eq!(chain.top(), card_ref(3));
        assert_eq!(chain.base(), card_ref(1));
        assert_eq!(chain.layers().count(), 3);
    }

    #[test]
    fn duration_marker_variant_constructs() {
        let markers = [DurationMarker::CannotBeMovedByOpponent];
        assert_eq!(markers.len(), 1);
    }

    #[test]
    fn mana_bank_defaults_to_empty() {
        assert_eq!(
            ManaBank::default(),
            ManaBank {
                matter: 0,
                mind: 0,
                spirit: 0,
            }
        );
    }

    #[test]
    fn per_player_reads_the_matching_seat() {
        let pair = PerPlayer::new(1, 2);
        assert_eq!(*pair.get(PlayerId::One), 1);
        assert_eq!(*pair.get(PlayerId::Two), 2);
    }

    #[test]
    fn per_player_get_mut_writes_the_matching_seat() {
        let mut pair = PerPlayer::new(1, 2);
        *pair.get_mut(PlayerId::Two) = 9;
        assert_eq!(pair, PerPlayer::new(1, 9));
    }

    #[test]
    fn summon_turn_record_fresh_is_available_with_no_main_entry() {
        assert_eq!(
            SummonTurnRecord::fresh(),
            SummonTurnRecord {
                upgrade: UpgradeActivity::Available,
                main_entry: None,
            }
        );
    }

    #[test]
    fn every_upgrade_activity_variant_constructs() {
        let activities = [
            UpgradeActivity::Available,
            UpgradeActivity::PlayedThisTurn,
            UpgradeActivity::UpgradedThisTurn,
        ];

        assert_eq!(activities.len(), 3);
    }

    #[test]
    fn entered_main_is_independent_from_upgrade_activity() {
        let record = SummonTurnRecord {
            upgrade: UpgradeActivity::UpgradedThisTurn,
            main_entry: Some(EnteredMain),
        };

        assert_eq!(record.upgrade, UpgradeActivity::UpgradedThisTurn);
        assert_eq!(record.main_entry, Some(EnteredMain));
    }

    #[test]
    fn stack_window_and_phase_variants_construct() {
        let window = StackWindow {
            holder: PlayerId::One,
            prior_pass: true,
        };
        let phases = [Phase::Upkeep, Phase::Main, Phase::Combat];
        assert_eq!(phases.len(), 3);
        assert_eq!(window.holder, PlayerId::One);
    }

    #[test]
    fn turn_state_window_is_a_peer_of_phase_not_nested_in_it() {
        // A window can be open during any phase (a proactive Spell cast in
        // Main, a respondable trigger during Upkeep resolution, a declared
        // attack in Combat); it must be representable independent of which
        // phase is resting.
        let window = StackWindow {
            holder: PlayerId::Two,
            prior_pass: false,
        };
        let turn = TurnState {
            active_player: PlayerId::One,
            phase: Phase::Upkeep,
            window: Some(window),
            normal_attack_used: false,
            normal_retreat_used: false,
            spell_played_this_turn: false,
        };
        assert_eq!(turn.phase, Phase::Upkeep);
        assert_eq!(turn.window, Some(window));
    }

    #[test]
    fn mana_source_variants_construct() {
        let sources = [ManaSource::Player, ManaSource::Summon(Position::Main)];
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn pending_input_variants_construct() {
        let pendings = [
            PendingInput::ManaProduction {
                player: PlayerId::One,
                source: ManaSource::Player,
            },
            PendingInput::Promotion {
                player: PlayerId::One,
            },
            PendingInput::PrizePick {
                chooser: PlayerId::One,
            },
        ];
        assert_eq!(pendings.len(), 3);
    }

    #[test]
    fn loss_reason_variants_construct() {
        let reasons = [
            LossReason::ThirdMainLoss,
            LossReason::NoPromotionAvailable,
            LossReason::EmptyDeckDraw,
        ];
        assert_eq!(reasons.len(), 3);
    }

    #[test]
    fn game_outcome_constructs() {
        let outcome = GameOutcome {
            winner: PlayerId::One,
            reason: LossReason::ThirdMainLoss,
        };
        assert_eq!(outcome.winner, PlayerId::One);
    }

    fn breakage() -> Breakage {
        Breakage {
            rule: "destruction",
            entity: fixtures::id("quarry-whelp"),
            expected: ComponentKind::Life,
        }
    }

    #[test]
    fn every_game_status_variant_constructs() {
        let statuses = [
            GameStatus::Playing,
            GameStatus::Ended(GameOutcome {
                winner: PlayerId::One,
                reason: LossReason::ThirdMainLoss,
            }),
            GameStatus::Broken(breakage()),
        ];
        assert_eq!(statuses.len(), 3);
    }

    #[test]
    fn is_playing_is_true_only_for_playing() {
        assert!(GameStatus::Playing.is_playing());
        assert!(
            !GameStatus::Ended(GameOutcome {
                winner: PlayerId::One,
                reason: LossReason::ThirdMainLoss,
            })
            .is_playing()
        );
        assert!(!GameStatus::Broken(breakage()).is_playing());
    }

    #[test]
    fn stack_item_variants_construct() {
        let items = [
            StackItem::Attack {
                attacker: PlayerId::One,
                target: Position::Main,
            },
            StackItem::Spell {
                caster: PlayerId::One,
                card: CardRef {
                    instance: CardInstanceId(1),
                    def: fixtures::id("ember-lance"),
                },
                targets: vec![Position::Main],
            },
            StackItem::Trigger {
                controller: PlayerId::One,
                source: Position::Main,
                event: TriggerEvent::AnySummonDestroyed,
                targets: vec![Position::Main],
                effects: vec![],
            },
        ];
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn movement_step_variants_construct_in_the_fixed_order() {
        let steps = [
            MovementStep::LeavingMain,
            MovementStep::EnteringBench,
            MovementStep::LeavingBench,
            MovementStep::EnteringMain,
        ];
        assert_eq!(steps.len(), 4);
    }

    #[test]
    fn every_work_item_variant_constructs() {
        let items = vec![
            WorkItem::DestructionCheck(Position::Main),
            WorkItem::DiscardDestroyedChain(Position::Main),
            WorkItem::RecordMainLoss(PlayerId::One),
            WorkItem::RecoverPrize(PlayerId::One),
            WorkItem::PromoteBenchSummon(PlayerId::One),
            WorkItem::ResolveMovementConsequences(PlayerId::One),
            WorkItem::MovementTrigger(MovementStep::LeavingMain, PlayerId::One, Position::Main),
            WorkItem::FireTrigger(
                PlayerId::One,
                Position::Main,
                TriggerEvent::YourUpkeep,
                fixtures::trigger_id("spite-thorn"),
            ),
            WorkItem::LossCheck(PlayerId::One),
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ];
        assert_eq!(items.len(), 12);
    }
}
