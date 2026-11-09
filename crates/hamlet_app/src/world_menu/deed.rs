//! The deeds that end an agent or a hamlet or change who runs one,
//! and the question asked before each.

use bevy::ecs::entity::Entity;
use bevy::ecs::query::{With, Without};
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::ui::px;
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, corners, frame, label, popup,
    row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_client::messages::Rewind;
use hamlet_client::realm::{
    self as agents, AgentNumber, Busy, Deeds, HamletId, Left, LivesIn,
};
use hamlet_realm::orders;
use hamlet_ui::styles::{
    button, danger_button, lettered, merge_button,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::{Cx, View, built};

use super::close;
use crate::chat;
use crate::convo::Convo;

/// Something that ends an agent or a hamlet, or changes who runs
/// one.
#[derive(Clone, PartialEq)]
pub(crate) enum Deed {
    End(Entity),
    Merge {
        absorbed: Entity,
        kept: Entity,
    },
    /// Closes down the hamlet of this id.
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
        /// The time of the message, in milliseconds since 1970.
        at: u64,
    },
    /// Ends the realm's own charter, so the built-in one stands.
    DefaultCharter,
    /// Copies every lodge to the saved workspace, over the lodges
    /// there.
    MoveLodges,
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
            Deed::End(agent) => {
                let text = format!(
                    "/end {} ended by the chief",
                    name(*agent)
                );
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
                let text = format!(
                    "/mayor {} chosen by the chief",
                    name(*agent)
                );
                orders::decree(world, *agent, text);
            }
            // The hamlets and the library are the realm's, and the
            // open conversation is the open chat's.
            Deed::Close(hamlet) => chat::decree(
                world,
                hamlet,
                format!("/remove-hamlet {hamlet}"),
            ),
            Deed::Unshelve(skill) => {
                chat::command(world, format!("/unshelve {skill}"));
            }
            Deed::NewSession => crate::convo::new_session(world),
            Deed::DefaultCharter => {
                crate::settings::default_charter(world);
            }
            Deed::MoveLodges => {
                crate::settings::workspace::move_lodges(world);
            }
            Deed::Rewind { session, entry, .. } => {
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
            Self::End(_) => "end",
            Self::Merge { .. } => "merge",
            Self::Close(_) => "remove-hamlet",
            Self::Erase(_) => "erase",
            Self::Unshelve(_) => "unshelve",
            Self::Mayor(_) => "mayor",
            Self::NewSession
            | Self::DefaultCharter
            | Self::MoveLodges
            | Self::Rewind { .. } => return true,
        };
        Deeds::is_allowed(world, command)
    }

    fn question(&self, world: &World) -> String {
        let name = |agent: &Entity| agents::shown_of(world, *agent);
        match self {
            Deed::End(agent) => format!("End {}?", name(agent)),
            Deed::Merge { absorbed, kept } => format!(
                "Merge {} into {}?",
                name(absorbed),
                name(kept)
            ),
            Deed::Close(hamlet) => {
                format!("Close down {}?", hamlet)
            }
            Deed::Erase(agent) => {
                format!("Delete {} for good?", name(agent))
            }
            Deed::Unshelve(skill) => {
                format!("Take {} out of the library?", skill)
            }
            Deed::NewSession => "Start a new session?".to_string(),
            Deed::Mayor(agent) => {
                format!("Make {} mayor?", name(agent))
            }
            Deed::Rewind { .. } => "Rewind to here?".to_string(),
            Deed::DefaultCharter => {
                "Go back to the default charter?".to_string()
            }
            Deed::MoveLodges => "Move every lodge?".to_string(),
        }
    }

    fn outcome(&self, world: &World) -> String {
        let name = |agent: &Entity| agents::shown_of(world, *agent);
        match self {
            Deed::End(agent) => format!(
                "The hire {} goes to the archive, with its memory, \
                 skills and log. It can be restored.",
                name(agent)
            ),
            Deed::Merge { absorbed, kept } => format!(
                "{} takes the memory, skills, files and roads of \
                 {}, whose lodge is archived. It can be split out \
                 again.",
                name(kept),
                name(absorbed)
            ),
            Deed::Close(hamlet) => format!(
                "Everyone in {hamlet} stops and every lodge is \
                 archived. You can restore it from the list of \
                 hamlets."
            ),
            Deed::Erase(agent) => {
                let name = name(agent);
                format!(
                    "The archived lodge of {name} is deleted from \
                     disk. This cannot be undone."
                )
            }
            Deed::NewSession => "This conversation goes to \
                                 SESSIONS, where it can be read and \
                                 resumed. The agent keeps its \
                                 memory and skills."
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
                 and the message goes back in the message box. \
                 Changed files, commands that ran, letters and \
                 spending are not undone.",
                name(&world.resource::<Convo>().agent)
            ),
            Deed::DefaultCharter => "Your own charter is deleted \
                                     from disk. This cannot be \
                                     undone."
                .to_string(),
            Deed::MoveLodges => "Each agent's lodge is copied to \
                                 the saved workspace, and the \
                                 lodges already there are replaced. \
                                 This cannot be undone."
                .to_string(),
        }
    }

    fn verb(&self) -> &'static str {
        match self {
            Deed::End(_) => "End",
            Deed::Merge { .. } => "Merge",
            Deed::Close(_) => "Close down",
            Deed::Erase(_) => "Delete",
            Deed::Unshelve(_) => "Take out",
            Deed::NewSession => "New session",
            Deed::Mayor(_) => "Make mayor",
            Deed::Rewind { .. } => "Rewind",
            Deed::DefaultCharter => "Use the default",
            Deed::MoveLodges => "Replace them",
        }
    }

    /// The agents it ends or changes that are at work.
    fn working(&self, world: &mut World) -> Vec<String> {
        let busy = |world: &World, agents: &[Entity]| {
            agents
                .iter()
                .filter(|agent| world.get::<Busy>(**agent).is_some())
                .map(|agent| agents::shown_of(world, *agent))
                .collect()
        };
        match self {
            Deed::End(agent) => busy(world, &[*agent]),
            Deed::Merge { absorbed, kept } => {
                busy(world, &[*absorbed, *kept])
            }
            Deed::Erase(_)
            | Deed::Unshelve(_)
            | Deed::NewSession
            | Deed::Mayor(_)
            | Deed::Rewind { .. }
            | Deed::DefaultCharter
            | Deed::MoveLodges => Vec::new(),
            Deed::Close(hamlet) => {
                let mut living =
                    world.query_filtered::<(Entity, &LivesIn), (
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
                    .map(|(agent, _)| agents::shown_of(world, agent))
                    .collect()
            }
        }
    }
}

