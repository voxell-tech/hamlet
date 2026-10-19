//! The deeds that end an agent or a hamlet or change who runs one,
//! and the words of the question asked before each.

use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::name::Name;
use bevy::ecs::query::{With, Without};
use bevy::ecs::world::World;
use hamlet_core::agents::{
    self, Busy, Citizen, Left, LivesIn, Settlement,
};
use hamlet_core::oversight::Veto;
use hamlet_core::session::Rewind;
use hamlet_home::charter;
use hamlet_realm_api::orders;
use hamlet_sdk::confirm::Confirm;

use crate::chat;
use crate::me::Me;

/// Asks the chief before `deed` is done. The window answers it with
/// the question, at the pointer.
#[derive(Event, Clone, PartialEq)]
pub struct Ask(pub Deed);

/// The home's own charter was removed.
#[derive(Event, Clone, Copy)]
pub struct CharterReset;

/// Asks before `deed` is done.
pub fn ask(world: &mut World, deed: Deed) {
    world.trigger(Ask(deed));
    world.flush();
}

/// Stops what `agent` is doing now.
pub fn stop(world: &mut World, agent: Entity) {
    let agent = Some(agents::name_of(world, agent));
    let chief = world.resource::<Me>().user.clone();
    world.trigger(Veto { agent, chief });
    world.flush();
}

/// Something that ends an agent or a hamlet, or changes who runs
/// one.
#[derive(Clone, PartialEq)]
pub enum Deed {
    Remove(Entity),
    Merge {
        absorbed: Entity,
        kept: Entity,
    },
    /// Closes down the hamlet of this name.
    Close(String),
    /// Deletes what is archived of an agent that left, for good.
    Erase(Entity),
    /// Deletes what is archived of a hamlet that was closed down,
    /// for good.
    EraseHamlet(String),
    /// A command a plugin asked the chief to confirm.
    Command(Confirm),
    /// Puts the open conversation away and starts another.
    NewSession,
    /// Makes this agent the mayor of its hamlet.
    Mayor(Entity),
    /// Takes the open conversation back to just before the message
    /// `entry` of it.
    Rewind {
        session: Entity,
        entry: u64,
    },
    /// Removes the home's own charter, so the built-in one stands.
    DefaultCharter,
}

impl Deed {
    /// Gives the chief's command that does it to the hamlet it is
    /// about.
    pub fn run(&self, world: &mut World) {
        let name = |agent: Entity| agents::name_of(world, agent);
        match self {
            Deed::Remove(agent) => {
                let text = format!("/remove {}", name(*agent));
                orders::decree(world, *agent, text);
            }
            Deed::Merge { absorbed, kept } => {
                let text = format!(
                    "/merge {} into {}",
                    name(*absorbed),
                    name(*kept)
                );
                orders::decree(world, *kept, text);
            }
            Deed::Erase(agent) => {
                let text = format!("/erase {}", name(*agent));
                orders::decree(world, *agent, text);
            }
            Deed::Mayor(agent) => {
                let text = format!("/mayor {}", name(*agent));
                orders::decree(world, *agent, text);
            }
            // The hamlets and the library are the realm's, and the
            // open conversation is the open chat's.
            Deed::Close(hamlet) => {
                chat::send(world, format!("/remove-hamlet {hamlet}"))
            }
            Deed::EraseHamlet(hamlet) => {
                chat::send(world, format!("/erase-hamlet {hamlet}"))
            }
            Deed::Command(asked) => {
                chat::send(world, asked.command.clone())
            }
            Deed::NewSession => chat::send(world, "/new".into()),
            Deed::DefaultCharter => {
                let reset = world
                    .get_resource::<hamlet_core::core::HomeDir>()
                    .is_some_and(|home| {
                        charter::reset(&home.0.root).is_ok()
                    });
                if reset {
                    world.trigger(CharterReset);
                }
            }
            Deed::Rewind { session, entry } => {
                world.trigger(Rewind {
                    session: *session,
                    entry: *entry,
                });
                world.flush();
            }
        }
    }

