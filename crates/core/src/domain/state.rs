//! `GameState`: one plain, cloneable value that is authoritative for the
//! whole match. Everything the engine does is a pure function from one
//! `GameState` plus one `GameAction` to the next `GameState` plus a batch of
//! `GameEvent`s (decision 3).

use std::collections::VecDeque;

use crate::domain::cards::{CardDefId, TriggerEvent};
use crate::domain::ids::{CardInstanceId, PlayerId, Position};

/// A card reference in a non-battlefield zone: the specific instance and the
/// card it prints. Deck, hand, prizes, and discard all hold these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardRef {
    pub instance: CardInstanceId,
    pub def: CardDefId,
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

/// One Summon in play: its upgrade chain, accumulated Damage, Ready state,
/// who owns and who controls it, any duration markers, and the per-turn
/// flags that gate upgrading, attacking, and Retreating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonInstance {
    pub chain: UpgradeChain,
    pub damage: u32,
    pub ready: bool,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub duration_markers: Vec<DurationMarker>,
    /// Rules §17: a Base Summon cannot be upgraded the turn it was played.
    pub played_this_turn: bool,
    /// Rules §18: a Summon may be upgraded only once per turn.
    pub upgraded_this_turn: bool,
    /// Rules §30: whether this Summon entered Main this turn (feeds
    /// `ConditionalBonus { condition: DefenderEnteredMainThisTurn, .. }`).
    pub entered_main_this_turn: bool,
}

/// The three typed Mana pools a player has banked (rules §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ManaBank {
    pub matter: u32,
    pub mind: u32,
    pub spirit: u32,
}

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
    /// Rules §7: only the second player starts with the Coin.
    pub has_coin: bool,
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
    /// Rules §28: one step of the fixed movement-trigger order.
    MovementTrigger(MovementStep, Position),
    /// Rules §36–38: a Triggered Ability fires for the Summon here.
    FireTrigger(Position, TriggerEvent),
    /// Rules §24 step 6, §2: check whether this player has now lost.
    LossCheck(PlayerId),
    /// Rules §10: ready every Summon the new active player controls.
    ReadyAll,
    /// Rules §10: draw one card; an empty deck is checked via `LossCheck`.
    DrawCard,
    /// Rules §10–12: generate Mana from this source; may set `pending`.
    ProduceMana(ManaSource),
}

/// One plain, cloneable value: the whole match. The same state and the same
/// action always produce the same outcome (the design's transition
/// contract).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameState {
    pub players: PerPlayer<PlayerState>,
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
    pub outcome: Option<GameOutcome>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::cards::CardDefId;

    fn card_ref(id: u32) -> CardRef {
        CardRef {
            instance: CardInstanceId(id),
            def: CardDefId("quarry-whelp"),
        }
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
                    def: CardDefId("ember-lance"),
                },
                targets: vec![Position::Main],
            },
        ];
        assert_eq!(items.len(), 2);
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
            WorkItem::MovementTrigger(MovementStep::LeavingMain, Position::Main),
            WorkItem::FireTrigger(Position::Main, TriggerEvent::YourUpkeep),
            WorkItem::LossCheck(PlayerId::One),
            WorkItem::ReadyAll,
            WorkItem::DrawCard,
            WorkItem::ProduceMana(ManaSource::Player),
        ];
        assert_eq!(items.len(), 12);
    }
}
