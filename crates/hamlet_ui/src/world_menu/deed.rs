//! The deeds that end an agent or a hamlet or change who runs one,
//! and the question asked before each.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{With, Without};
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_core::agents::{
    self, Busy, Citizen, Left, LivesIn, Settlement,
};
use hamlet_core::session::Rewind;

use super::close;
use crate::feel::Mood;
use crate::looks::{brass, grim, lettered};
use crate::me::Me;
use crate::platform::DisplayText;
use crate::{AnyView, Cx, chat, hand};

/// Something that ends an agent or a hamlet, or changes who runs
/// one.
#[derive(Clone, PartialEq)]
pub(crate) enum Deed {
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
    /// Takes a skill out of the library.
    Unshelve(String),
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
    pub(crate) fn run(&self, world: &mut World) {
        let name = |agent: Entity| agents::name_of(world, agent);
        match self {
            Deed::Remove(agent) => {
                let text = format!("/remove {}", name(*agent));
                hand::decree(world, *agent, text);
            }
            Deed::Merge { absorbed, kept } => {
                let text = format!(
                    "/merge {} into {}",
                    name(*absorbed),
                    name(*kept)
                );
                hand::decree(world, *kept, text);
            }
            Deed::Erase(agent) => {
                let text = format!("/erase {}", name(*agent));
                hand::decree(world, *agent, text);
            }
            Deed::Mayor(agent) => {
                let text = format!("/mayor {}", name(*agent));
                hand::decree(world, *agent, text);
            }
            // The hamlets and the library are the realm's, and the
            // open conversation is the open chat's.
            Deed::Close(hamlet) => {
                chat::send(world, format!("/remove-hamlet {hamlet}"))
            }
            Deed::EraseHamlet(hamlet) => {
                chat::send(world, format!("/erase-hamlet {hamlet}"))
            }
            Deed::Unshelve(skill) => {
                chat::send(world, format!("/unshelve {skill}"))
            }
            Deed::NewSession => chat::send(world, "/new".into()),
            Deed::DefaultCharter => {
                crate::settings::default_charter(world)
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

    fn question(&self, world: &World) -> String {
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
            Deed::Unshelve(skill) => format!(
                "TAKE {} OUT OF THE LIBRARY?",
                skill.to_uppercase()
            ),
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

    fn outcome(&self, world: &World) -> String {
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
            Deed::Unshelve(skill) => format!(
                "{skill} is deleted from the library for every \
                 hamlet. This cannot be undone."
            ),
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

    fn verb(&self) -> &'static str {
        match self {
            Deed::Remove(_) => "REMOVE",
            Deed::Merge { .. } => "MERGE",
            Deed::Close(_) => "CLOSE DOWN",
            Deed::Erase(_) | Deed::EraseHamlet(_) => "DELETE",
            Deed::Unshelve(_) => "TAKE OUT",
            Deed::NewSession => "NEW SESSION",
            Deed::Mayor(_) => "MAKE MAYOR",
            Deed::Rewind { .. } => "REWIND",
            Deed::DefaultCharter => "USE THE DEFAULT",
        }
    }

    /// The agents it ends or changes that are at work.
    fn working(&self, world: &mut World) -> Vec<String> {
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
            | Deed::Unshelve(_)
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

/// The question before `deed`, with a warning for each agent that is
/// at work.
pub(crate) fn question(deed: Deed, at: Vec2) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let working = deed.working(cx.world);
        let warning = match working.as_slice() {
            [] => frame().boxed(),
            names => label(format!(
                "{} {} working right now. What it is doing will be \
                 cut short.",
                names.join(" and "),
                if names.len() == 1 { "is" } else { "are" }
            ))
            .size(13.0)
            .tone(Tone::Critical)
            .boxed(),
        };
        let done = deed.clone();
        let body = column((
            label(deed.question(cx.world))
                .size(13.0)
                .bold(true)
                .tagged(DisplayText),
            label(deed.outcome(cx.world)).size(13.0).tone(Tone::Dim),
            warning,
            row((
                lettered(deed.verb())
                    .rules(grim)
                    .tagged(Mood::Bad)
                    .on_activate(move |world| {
                        close(world);
                        done.run(world);
                    }),
                lettered("CANCEL").rules(brass).on_activate(close),
            ))
            .gap(8.0),
        ))
        .gap(10.0)
        .width(px(300.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(bevy::ui::UiRect::all(px(14.0))),
        )
    })
}