    /// The question, in capitals.
    pub fn question(&self, world: &World) -> String {
        let name = |agent: &Entity| {
            agents::name_of(world, *agent).to_uppercase()
        };
        match self {
            Deed::Remove(agent) => format!("REMOVE {}?", name(agent)),
            Deed::Merge { absorbed, kept } => format!(
                "MERGE {} INTO {}?",
                name(absorbed),
                name(kept)
            ),
            Deed::Close(hamlet) => {
                format!("CLOSE DOWN {}?", hamlet.to_uppercase())
            }
            Deed::Erase(agent) => {
                format!("DELETE {} FOR GOOD?", name(agent))
            }
            Deed::EraseHamlet(hamlet) => {
                format!("DELETE {} FOR GOOD?", hamlet.to_uppercase())
            }
            Deed::Command(asked) => asked.question.clone(),
            Deed::NewSession => "START A NEW SESSION?".to_string(),
            Deed::Mayor(agent) => {
                format!("MAKE {} MAYOR?", name(agent))
            }
            Deed::Rewind { .. } => "REWIND TO HERE?".to_string(),
            Deed::DefaultCharter => {
                "GO BACK TO THE DEFAULT CHARTER?".to_string()
            }
        }
    }

    /// What follows if the chief says yes.
    pub fn outcome(&self, world: &World) -> String {
        let name = |agent: &Entity| agents::name_of(world, *agent);
        match self {
            Deed::Remove(agent) => format!(
                "The house of {} goes to the archive, with its \
                 notes, skills and log. It can be restored.",
                name(agent)
            ),
            Deed::Merge { absorbed, kept } => format!(
                "{} takes the notes, skills, files and roads of {}, \
                 whose house is archived. It can be split out again.",
                name(kept),
                name(absorbed)
            ),
            Deed::Close(hamlet) => format!(
                "Everyone in {hamlet} stops and every house is \
                 archived. You can restore it from the list of \
                 hamlets."
            ),
            Deed::Erase(agent) => {
                let name = name(agent);
                format!(
                    "The archived house of {name} is deleted from \
                     disk. This cannot be undone."
                )
            }
            Deed::NewSession => "This conversation goes to the \
                                 Archive, where it can be read and \
                                 resumed. The agent keeps its notes \
                                 and skills."
                .to_string(),
            Deed::Command(asked) => asked.outcome.clone(),
            Deed::EraseHamlet(hamlet) => format!(
                "Everything archived of {hamlet} is deleted from \
                 disk. This cannot be undone."
            ),
            Deed::Mayor(agent) => format!(
                "{} runs the hamlet from now on, and today's mayor \
                 becomes one of its agents.",
                name(agent)
            ),
            Deed::Rewind { .. } => format!(
                "{} forgets this message and everything after it, \
                 and the message goes back in the field. Changed \
                 files, commands that ran, letters and spending are \
                 not undone.",
                name(&world.resource::<Me>().agent)
            ),
            Deed::DefaultCharter => "Your own charter is deleted \
                                     from disk. This cannot be \
                                     undone."
                .to_string(),
        }
    }

    /// The lettering of the button that says yes.
    pub fn verb(&self) -> String {
        match self {
            Deed::Remove(_) => "REMOVE".into(),
            Deed::Merge { .. } => "MERGE".into(),
            Deed::Close(_) => "CLOSE DOWN".into(),
            Deed::Erase(_) | Deed::EraseHamlet(_) => "DELETE".into(),
            Deed::Command(asked) => asked.verb.clone(),
            Deed::NewSession => "NEW SESSION".into(),
            Deed::Mayor(_) => "MAKE MAYOR".into(),
            Deed::Rewind { .. } => "REWIND".into(),
            Deed::DefaultCharter => "USE THE DEFAULT".into(),
        }
    }

    /// The agents it ends or changes that are at work.
    pub fn working(&self, world: &mut World) -> Vec<String> {
        let busy = |world: &World, agents: &[Entity]| {
            agents
                .iter()
                .filter(|agent| world.get::<Busy>(**agent).is_some())
                .map(|agent| agents::name_of(world, *agent))
                .collect()
        };
        match self {
            Deed::Remove(agent) => busy(world, &[*agent]),
            Deed::Merge { absorbed, kept } => {
                busy(world, &[*absorbed, *kept])
            }
            Deed::Erase(_)
            | Deed::EraseHamlet(_)
            | Deed::Command(_)
            | Deed::NewSession
            | Deed::Mayor(_)
            | Deed::Rewind { .. }
            | Deed::DefaultCharter => Vec::new(),
            Deed::Close(hamlet) => {
                let mut living = world
                    .query_filtered::<(&Name, &LivesIn), (
                        With<Citizen>,
                        With<Busy>,
                        Without<Left>,
                    )>();
                living
                    .iter(world)
                    .filter(|(_, lives)| {
                        world.get::<Settlement>(lives.0).is_some_and(
                            |settlement| settlement.name == *hamlet,
                        )
                    })
                    .map(|(name, _)| name.as_str().to_string())
                    .collect()
            }
        }
    }
}
