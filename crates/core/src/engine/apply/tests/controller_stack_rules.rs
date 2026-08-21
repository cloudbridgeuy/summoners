//! Public-action checks for controller-relative Spell facts and the protected
//! Attack inside a live Stack segment.

use super::*;

fn griefsinger_state() -> GameState {
    let mut state = base_state();
    state.players.get_mut(PlayerId::One).main = Some(SummonInstance {
        chain: UpgradeChain::new(card_ref(20, "griefsinger"), vec![]),
        ..summon(PlayerId::One)
    });
    state.players.get_mut(PlayerId::One).mana = ManaBank {
        matter: 10,
        mind: 10,
        spirit: 10,
    };
    state.players.get_mut(PlayerId::Two).mana.matter = 10;
    state
}

fn cast_and_resolve_support(mut state: GameState) -> GameState {
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(30, "renewing-balm")];
    let cast = apply(
        &state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(30),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("the active player can cast the Support Spell");
    let passed = apply(
        &cast.state,
        &GameAction::PassPriority {
            player: PlayerId::Two,
        },
    )
    .expect("the opponent passes");
    apply(
        &passed.state,
        &GameAction::PassPriority {
            player: PlayerId::One,
        },
    )
    .expect("the caster passes and resolves the Spell")
    .state
}

#[test]
fn casting_a_spell_records_history_only_for_its_caster() {
    let state = cast_and_resolve_support(griefsinger_state());

    assert!(*state.turn.spell_played_this_turn.get(PlayerId::One));
    assert!(!*state.turn.spell_played_this_turn.get(PlayerId::Two));
}

#[test]
fn support_spell_above_a_protected_attack_does_not_remove_its_block() {
    let mut state = cast_and_resolve_support(griefsinger_state());
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(31, "ember-lance")];
    state.players.get_mut(PlayerId::Two).hand = vec![card_ref(32, "renewing-balm")];
    let attack = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("the active player declares the protected Attack");
    let support = apply(
        &attack.state,
        &GameAction::CastSpell {
            player: PlayerId::Two,
            card: CardInstanceId(32),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("the defender casts a Support Spell above the Attack");

    assert_eq!(
        apply(
            &support.state,
            &GameAction::CastSpell {
                player: PlayerId::One,
                card: CardInstanceId(31),
                targets: vec![Position::Main],
                mana_hint: None,
            },
        ),
        Err(ActionError::WrongPhase)
    );
}

#[test]
fn opponents_spell_history_does_not_activate_the_attackers_block() {
    let mut state = griefsinger_state();
    state.players.get_mut(PlayerId::One).hand = vec![card_ref(41, "ember-lance")];
    state.players.get_mut(PlayerId::Two).hand = vec![card_ref(42, "renewing-balm")];
    let attack = apply(
        &state,
        &GameAction::DeclareAttack {
            player: PlayerId::One,
            target: Position::Main,
            mana_hint: None,
        },
    )
    .expect("the active player declares an Attack before casting a Spell");
    let support = apply(
        &attack.state,
        &GameAction::CastSpell {
            player: PlayerId::Two,
            card: CardInstanceId(42),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("the defender casts a Support Spell");

    apply(
        &support.state,
        &GameAction::CastSpell {
            player: PlayerId::One,
            card: CardInstanceId(41),
            targets: vec![Position::Main],
            mana_hint: None,
        },
    )
    .expect("only the defender has Spell history, so the attacker's block is false");
}
