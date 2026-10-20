//! The agents service as the panels read it: each request remembered
//! by its ticket until its reply comes, and what every panel follows
//! kept as resources of the plugin's world.

use std::collections::HashMap;

use hamlet_engine_sdk::api::agents::{Focus, Need, Roster, Skill};
use hamlet_engine_sdk::{
    AgentsEvent, AgentsReply, AgentsRequest, Failure, Message, Reply,
    Ticket, World, agents,
};

use crate::change::Changes;
use crate::confirm::Confirming;

/// The chat's panel, of the plugin `hamlet.chat`.
const CHAT: &str = "hamlet.chat/chat";

/// Brings the chat to the front.
pub fn show_chat() {
    hamlet_engine_sdk::ui::open(CHAT);
}

/// The requests asked and not yet answered, by their tickets.
#[derive(Default)]
pub struct Asked(HashMap<Ticket, AgentsRequest>);

/// The window's focus, as last told.
#[derive(Default)]
pub struct Focused(pub Option<Focus>);

/// What waits for the chief, oldest first.
#[derive(Default)]
pub struct Waits(pub Vec<Need>);

/// The library's skills.
#[derive(Default)]
pub struct Shelves(pub Vec<Skill>);

/// What a message from the agents service came to.
pub enum Heard {
    /// The reply to a request, with the request when it was asked
    /// through [`ask`].
    Reply(Option<AgentsRequest>, AgentsReply),
    Failed(Option<AgentsRequest>, Failure),
    Event(AgentsEvent),
}

/// Puts in each resource this crate keeps, so views can read them.
pub fn init(world: &mut World) {
    world.init_resource::<Asked>();
    world.init_resource::<Focused>();
    world.init_resource::<Roster>();
    world.init_resource::<Waits>();
    world.init_resource::<Shelves>();
    world.init_resource::<Changes>();
    world.init_resource::<Confirming>();
}

/// Watches the agents service and asks for `first`, for a plugin
/// that has no world at hand, as it declares what it adds. Their
/// replies are known by what they hold.
pub fn start(first: &[AgentsRequest]) {
    agents(&AgentsRequest::Watch);
    for request in first {
        agents(request);
    }
}

/// Asks the agents service `request`, remembered for its reply.
pub fn ask(world: &mut World, request: AgentsRequest) {
    let ticket = agents(&request);
    if ticket != 0 {
        world.resource_or_init::<Asked>().0.insert(ticket, request);
    }
}

/// Takes in `message`: what it tells of the focus, the roster, the
/// needs and the library is kept, and what it came to is given back
/// for the plugin's own.
pub fn hear(world: &mut World, message: Message) -> Option<Heard> {
    let Message::Reply { ticket, reply } = message else {
        return None;
    };
    let asked = world.resource_or_init::<Asked>().0.remove(&ticket);
    match reply {
        Ok(Reply::Agents(AgentsReply::Event(event))) => {
            keep_event(world, &event);
            Some(Heard::Event(event))
        }
        Ok(Reply::Agents(reply)) => {
            keep_reply(world, &reply);
            Some(Heard::Reply(asked, reply))
        }
        Ok(_) => None,
        Err(why) => Some(Heard::Failed(asked, why)),
    }
}

fn keep_reply(world: &mut World, reply: &AgentsReply) {
    match reply {
        AgentsReply::Focus(focus) => keep_focus(world, focus),
        AgentsReply::Roster(roster) => keep_roster(world, roster),
        AgentsReply::Needs(needs) => keep_needs(world, needs),
        AgentsReply::Skills(skills) => keep_skills(world, skills),
        _ => {}
    }
}

fn keep_event(world: &mut World, event: &AgentsEvent) {
    match event {
        AgentsEvent::Focus(focus) => keep_focus(world, focus),
        AgentsEvent::Roster(roster) => keep_roster(world, roster),
        AgentsEvent::Needs(needs) => keep_needs(world, needs),
        AgentsEvent::Skills(skills) => keep_skills(world, skills),
        _ => {}
    }
}

// Each is changed only when it differs, so the views that read it
// are not looked at again for nothing.

fn keep_focus(world: &mut World, focus: &Focus) {
    if world.get::<Focused>().and_then(|kept| kept.0.as_ref())
        != Some(focus)
    {
        world.insert(Focused(Some(focus.clone())));
    }
}

fn keep_roster(world: &mut World, roster: &Roster) {
    if world.get::<Roster>() != Some(roster) {
        world.insert(roster.clone());
    }
}

fn keep_needs(world: &mut World, needs: &[Need]) {
    if world.get::<Waits>().map(|waits| waits.0.as_slice())
        != Some(needs)
    {
        world.insert(Waits(needs.to_vec()));
    }
}

fn keep_skills(world: &mut World, skills: &[Skill]) {
    if world.get::<Shelves>().map(|shelves| shelves.0.as_slice())
        != Some(skills)
    {
        world.insert(Shelves(skills.to_vec()));
    }
}
