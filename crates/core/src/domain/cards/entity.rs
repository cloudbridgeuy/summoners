//! The card container: `Entity`, `Component`, and the typed reads over them.
//!
//! `Entity` is the one structural shape a card and an ability share: an
//! identity and an ordered sequence of components. A top-level entity is a
//! printed card; a `Component::Skill`, `Component::Attack`, or
//! `Component::Trigger` holds a nested entity for one of a card's abilities.
//! The recursion runs through `Vec<Component>`, so the type stays finite
//! without boxing, and it stops at `Effect`: nothing ever names an effect, so
//! it stays a leaf value.
//!
//! Multiplicity belongs to the read, not to the data — no arity is declared
//! anywhere. `get` answers with the first match, `all` answers with every
//! match in authored order, and `demand` answers with the first match or a
//! named `Breakage`. Each read is typed by a marker type through
//! `ComponentField`, implemented once per component so the dispatch stays
//! total and panic-free.

use super::{Cost, EffectLeaf, Form, Modifier, SpellTiming, TriggerEvent};
use crate::domain::ids::ManaType;
use std::fmt;

/// A core-defined, opaque card or ability identity. The core holds,
/// compares, and parses an `EntityId`; it never generates one. Minting an id
/// is a shell concern, outside this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId([u8; 16]);

/// Why `EntityId::parse` rejected its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityIdParseError {
    pub input: String,
}

/// Where the canonical UUID shape (`8-4-4-4-12`) places its four hyphens.
const CANONICAL_HYPHEN_POSITIONS: [usize; 4] = [8, 13, 18, 23];

impl EntityId {
    /// Parse sixteen bytes from a hex string. Exactly two shapes are
    /// accepted: a bare 32-digit hex string, and the canonical UUID shape
    /// (`8-4-4-4-12`, e.g. `01234567-89ab-cdef-0123-456789abcdef`). Both
    /// parse to the same id when they carry the same digits, so an id has
    /// at most two spellings, never an open-ended family of them. Every
    /// other input is rejected — the wrong length, a hyphen anywhere but
    /// the canonical four positions, or any character that is not an ASCII
    /// hex digit (this also rejects a sign character such as `+` or `-`
    /// inside a byte pair, which `u8::from_str_radix` alone would accept).
    /// This never mints an id: the same input always parses to the same
    /// bytes.
    pub fn parse(input: &str) -> Result<EntityId, EntityIdParseError> {
        let malformed = || EntityIdParseError {
            input: input.to_string(),
        };

        let hex: String = match input.len() {
            32 => input.to_string(),
            36 => {
                let bytes = input.as_bytes();
                if CANONICAL_HYPHEN_POSITIONS
                    .iter()
                    .any(|&position| bytes[position] != b'-')
                {
                    return Err(malformed());
                }
                input
                    .chars()
                    .filter(|character| *character != '-')
                    .collect()
            }
            _ => return Err(malformed()),
        };

        if hex.len() != 32 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err(malformed());
        }

        let mut bytes = [0u8; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let start = index * 2;
            // `hex` is now known to be exactly 32 ASCII hex digits, so this
            // slice and this parse cannot fail.
            let pair = hex.get(start..start + 2).ok_or_else(malformed)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| malformed())?;
        }
        Ok(EntityId(bytes))
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0;
        write!(
            formatter,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15],
        )
    }
}

/// A card or ability's display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name(pub String);

/// The human-readable handle a test or a player uses instead of an
/// `EntityId`: a collection prefix and a number, e.g. `QRY-014`. It is data,
/// so a card may carry one, several, or none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountingId {
    pub prefix: String,
    pub number: u32,
}

impl AccountingId {
    /// The canonical printed code, e.g. `QRY-014`. `CardSet` indexes on this
    /// string.
    pub fn code(&self) -> String {
        format!("{}-{:03}", self.prefix, self.number)
    }
}

/// How much damage an entity can take before it is destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Life(pub u32);

/// The printed cost to retreat this entity from Main to the Bench.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetreatCost(pub u32);

