//! The Hamlets stall's purchases: a hamlet founded from its template
//! with all it comes with, selected and shown in the realm.

use agents_ui::{AgentsWorldExt, FailureExt, Heard, Rostered};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, RealmLimits, Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::open;

use crate::samples::hamlets::Template;
use crate::shelf::{Said, Stock};
use crate::{BazaarWorldExt, Stall};

/// The realm's panel, of the plugin `hamlet.realm`.
const REALM: &str = "hamlet.realm/realm";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Founding>().init_resource::<Most>();
}

/// The hamlet being founded, by its name.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Founding(pub Option<String>);

/// The most hamlets the realm's limits allow, as last told.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Most(pub Option<u64>);

/// True when the realm has the hamlet `name`.
pub(crate) fn owns(world: &World, name: &str) -> bool {
    world
        .resource::<Rostered>()
        .0
        .hamlets
        .iter()
        .any(|hamlet| hamlet.id == name)
}

/// The reason no more hamlets can be founded, when the realm is
/// full.
pub(crate) fn full(world: &World) -> Option<String> {
    let most = world.resource::<Most>().0?;
    let count = world.resource::<Rostered>().0.hamlets.len() as u64;
    (count >= most).then(|| {
        format!(
            "Your realm holds as many hamlets as your limits allow, \
             {count} of {most}. Raise Most hamlets in Settings, \
             under Realm, or close a hamlet to make room."
        )
    })
}

/// Says `words` at the Hamlets stall.
fn say(world: &mut World, words: impl Into<String>, wrong: bool) {
    world.resource_mut::<Said<Template>>().say(words, wrong);
}

/// Founds the hamlet `name` from its template.
pub(crate) fn buy(world: &mut World, name: &str) {
    if world.resource::<Founding>().0.is_some() || owns(world, name) {
        return;
    }
    if let Some(why) = full(world) {
        return say(world, why, true);
    }
    let Some(template) = world
        .resource::<Stock<Template>>()
        .get(&name.to_string())
        .copied()
    else {
        return say(world, "That hamlet is no longer offered.", true);
    };
    world.resource_mut::<Founding>().0 = Some(name.to_string());
    world.resource_mut::<Said<Template>>().0 = None;
    world.ask(AgentsRequest::FoundFrom(template.founded()));
}

/// Selects the hamlet `name` and shows it in the realm.
pub(crate) fn visit(world: &mut World, name: &str) {
    world.ask(AgentsRequest::Select(Selected::Hamlet(
        name.to_string(),
    )));
    open(REALM);
}

/// Takes in what the agents service says of the realm's limits and a
/// hamlet founded.
pub(crate) fn heard(world: &mut World, told: &Heard) {
    match told {
        Heard::Reply(_, AgentsReply::Limits(limits))
        | Heard::Event(AgentsEvent::Limits(limits)) => {
            let RealmLimits { most_hamlets, .. } = limits;
            world.keep(Most(Some(*most_hamlets)));
        }
        Heard::Reply(
            Some(AgentsRequest::FoundFrom(template)),
            AgentsReply::Done,
        ) => {
            world.resource_mut::<Founding>().0 = None;
            world.traded(Stall::Hamlets);
            let agents = match template.agents.len() {
                1 => "one agent".to_string(),
                count => format!("{count} agents"),
            };
            let words = format!(
                "{} stands in your realm, chief. {} keeps it, with \
                 {agents} and all they know.",
                template.title, template.mayor.name,
            );
            say(world, words, false);
            visit(world, &template.name);
        }
        Heard::Failed(
            Some(AgentsRequest::FoundFrom(template)),
            why,
        ) => {
            world.resource_mut::<Founding>().0 = None;
            let words = format!(
                "{} could not be founded: {}",
                template.title,
                why.plain()
            );
            say(world, words, true);
        }
        _ => {}
    }
}