/// The question before `deed`, against `anchor`, with a warning for
/// each agent that is at work.
pub(crate) fn question(deed: Deed, anchor: Rect) -> impl View {
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
        // A merge that completes is purple; the rest end something.
        let (confirm, variant): (fn(&mut Cx), Variant) = match &deed {
            Deed::Merge { .. } => (merge_button, Variant::Subtle),
            _ => (danger_button, Variant::Danger),
        };
        let (gap, section, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.gap,
                theme.space.section,
                theme.space.panel,
                theme.text.heading,
            )
        };
        let body = column((
            label(deed.question(cx.world)).size(heading).bold(true),
            label(deed.outcome(cx.world)).wrap(true),
            warning,
            row((
                lettered(deed.verb())
                    .rules(confirm)
                    .tagged(variant)
                    .on_activate(move |world| {
                        close(world);
                        done.run(world);
                    }),
                lettered("Cancel").rules(button).on_activate(close),
            ))
            .gap(gap),
        ))
        .gap(section)
        .width(px(300.0));
        // The chief's own message, at the chat's right, has its
        // question hang from its right edge, touching it.
        let placements = match deed {
            Deed::Rewind { .. } => {
                [PopoverSide::Bottom, PopoverSide::Top]
                    .into_iter()
                    .map(|side| PopoverPlacement {
                        side,
                        align: PopoverAlign::End,
                        gap: 0.0,
                    })
                    .collect()
            }
            _ => corners(0.0),
        };
        let popup = popup(anchor, (body,))
            .placements(placements)
            .on_dismiss(|world, _| close(world))
            .padding(bevy::ui::UiRect::all(px(padding)));
        // Ringed as the message it asks about is.
        match deed {
            Deed::Rewind { .. } => {
                let ring = cx.theme().palette.accent;
                cx.build(popup.border_color(ring))
            }
            _ => cx.build(popup),
        }
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
        state.deeds = hamlet_testing::realm::deeds(&["end"]);
        state.hamlets.push(hamlet_testing::realm::hamlet("mill", 4));
        state
            .agents
            .push(hamlet_testing::realm::agent(4, "mill", 0));
        state
            .agents
            .push(hamlet_testing::realm::agent(5, "mill", 1));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        // The chief's seat opened the Hand's chat.
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        let carder =
            agents::named(app.world_mut(), "mill#1").unwrap();
        Deed::End(carder).run(app.world_mut());
        app.world_mut().flush();
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one command: {sent:?}");
        };
        assert_eq!(say.to, protocol::AgentId(4));
        assert_eq!(say.text, "/end mill#1 ended by the chief");
    }
}
