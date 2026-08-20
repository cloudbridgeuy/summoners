use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Set {
    pub(crate) schema_version: u32,
    pub(crate) id: String,
    pub(crate) revision: u32,
    pub(crate) name: String,
    pub(crate) cards: Vec<Card>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Card {
    pub(crate) code: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) text: String,
    pub(crate) kind: CardKind,
    pub(crate) form: Option<Form>,
    pub(crate) life: Option<u32>,
    pub(crate) types: Option<Vec<ManaType>>,
    pub(crate) retreat: Option<u32>,
    pub(crate) timing: Option<SpellTiming>,
    pub(crate) persistence: Option<Persistence>,
    pub(crate) cost: Option<Vec<ManaSymbol>>,
    #[serde(default)]
    pub(crate) abilities: Vec<Ability>,
    #[serde(default)]
    pub(crate) effects: Vec<Effect>,
    #[serde(default)]
    pub(crate) modifiers: Vec<Modifier>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ability {
    pub(crate) code: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) text: String,
    pub(crate) kind: AbilityKind,
    pub(crate) cost: Option<Vec<ManaSymbol>>,
    pub(crate) event: Option<TriggerEvent>,
    pub(crate) response: Option<ResponseMode>,
    #[serde(default)]
    pub(crate) effects: Vec<Effect>,
    pub(crate) modifier: Option<Modifier>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DamageAddition {
    pub(crate) amount: u32,
    pub(crate) condition: Condition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum CardKind {
    Summon,
    Spell,
    Enchantment,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Form {
    Base,
    Enhanced,
    Elite,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManaType {
    Matter,
    Mind,
    Spirit,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ManaSymbol {
    Matter,
    Mind,
    Spirit,
    Generic,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SpellTiming {
    Support,
    Attack,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Persistence {
    Discard,
    Persistent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AbilityKind {
    Attack,
    Skill,
    Passive,
    Trigger,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TriggerEvent {
    YourUpkeep,
    EntersMain,
    EntersBench,
    LeavesMain,
    LeavesBench,
    AnySummonDestroyed,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResponseMode {
    Immediate,
    Respondable,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum DamageConstraint {
    Unincreasable,
    Unpreventable,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Target {
    DefendingMain,
    SelectedOpposingPosition,
    Source,
    SelectedOwnSummon,
    SelectedOwnBenchedSummon,
    OwnMainWithSelectedBench,
    OpposingMainWithSelectedBench,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Destination {
    EmptyOwnBench,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Duration {
    UntilYourNextTurn,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ResponseBlock {
    AttackSpells,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RelativePlayer {
    Controller,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Condition {
    DefenderEnteredMainThisTurn { player: RelativePlayer },
    SpellPlayedThisTurn { player: RelativePlayer },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Modifier {
    OpposingRetreatCost { amount: u32 },
    IncomingAttackDamageReduction { amount: u32 },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Effect {
    Damage {
        target: Target,
        base: u32,
        #[serde(default)]
        constraints: Vec<DamageConstraint>,
        #[serde(default)]
        additions: Vec<DamageAddition>,
    },
    Heal {
        target: Target,
        amount: u32,
    },
    MoveSummon {
        target: Target,
        destination: Destination,
    },
    SwapPositions {
        target: Target,
    },
    BlockResponses {
        condition: Condition,
        response: ResponseBlock,
    },
    ReturnSpellFromDiscard,
    LookAtPrizes,
    DrawCards {
        amount: u32,
    },
    ReturnSpellToDeckTop,
    ProduceMana {
        target: Target,
    },
    CannotBeMovedByOpponent {
        target: Target,
        duration: Duration,
    },
    ReadySummon {
        target: Target,
    },
}
