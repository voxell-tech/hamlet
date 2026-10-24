//! The Inspector: whatever the window's focus has selected. An agent
//! with its notes, character, archive and prompt; one that is gone;
//! the library; a hamlet with its agents, bylaws and record; or every
//! hamlet when nothing is selected.

mod agent;
mod archive;
mod bylaws;
mod founding;
mod gone;
mod hamlet;
mod more;
mod overview;

use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, RosterAgent, Selected,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Note, ViewExt, derived, keyed, notify,
};

use crate::inspector::founding::FOUND;
use crate::shared::{
    AgentsWorldExt, Focused, Heard, Rostered, either, moved, start,
};

/// The characters of a purpose shown.
pub(crate) const PURPOSE: usize = 96;

/// The name of the page of every hamlet, at the start of each
/// breadcrumb.
pub(crate) const HAMLETS: &str = "HAMLETS";

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        agent::plugin,
        archive::plugin,
        overview::plugin,
        founding::plugin,
        hamlet::plugin,
        bylaws::plugin,
    ))
    .add_panel("inspector", "Inspector", inspector)
    .add_overlay(FOUND, founding::founding)
    .add_command(FOUND, "Found a Hamlet...", founding::open)
    .add_menu_entry("Hamlet", FOUND)
    .add_systems(
        Declare,
        start(&[
            AgentsRequest::Focus,
            AgentsRequest::Roster,
            AgentsRequest::Skills,
            AgentsRequest::Settings,
            AgentsRequest::Catalogue,
        ]),
    )
    .add_systems(Update, receive);
}

/// The page the Inspector shows.
#[derive(Clone, PartialEq, Debug)]
enum Page {
    Overview,
    Library,
    Hamlet(String),
    Gone(String),
    Agent,
}

/// The page for what the focus has selected.
fn page(world: &World) -> Page {
    let roster = &world.resource::<Rostered>().0;
    let selected = world
        .resource::<Focused>()
        .0
        .as_ref()
        .map(|focus| focus.selected.clone());
    match selected {
        // The chief's own page is the realm's to show.
        None | Some(Selected::Nothing | Selected::Hearth) => {
            Page::Overview
        }
        Some(Selected::Library) => Page::Library,
        Some(Selected::Hamlet(id)) => {
            if roster.hamlets.iter().any(|hamlet| hamlet.id == id) {
                Page::Hamlet(id)
            } else {
                Page::Overview
            }
        }
        Some(Selected::Agent(name)) => {
            let living = roster
                .agents
                .iter()
                .any(|agent| agent.name == name && !agent.gone);
            // One that came back is shown as itself again.
            if !living
                && roster
                    .agents
                    .iter()
                    .any(|agent| agent.name == name)
            {
                Page::Gone(name)
            } else {
                Page::Agent
            }
        }
    }
}

/// Selects `thing`.
pub(crate) fn select(world: &mut World, thing: Selected) {
    world.ask(AgentsRequest::Select(thing));
}

/// The agent `name` of the roster.
pub(crate) fn agent_named<'a>(
    world: &'a World,
    name: &str,
) -> Option<&'a RosterAgent> {
    world
        .resource::<Rostered>()
        .0
        .agents
        .iter()
        .find(|agent| agent.name == name)
}

fn inspector() -> AnyView {
    keyed(
        derived(page)
            .when(either(moved::<Focused>(), moved::<Rostered>())),
        |page| match page {
            Page::Overview => overview::overview(),
            Page::Library => gone::library(),
            Page::Hamlet(id) => hamlet::hamlet(id.clone()),
            Page::Gone(name) => gone::gone(name.clone()),
            Page::Agent => agent::living(),
        },
    )
    .boxed()
}

fn receive(
    world: &mut World,
    mut cursor: Local<MessageCursor<Heard>>,
    mut shown: Local<Option<Page>>,
) {
    // A page that changed lights the panel's top, whether or not
    // the panel is the one in front.
    let now = page(world);
    if shown.replace(now.clone()) != Some(now) && shown.is_some() {
        notify("inspector", Note::Updated);
    }
    for told in world.heard(&mut cursor) {
        match &told {
            Heard::Reply(_, AgentsReply::Focus(_))
            | Heard::Event(AgentsEvent::Focus(_))
            // A turn that ended may have written its papers.
            | Heard::Event(AgentsEvent::Working { working: false, .. }) => {
                agent::follow(world);
            }
            Heard::Event(AgentsEvent::Roster(_)) => {
                if matches!(page(world), Page::Hamlet(_)) {
                    hamlet::follow(world);
                    bylaws::follow(world);
                } else {
                    hamlet::leave(world);
                    bylaws::leave(world);
                }
            }
            _ => {}
        }
        agent::heard(world, &told);
        archive::heard(world, &told);
        hamlet::heard(world, &told);
        bylaws::heard(world, &told);
    }
}