/// The Mana types this entity produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManaTypes(pub Vec<ManaType>);

/// Open category labels a rule can key on instead of a class field, e.g.
/// `"mount"` or `"artifact"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tags(pub Vec<String>);

/// A zero-sized marker for `Component::Skill`'s nested entity. `Skill`,
/// `Attack`, and `Trigger` all wrap an `Entity`, so each needs its own marker
/// type to keep `entity.all::<Skill>()` distinct from `entity.all::<Attack>()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skill;

/// A zero-sized marker for `Component::Attack`'s nested entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attack;

/// A zero-sized marker for `Component::Trigger`'s nested entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigger;

/// A zero-sized marker for `Component::Respondable`: this ability may be
/// responded to while it waits on the Stack (rules §37–38). Presence, not a
/// boolean payload, since absence already answers "not respondable" on its
/// own — a card that never prints this component needs no separate `false`
/// to write down (make impossible states impossible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Respondable;

/// A zero-sized marker for `Component::Persistent`: a resolved Spell or
/// Enchantment carrying this component stays in play instead of moving to
/// its caster's discard pile (rules §44). Presence, not a boolean payload —
/// and not a card family — for the same reason `Respondable` is one: a card
/// that never prints this component already answers "does not persist" on
/// its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Persistent;

/// One typed fact or ability an `Entity` carries. The enum is closed and
/// additive: a new variant never invalidates a card or a read already
/// written against an earlier one. A card declares facts here; it declares
/// no rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Component {
    Name(Name),
    AccountingId(AccountingId),
    Life(Life),
    RetreatCost(RetreatCost),
    Form(Form),
    Produces(ManaTypes),
    Tags(Tags),
    Cost(Cost),
    /// May appear many times; read with `entity.all::<Skill>()`.
    Skill(Entity),
    /// May appear many times; read with `entity.all::<Attack>()`.
    Attack(Entity),
    /// May appear many times; read with `entity.all::<Trigger>()`.
    Trigger(Entity),
    /// May appear many times; order carries meaning. Recursion stops here —
    /// nothing ever references an effect, so it stays a leaf value.
    Effect(EffectLeaf),
    Passive(Modifier),
    /// A Spell's timing family (rules §34): whether it may be cast
    /// proactively during its controller's own resting Main Phase, or only
    /// as a response while its caster holds Priority.
    Timing(SpellTiming),
    /// The event a Trigger fires on (rules §36–41). Read off the same
    /// nested entity a `Component::Trigger` wraps, alongside that entity's
    /// own `Component::Effect`s and, if it carries one, its
    /// `Component::Respondable` marker.
    Event(TriggerEvent),
    /// Marks the entity carrying it as respondable while it waits on the
    /// Stack (rules §37–38). See `Respondable`'s own doc comment for why
    /// this is a presence marker rather than a boolean payload.
    Respondable,
    /// Marks a Spell or Enchantment as staying in play after it resolves,
    /// instead of moving to its caster's discard pile (rules §44). See
    /// `Persistent`'s own doc comment for why this is a presence marker
    /// rather than a boolean payload, and why it replaces a card-family
    /// check.
    Persistent,
}

/// One printed card, or one ability nested inside a card, sharing the same
/// structural shape: an identity and an ordered sequence of components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub components: Vec<Component>,
}

/// Names one `Component` variant with no payload, for `Breakage::expected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    Name,
    AccountingId,
    Life,
    RetreatCost,
    Form,
    Produces,
    Tags,
    Cost,
    Skill,
    Attack,
    Trigger,
    Effect,
    Passive,
    Timing,
    Event,
    Respondable,
    Persistent,
}

/// What a demanded component's absence means: this game (or, inside
/// `scenario::parse`, this prospective game) can no longer be computed.
/// Names the rule that demanded, the entity it demanded from, and the
/// component it expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breakage {
    pub rule: &'static str,
    pub entity: EntityId,
    pub expected: ComponentKind,
}

