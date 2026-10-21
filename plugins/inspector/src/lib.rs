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

use agents_ui::{
    AgentsWorldExt, CONFIRM, Focused, Heard, confirmation, either,
    moved, start,
};
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Roster, RosterAgent,
    Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ViewExt, World, derived, keyed,
};
use hamlet_engine_sdk::{Entry, Message, Plugin, decode, encode};

use crate::founding::{FOUND, Founding};
use crate::hamlet::Naming;
use crate::overview::Search;

/// The characters of a purpose shown.
pub(crate) const PURPOSE: usize = 96;

/// What is typed in the Inspector's fields, kept across a reload:
/// the founding form, the search of the hamlets and a rename.
type Kept = (
    (String, String, String),
    Option<String>,
    Option<(String, String)>,
);

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
    let roster = world.resource::<Roster>();
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
        .resource::<Roster>()
        .agents
        .iter()
        .find(|agent| agent.name == name)
}

#[derive(Default)]
struct Inspector;

/// Puts in what the Inspector's views read.
fn prepare(world: &mut World) {
    world.init_agents();
    agent::prepare(world);
    archive::prepare(world);
    overview::prepare(world);
    founding::prepare(world);
    hamlet::prepare(world);
    bylaws::prepare(world);
}

impl Plugin for Inspector {
    fn declare(&self, entry: &mut Entry) {
        entry
            .panel("inspector", "Inspector")
            .overlay(CONFIRM)
            .overlay(FOUND)
            .command(FOUND, "Found a Hamlet...")
            .menu_entry("Hamlet", FOUND);
        start(&[
            AgentsRequest::Focus,
            AgentsRequest::Roster,
            AgentsRequest::Skills,
            AgentsRequest::Settings,
            AgentsRequest::Catalogue,
        ]);
    }

    fn mount(&mut self, name: &str, world: &mut World) -> AnyView {
        prepare(world);
        match name {
            CONFIRM => return confirmation(),
            FOUND => return founding::founding(),
            _ => {}
        }
        keyed(
            derived(page)
                .when(either(moved::<Focused>(), moved::<Roster>())),
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

    fn run(&mut self, name: &str, _: &mut World) {
        if name == FOUND {
            founding::open();
        }
    }

    fn save(&self, world: &World) -> Vec<u8> {
        let founding =
            world.get::<Founding>().cloned().unwrap_or_default();
        let kept: Kept = (
            (founding.name, founding.mayor, founding.purpose),
            world.get::<Search>().map(|search| search.0.clone()),
            world.get::<Naming>().and_then(|naming| naming.0.clone()),
        );
        encode(&kept).unwrap_or_default()
    }

    fn restore(&mut self, state: &[u8], world: &mut World) {
        let Ok(((name, mayor, purpose), search, naming)) =
            decode::<Kept>(state)
        else {
            return;
        };
        world.insert(Founding {
            name,
            mayor,
            purpose,
        });
        world.insert(Search(search.unwrap_or_default()));
        world.insert(Naming(naming));
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        prepare(world);
        let Some(told) = world.hear(message) else {
            return;
        };
        match &told {
            Heard::Failed(Some(request), why) => {
                hamlet_engine_sdk::warn(&format!("{request:?}: {why}"));
            }
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

hamlet_engine_sdk::export!(Inspector);
