use summoners_core::domain::cards::{Component, Modifier, SpellTiming, TriggerEvent};

pub(crate) fn modifier(component: &Component) -> Option<String> {
    match component {
        Component::Passive(Modifier::OpposingRetreatCostDelta(amount)) => {
            Some(format!("opposing retreat cost {amount:+}"))
        }
        Component::Passive(Modifier::IncomingAttackDamageReduction(amount)) => {
            Some(format!("reduce incoming attack damage by {amount}"))
        }
        Component::Name(_)
        | Component::AccountingId(_)
        | Component::Life(_)
        | Component::RetreatCost(_)
        | Component::Form(_)
        | Component::Produces(_)
        | Component::Tags(_)
        | Component::Cost(_)
        | Component::Skill(_)
        | Component::Attack(_)
        | Component::Trigger(_)
        | Component::Effect(_)
        | Component::Timing(_)
        | Component::Event(_)
        | Component::Respondable
        | Component::Persistent => None,
    }
}
pub(crate) fn timing(component: &Component) -> Option<String> {
    match component {
        Component::Timing(SpellTiming::Support) => Some("support".to_string()),
        Component::Timing(SpellTiming::Attack) => Some("attack".to_string()),
        Component::Name(_)
        | Component::AccountingId(_)
        | Component::Life(_)
        | Component::RetreatCost(_)
        | Component::Form(_)
        | Component::Produces(_)
        | Component::Tags(_)
        | Component::Cost(_)
        | Component::Skill(_)
        | Component::Attack(_)
        | Component::Trigger(_)
        | Component::Effect(_)
        | Component::Passive(_)
        | Component::Event(_)
        | Component::Respondable
        | Component::Persistent => None,
    }
}
pub(crate) fn event(component: &Component) -> Option<String> {
    match component {
        Component::Event(event) => Some(trigger_event(*event).to_string()),
        Component::Name(_)
        | Component::AccountingId(_)
        | Component::Life(_)
        | Component::RetreatCost(_)
        | Component::Form(_)
        | Component::Produces(_)
        | Component::Tags(_)
        | Component::Cost(_)
        | Component::Skill(_)
        | Component::Attack(_)
        | Component::Trigger(_)
        | Component::Effect(_)
        | Component::Passive(_)
        | Component::Timing(_)
        | Component::Respondable
        | Component::Persistent => None,
    }
}
pub(crate) fn trigger_event(event: TriggerEvent) -> &'static str {
    match event {
        TriggerEvent::YourUpkeep => "your upkeep",
        TriggerEvent::EntersMain => "enters Main",
        TriggerEvent::EntersBench => "enters Bench",
        TriggerEvent::LeavesMain => "leaves Main",
        TriggerEvent::LeavesBench => "leaves Bench",
        TriggerEvent::AnySummonDestroyed => "any Summon destroyed",
    }
}
pub(crate) fn respondable(component: &Component) -> bool {
    matches!(component, Component::Respondable)
}
pub(crate) fn persistent(component: &Component) -> bool {
    matches!(component, Component::Persistent)
}