/// Maps a marker type to the `Component` variant it reads, so
/// `Entity::get`, `Entity::all`, and `Entity::demand` stay typed. Implemented
/// once per component; every implementation is total and panic-free.
pub trait ComponentField {
    /// The type a successful read produces. Equal to `Self` for every
    /// component whose payload the marker names directly; `Entity` for
    /// `Skill`, `Attack`, and `Trigger`, whose marker is not their payload.
    type Output;

    fn component_kind() -> ComponentKind;

    fn extract(component: &Component) -> Option<&Self::Output>;
}

macro_rules! component_field {
    ($marker:ty, $output:ty, $kind:ident, $variant:pat => $binding:ident) => {
        impl ComponentField for $marker {
            type Output = $output;

            fn component_kind() -> ComponentKind {
                ComponentKind::$kind
            }

            fn extract(component: &Component) -> Option<&Self::Output> {
                match component {
                    $variant => Some($binding),
                    _ => None,
                }
            }
        }
    };
}

component_field!(Name, Name, Name, Component::Name(name) => name);
component_field!(AccountingId, AccountingId, AccountingId, Component::AccountingId(id) => id);
component_field!(Life, Life, Life, Component::Life(life) => life);
component_field!(RetreatCost, RetreatCost, RetreatCost, Component::RetreatCost(cost) => cost);
component_field!(Form, Form, Form, Component::Form(form) => form);
component_field!(ManaTypes, ManaTypes, Produces, Component::Produces(types) => types);
component_field!(Tags, Tags, Tags, Component::Tags(tags) => tags);
component_field!(Cost, Cost, Cost, Component::Cost(cost) => cost);
component_field!(Skill, Entity, Skill, Component::Skill(entity) => entity);
component_field!(Attack, Entity, Attack, Component::Attack(entity) => entity);
component_field!(Trigger, Entity, Trigger, Component::Trigger(entity) => entity);
component_field!(EffectLeaf, EffectLeaf, Effect, Component::Effect(effect) => effect);
component_field!(Modifier, Modifier, Passive, Component::Passive(modifier) => modifier);
component_field!(SpellTiming, SpellTiming, Timing, Component::Timing(timing) => timing);
component_field!(TriggerEvent, TriggerEvent, Event, Component::Event(event) => event);

impl ComponentField for Respondable {
    type Output = Respondable;

    fn component_kind() -> ComponentKind {
        ComponentKind::Respondable
    }

    fn extract(component: &Component) -> Option<&Self::Output> {
        const MARKER: Respondable = Respondable;
        match component {
            Component::Respondable => Some(&MARKER),
            _ => None,
        }
    }
}

impl ComponentField for Persistent {
    type Output = Persistent;

    fn component_kind() -> ComponentKind {
        ComponentKind::Persistent
    }

    fn extract(component: &Component) -> Option<&Self::Output> {
        const MARKER: Persistent = Persistent;
        match component {
            Component::Persistent => Some(&MARKER),
            _ => None,
        }
    }
}

impl Entity {
    /// The first matching component, or `None`. Absence is a legitimate
    /// answer for an optional read.
    pub fn get<C: ComponentField>(&self) -> Option<&C::Output> {
        self.components.iter().find_map(C::extract)
    }

    /// Every matching component, in authored order.
    pub fn all<C: ComponentField>(&self) -> Vec<&C::Output> {
        self.components.iter().filter_map(C::extract).collect()
    }

