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
use hamlet_client::messages::Rewind;
use hamlet_client::realm::{
    self as agents, AgentNumber, Busy, Deeds, HamletId, Left, LivesIn,
};
use hamlet_realm::orders;
use hamlet_ui::styles::{
    button, danger_button, description, lettered,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{View, built};

use super::close;
use crate::chat;
use crate::convo::Convo;

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
        entry: String,
    },
    /// Removes the home's own charter, so the built-in one stands.
    DefaultCharter,
}

impl Deed {
    /// Gives the chief's command that does it to the hamlet it is
    /// about.
    pub(crate) fn run(&self, world: &mut World) {
        if !self.is_available(world) {
            return;
        }
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
            Deed::Close(hamlet) => chat::decree(
                world,
                hamlet,
                format!("/remove-hamlet {hamlet}"),
            ),
            Deed::Unshelve(skill) => chat::decree(
                world,
                agents::HOME,
                format!("/unshelve {skill}"),
            ),
            Deed::NewSession => chat::send(world, "/new".into()),
            Deed::DefaultCharter => {
                crate::settings::default_charter(world);
            }
            Deed::Rewind { session, entry } => {
                world.trigger(Rewind {
                    chat: *session,
                    item: entry.clone(),
                });
                world.flush();
            }
        }
    }

    fn is_available(&self, world: &World) -> bool {
        let command = match self {
            Self::Remove(_) => "remove",
            Self::Merge { .. } => "merge",
            Self::Close(_) => "remove-hamlet",
            Self::Erase(_) => "erase",
            Self::Unshelve(_) => "unshelve",
            Self::Mayor(_) => "mayor",
            Self::NewSession
            | Self::DefaultCharter
            | Self::Rewind { .. } => return true,
        };
        Deeds::is_allowed(world, command)
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
                name(&world.resource::<Convo>().agent)
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
            Deed::Erase(_) => "DELETE",
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
            | Deed::Unshelve(_)
            | Deed::NewSession
            | Deed::Mayor(_)
            | Deed::Rewind { .. }
            | Deed::DefaultCharter => Vec::new(),
            Deed::Close(hamlet) => {
                let mut living =
                    world.query_filtered::<(&Name, &LivesIn), (
                        With<AgentNumber>,
                        With<Busy>,
                        Without<Left>,
                    )>();
                living
                    .iter(world)
                    .filter(|(_, lives)| {
                        world
                            .get::<HamletId>(lives.0)
                            .is_some_and(|id| id.0 == *hamlet)
                    })
                    .map(|(name, _)| name.as_str().to_string())
                    .collect()
            }
        }
    }
}

/// The question before `deed`, with a warning for each agent that is
/// at work.
pub(crate) fn question(deed: Deed, at: Vec2) -> impl View {
    built(move |cx| {
        if !deed.is_available(cx.world) {
            return cx.build(frame());
        }
        let working = deed.working(cx.world);
        let warning = match working.as_slice() {
            [] => frame().boxed(),
            names => label(format!(
                "{} {} working right now. What it is doing will be \
                 cut short.",
                names.join(" and "),
                if names.len() == 1 { "is" } else { "are" }
            ))
            .tone(Tone::Critical)
            .boxed(),
        };
        let done = deed.clone();
        let (gap, section, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.gap,
                theme.space.section,
                theme.space.overlay_padding,
                theme.text.heading,
            )
        };
        let body = column((
            label(deed.question(cx.world))
                .size(heading)
                .bold(true)
                .tagged(DisplayText),
            description(deed.outcome(cx.world)),
            warning,
            row((
                lettered(deed.verb())
                    .rules(danger_button)
                    .tagged(Variant::Danger)
                    .on_activate(move |world| {
                        close(world);
                        done.run(world);
                    }),
                lettered("CANCEL").rules(button).on_activate(close),
            ))
            .gap(gap),
        ))
        .gap(section)
        .width(px(300.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(bevy::ui::UiRect::all(px(padding))),
        )
    })
}

#[cfg(test)]
mod tests {
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;

    use super::*;

    #[test]
    fn a_deed_goes_to_the_mayor_of_the_agents_received_hamlet() {
        let mut app = crate::testing::panel_app();
        app.add_plugins(chat::plugin);
        let mut state = hamlet_testing::realm::state();
        state.deeds = hamlet_testing::realm::deeds(&["remove"]);
        state.hamlets.push(hamlet_testing::realm::hamlet("mill"));
        state
            .agents
            .push(hamlet_testing::realm::agent("mill", 0, "miller"));
        state
            .agents
            .push(hamlet_testing::realm::agent("mill", 1, "carder"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        // The chief's seat opened the mayor's chat.
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        let carder =
            agents::named(app.world_mut(), "carder").unwrap();
        Deed::Remove(carder).run(app.world_mut());
        app.world_mut().flush();
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one command: {sent:?}");
        };
        assert_eq!(
            say.to,
            protocol::Address {
                hamlet: "mill".into(),
                agent: "mayor".into(),
            }
        );
        assert_eq!(say.text, "/remove carder");
    }
}
