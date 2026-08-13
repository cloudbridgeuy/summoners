//! End-to-end proof that a card missing a fact a rule demands breaks the
//! game cleanly, through `apply`, rather than panicking or silently
//! treating the card as indestructible: `state.status` becomes `Broken`,
//! naming the rule, the entity, and the missing component; nothing already
//! queued is lost; and the next action attempted is rejected. Shares
//! `tests.rs`'s fixtures through `super::*`, like `demo.rs` and its
//! siblings.

use super::*;
use crate::domain::cards::{Breakage, CardSet, ComponentKind, Entity, EntityId};
use crate::domain::state::WorkItem;
use std::sync::Arc;

/// An id no fixture card in `fixtures::entities()` uses.
fn life_less_entity_id() -> EntityId {
    EntityId::parse(&"aa".repeat(16)).expect("valid fixture id")
}

/// A top-level entity that prints nothing at all — in particular, no
/// `Life` — the shape no card in this crate's own fixture set is ever
/// authored with on purpose.
fn life_less_entity() -> Entity {
    Entity {
        id: life_less_entity_id(),
        components: vec![],
    }
}

/// Every fixture entity `apply`'s handlers already know how to read, plus
/// one Life-less card, wrapped in a fresh `CardSet`/`Arc` of its own — the
/// shared fixture `Arc` (`fixtures::card_set()`) is never touched.
fn cards_with_a_life_less_entity() -> Arc<CardSet> {
    let mut entities = fixtures::entities();
    entities.push(life_less_entity());
    Arc::new(CardSet::new(entities))
}

#[test]
fn attacking_a_battlefield_card_with_no_life_breaks_the_game_through_apply() {
    // Rules §23: a battlefield card is expected to print its own Life. One
    // attacks with the fixture Quarry Whelp already sitting on their own
    // Main (`base_state`); Two's Main instead holds a card with no
    // components at all, so nothing can answer for its Life once the
    // attack lands.
    let mut state = base_state();
    state.cards = cards_with_a_life_less_entity();
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: life_less_entity_id(),
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    });

    let declared = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("Quarry Whelp's Attack is free and Main is a legal target");
    let defender_passed = apply(
        &declared.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the defender holds Priority first");
    let resolved = apply(
        &defender_passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the attacker passes second, closing the window and draining the Attack");

    assert_eq!(
        resolved.state.status,
        GameStatus::Broken(Breakage {
            rule: "destruction",
            entity: life_less_entity_id(),
            expected: ComponentKind::Life,
        }),
        "the destruction check that ran once the Attack's damage landed \
         could not read Two's Main card's Life, so the match breaks \
         instead of either panicking or treating the card as indestructible"
    );

    // The next action attempted, by either player, is rejected — the same
    // way a finished game is, just for a different reason.
    assert_eq!(
        apply(&resolved.state, &end_turn(PlayerId::Two)),
        Err(ActionError::GameBroken)
    );
}

#[test]
fn a_broken_destruction_check_leaves_earlier_queued_work_untouched() {
    // Rules §23-24, §41: Two's Bench holds an already-over-damaged, fully
    // legitimate Quarry Whelp, and Two's Main holds the Life-less card.
    // Both `DestructionCheck`s are already queued, Bench ahead of Main.
    // The Bench check runs first and queues its own two-step chain
    // (discard, then a loss check); the Main check runs second and breaks
    // the game before it can queue anything of its own — the Bench chain
    // it left behind must still be sitting in `work`, unread, once the
    // loop stops (rules: "whatever is still queued in `work` at that
    // point is left exactly where it is").
    let mut state = base_state();
    state.cards = cards_with_a_life_less_entity();
    state.players.get_mut(PlayerId::Two).bench[0] = Some(SummonInstance {
        damage: 40, // Quarry Whelp's printed Life.
        ..summon(PlayerId::Two)
    });
    state.players.get_mut(PlayerId::Two).main = Some(SummonInstance {
        chain: UpgradeChain::new(
            CardRef {
                instance: CardInstanceId(2),
                def: life_less_entity_id(),
            },
            vec![],
        ),
        ..summon(PlayerId::Two)
    });
    state.work = VecDeque::from(vec![
        WorkItem::DestructionCheck(Position::Bench(BenchSlot::First)),
        WorkItem::DestructionCheck(Position::Main),
    ]);

    let (drained, _events) = crate::engine::resolution::drain(&state);

    assert_eq!(
        drained.status,
        GameStatus::Broken(Breakage {
            rule: "destruction",
            entity: life_less_entity_id(),
            expected: ComponentKind::Life,
        })
    );
    assert_eq!(
        drained.work,
        VecDeque::from(vec![
            WorkItem::DiscardDestroyedChain(Position::Bench(BenchSlot::First)),
            WorkItem::LossCheck(PlayerId::Two),
        ]),
        "the bench chain the first, successful check already computed is \
         still sitting in the queue, untouched, once the second check \
         breaks the game"
    );
}