    /// The first matching component, or a `Breakage` naming `rule`, this
    /// entity, and the component kind that was expected. Absence here means
    /// the game this entity belongs to can no longer be computed.
    pub fn demand<C: ComponentField>(&self, rule: &'static str) -> Result<&C::Output, Breakage> {
        self.get::<C>().ok_or(Breakage {
            rule,
            entity: self.id,
            expected: C::component_kind(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use crate::domain::cards::EffectTarget;

    fn id(byte: u8) -> EntityId {
        EntityId([byte; 16])
    }

    #[test]
    fn entity_id_parses_a_bare_hex_string() {
        let parsed = EntityId::parse("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(
            parsed,
            EntityId([
                0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
                0xcd, 0xef,
            ])
        );
    }

    #[test]
    fn entity_id_parses_a_uuid_shaped_string_identically() {
        let bare = EntityId::parse("0123456789abcdef0123456789abcdef").unwrap();
        let hyphenated = EntityId::parse("01234567-89ab-cdef-0123-456789abcdef").unwrap();
        assert_eq!(bare, hyphenated);
    }

    #[test]
    fn entity_id_display_normalizes_bare_input_to_canonical_uuid_text() {
        let parsed = EntityId::parse("0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(parsed.to_string(), "01234567-89ab-cdef-0123-456789abcdef");
    }

    #[test]
    fn entity_id_display_normalizes_uppercase_input_to_lowercase_uuid_text() {
        let parsed = EntityId::parse("01234567-89AB-CDEF-0123-456789ABCDEF").unwrap();
        assert_eq!(parsed.to_string(), "01234567-89ab-cdef-0123-456789abcdef");
    }

    #[test]
    fn entity_id_rejects_the_wrong_length() {
        assert!(EntityId::parse("abc").is_err());
        assert!(EntityId::parse("").is_err());
    }

    #[test]
    fn entity_id_rejects_non_hex_characters() {
        assert!(EntityId::parse("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    /// `u8::from_str_radix` alone accepts a leading `+` (`from_str_radix("+f",
    /// 16) == Ok(15)`), which would let `"+f".repeat(16)` parse to the same
    /// bytes as `"0f".repeat(16)` — one id with two spellings. `parse` must
    /// reject the whole string before it ever reaches `from_str_radix`.
    #[test]
    fn entity_id_rejects_a_leading_plus_sign_in_a_byte_pair() {
        assert!(EntityId::parse(&"+f".repeat(16)).is_err());
    }

    /// A `-` embedded where a hex digit belongs must not be filtered away:
    /// this string is 32 characters long, so it never enters the
    /// canonical-hyphen-position branch, and the stray `-` must fail the
    /// hex-digit check instead of shifting the remaining digits.
    #[test]
    fn entity_id_rejects_a_sign_embedded_mid_string() {
        let base = "0123456789abcdef0123456789abcdef";
        let with_embedded_dash = format!("{}-{}", &base[..5], &base[6..]);
        assert_eq!(with_embedded_dash.len(), 32);
        assert!(EntityId::parse(&with_embedded_dash).is_err());
    }

    #[test]
    fn entity_id_rejects_a_hyphen_outside_the_canonical_positions() {
        // Same 32 hex digits and hyphen count as a valid UUID shape, but the
        // hyphens sit one position to the left of where they must be.
        assert!(EntityId::parse("0123456-789abcde-f0123-4567-89abcdef").is_err());
    }

    #[test]
    fn entity_id_holds_and_compares_the_same_input_identically() {
        let too_long = "1".repeat(34);
        assert!(EntityId::parse(&too_long).is_err());

        let same = EntityId::parse(&"1".repeat(32)).unwrap();
        let again = EntityId::parse(&"1".repeat(32)).unwrap();
        assert_eq!(same, again);
    }

    #[test]
    fn accounting_id_formats_the_printed_code() {
        let accounting = AccountingId {
            prefix: "QRY".to_string(),
            number: 14,
        };
        assert_eq!(accounting.code(), "QRY-014");
    }

    #[test]
    fn get_returns_the_first_of_several_life_components() {
        let entity = Entity {
            id: id(1),
            components: vec![Component::Life(Life(40)), Component::Life(Life(999))],
        };
        assert_eq!(entity.get::<Life>(), Some(&Life(40)));
    }

    #[test]
    fn get_returns_none_when_absent() {
        let entity = Entity {
            id: id(2),
            components: vec![Component::Life(Life(40))],
        };
        assert_eq!(entity.get::<RetreatCost>(), None);
    }

    #[test]
    fn all_returns_every_skill_in_authored_order() {
        let first = Entity {
            id: id(10),
            components: vec![Component::Name(Name("First".to_string()))],
        };
        let second = Entity {
            id: id(11),
            components: vec![Component::Name(Name("Second".to_string()))],
        };
        let third = Entity {
            id: id(12),
            components: vec![Component::Name(Name("Third".to_string()))],
        };
        let card = Entity {
            id: id(1),
            components: vec![
                Component::Skill(first.clone()),
                Component::Skill(second.clone()),
                Component::Skill(third.clone()),
            ],
        };
        assert_eq!(card.all::<Skill>(), vec![&first, &second, &third]);
    }

    /// One entity inside a component inside an entity, read at both levels
    /// (design proof 3). The card's `Skill` component holds a nested entity
    /// that itself carries a `Cost` and two `Effect` components; the test
    /// reads the nested entity out of the card, and then reads its own
    /// components out of that nested entity, proving the recursion is real
    /// rather than a value the reader can only take whole.
    #[test]
    fn a_component_nested_inside_a_skill_entity_reads_through_both_levels() {
        let ability = Entity {
            id: id(30),
            components: vec![
                Component::Cost(Cost {
                    generic: 1,
                    ..Cost::default()
                }),
                Component::Effect(EffectLeaf::Heal {
                    amount: 5,
                    target: EffectTarget::Selected,
                }),
                Component::Effect(EffectLeaf::DrawCards { amount: 1 }),
            ],
        };
        let card = Entity {
            id: id(1),
            components: vec![Component::Skill(ability)],
        };

        let skill = card.get::<Skill>().expect("the card prints one Skill");
        assert_eq!(
            skill.get::<Cost>(),
            Some(&Cost {
                generic: 1,
                ..Cost::default()
            })
        );
        assert_eq!(
            skill.all::<EffectLeaf>(),
            vec![
                &EffectLeaf::Heal {
                    amount: 5,
                    target: EffectTarget::Selected,
                },
                &EffectLeaf::DrawCards { amount: 1 },
            ]
        );
    }

    #[test]
    fn all_returns_every_effect_in_authored_order() {
        let entity = Entity {
            id: id(1),
            components: vec![
                Component::Effect(EffectLeaf::Heal {
                    amount: 5,
                    target: EffectTarget::Selected,
                }),
                Component::Effect(EffectLeaf::DrawCards { amount: 1 }),
            ],
        };
        assert_eq!(
            entity.all::<EffectLeaf>(),
            vec![
                &EffectLeaf::Heal {
                    amount: 5,
                    target: EffectTarget::Selected,
                },
                &EffectLeaf::DrawCards { amount: 1 },
            ]
        );
    }

    #[test]
    fn demand_returns_the_component_when_present() {
        let entity = Entity {
            id: id(1),
            components: vec![Component::Life(Life(40))],
        };
        assert_eq!(entity.demand::<Life>("destruction"), Ok(&Life(40)));
    }

    #[test]
    fn demand_returns_a_named_breakage_when_absent() {
        let entity = Entity {
            id: id(3),
            components: vec![],
        };
        assert_eq!(
            entity.demand::<RetreatCost>("retreat"),
            Err(Breakage {
                rule: "retreat",
                entity: id(3),
                expected: ComponentKind::RetreatCost,
            })
        );
    }

    #[test]
    fn get_and_attack_and_trigger_dispatch_to_their_own_markers() {
        let ability = Entity {
            id: id(20),
            components: vec![Component::Cost(Cost::default())],
        };
        let card = Entity {
            id: id(1),
            components: vec![
                Component::Attack(ability.clone()),
                Component::Trigger(ability.clone()),
            ],
        };
        assert_eq!(card.get::<Attack>(), Some(&ability));
        assert_eq!(card.get::<Trigger>(), Some(&ability));
        assert_eq!(card.get::<Skill>(), None);
    }

    #[test]
    fn every_component_variant_constructs_and_is_reachable_through_get() {
        let name = Entity {
            id: id(1),
            components: vec![Component::Name(Name("Whelp".to_string()))],
        };
        assert_eq!(name.get::<Name>(), Some(&Name("Whelp".to_string())));

        let accounting = Entity {
            id: id(2),
            components: vec![Component::AccountingId(AccountingId {
                prefix: "QRY".to_string(),
                number: 14,
            })],
        };
        assert_eq!(
            accounting.get::<AccountingId>(),
            Some(&AccountingId {
                prefix: "QRY".to_string(),
                number: 14,
            })
        );

        let form = Entity {
            id: id(3),
            components: vec![Component::Form(Form::Base)],
        };
        assert_eq!(form.get::<Form>(), Some(&Form::Base));

        let produces = Entity {
            id: id(4),
            components: vec![Component::Produces(ManaTypes(vec![ManaType::Matter]))],
        };
        assert_eq!(
            produces.get::<ManaTypes>(),
            Some(&ManaTypes(vec![ManaType::Matter]))
        );

        let tags = Entity {
            id: id(5),
            components: vec![Component::Tags(Tags(vec!["mount".to_string()]))],
        };
        assert_eq!(tags.get::<Tags>(), Some(&Tags(vec!["mount".to_string()])));

        let cost = Entity {
            id: id(6),
            components: vec![Component::Cost(Cost::default())],
        };
        assert_eq!(cost.get::<Cost>(), Some(&Cost::default()));

        let passive = Entity {
            id: id(7),
            components: vec![Component::Passive(Modifier::OpposingRetreatCostDelta(1))],
        };
        assert_eq!(
            passive.get::<Modifier>(),
            Some(&Modifier::OpposingRetreatCostDelta(1))
        );

        let timing = Entity {
            id: id(8),
            components: vec![Component::Timing(SpellTiming::Attack)],
        };
        assert_eq!(timing.get::<SpellTiming>(), Some(&SpellTiming::Attack));

        let event = Entity {
            id: id(9),
            components: vec![Component::Event(TriggerEvent::YourUpkeep)],
        };
        assert_eq!(event.get::<TriggerEvent>(), Some(&TriggerEvent::YourUpkeep));

        let respondable = Entity {
            id: id(10),
            components: vec![Component::Respondable],
        };
        assert_eq!(respondable.get::<Respondable>(), Some(&Respondable));

        let persistent = Entity {
            id: id(11),
            components: vec![Component::Persistent],
        };
        assert_eq!(persistent.get::<Persistent>(), Some(&Persistent));
    }

    /// `Persistent`'s presence, not a boolean payload, is what a resolved
    /// Spell or Enchantment carries to stay in play (rules §44) — the same
    /// shape `Respondable` uses for the same reason.
    #[test]
    fn persistent_presence_distinguishes_a_persisting_card_from_a_disposable_one() {
        let disposable = Entity {
            id: id(32),
            components: vec![Component::Name(Name("Ember Lance".to_string()))],
        };
        assert_eq!(disposable.get::<Persistent>(), None);

        let persisting = Entity {
            id: id(33),
            components: vec![
                Component::Name(Name("Standing Ward".to_string())),
                Component::Persistent,
            ],
        };
        assert_eq!(persisting.get::<Persistent>(), Some(&Persistent));
    }

    /// A Trigger's nested entity carries its event directly, and the
    /// `Respondable` marker's presence or absence — not a boolean payload —
    /// is what a rule reads to decide whether the trigger may be responded
    /// to (rules §37–38).
    #[test]
    fn respondable_presence_distinguishes_a_respondable_trigger_from_an_immediate_one() {
        let immediate = Entity {
            id: id(30),
            components: vec![Component::Event(TriggerEvent::EntersMain)],
        };
        assert_eq!(immediate.get::<Respondable>(), None);

        let respondable = Entity {
            id: id(31),
            components: vec![
                Component::Event(TriggerEvent::AnySummonDestroyed),
                Component::Respondable,
            ],
        };
        assert_eq!(respondable.get::<Respondable>(), Some(&Respondable));
    }
}
