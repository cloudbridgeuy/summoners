//! Identity and position vocabulary shared across the engine.
//!
//! These types name players, card instances, and battlefield positions.
//! They carry no game behavior of their own; they exist so every other
//! module can speak about "who" and "where" without smuggling raw indices
//! through the code.

/// One of the two competing players (rules §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerId {
    One,
    Two,
}

impl PlayerId {
    /// The other seat at the table.
    pub fn opponent(self) -> PlayerId {
        match self {
            PlayerId::One => PlayerId::Two,
            PlayerId::Two => PlayerId::One,
        }
    }
}

/// A single physical card as it exists inside one match: one deck slot, one
/// hand card, or one layer of an upgrade chain. Scenarios assign these ids
/// up front; the engine never invents a new one for a card that already
/// exists (rules §20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CardInstanceId(pub u32);

/// One of the three Bench positions (rules §8: up to three Benched Summons).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BenchSlot {
    First,
    Second,
    Third,
}

impl BenchSlot {
    /// Every Bench slot, in a fixed order.
    pub const ALL: [BenchSlot; 3] = [BenchSlot::First, BenchSlot::Second, BenchSlot::Third];

    /// The slot's place in a fixed-size array of three.
    pub fn index(self) -> usize {
        match self {
            BenchSlot::First => 0,
            BenchSlot::Second => 1,
            BenchSlot::Third => 2,
        }
    }
}

/// A battlefield position: the single Main slot or one of three Bench
/// slots. Attacks and most actions target a `Position`, never a Summon
/// identity that could move away before the action resolves (rules §30).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Position {
    Main,
    Bench(BenchSlot),
}

/// The three typed Mana pools (rules §11–12). There is no Generic pool;
/// Generic cost components accept Mana of any of these types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManaType {
    Matter,
    Mind,
    Spirit,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_id_opponent_is_the_other_seat() {
        assert_eq!(PlayerId::One.opponent(), PlayerId::Two);
        assert_eq!(PlayerId::Two.opponent(), PlayerId::One);
    }

    #[test]
    fn card_instance_id_constructs_and_compares() {
        assert_eq!(CardInstanceId(1), CardInstanceId(1));
        assert_ne!(CardInstanceId(1), CardInstanceId(2));
    }

    #[test]
    fn bench_slot_variants_construct_and_index() {
        assert_eq!(BenchSlot::First.index(), 0);
        assert_eq!(BenchSlot::Second.index(), 1);
        assert_eq!(BenchSlot::Third.index(), 2);
        assert_eq!(BenchSlot::ALL.len(), 3);
    }

    #[test]
    fn position_variants_construct() {
        let main = Position::Main;
        let bench = Position::Bench(BenchSlot::Second);
        assert_ne!(main, bench);
    }

    #[test]
    fn mana_type_variants_construct() {
        let types = [ManaType::Matter, ManaType::Mind, ManaType::Spirit];
        assert_eq!(types.len(), 3);
    }
}
