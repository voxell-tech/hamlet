//! The actions asked about before they are done.

use bevy::ecs::entity::Entity;
use bevy::ecs::query::{With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::ui::{AlignItems, px};
use bevy::ui_widgets::popover::{
    PopoverAlign, PopoverPlacement, PopoverSide,
};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, corners, frame, label, popup,
    row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_client::realm::{
    self as agents, Actions, Busy, HamletAgent, HamletId, Left,
    LivesIn,
};
use hamlet_realm::orders;
use hamlet_ui::styles::{Looks, button, lettered};
use hamlet_ui::theme::StatusKind;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::{View, built};

use super::close;
use crate::chat;

/// An action asked about before it is done.
#[derive(Clone, PartialEq)]
pub(crate) enum Action {
    End(Entity),
    /// Ends this agent after one more dream.
    EndWithDream(Entity),
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
    /// Makes this agent the lead of its hamlet.
    Lead(Entity),
    /// Rewinds the open conversation to before message `entry`, its
    /// words going back to the field of the Hub it was asked in.
    Rewind {
        hub: Entity,
        chat: Entity,
        entry: String,
        /// The time of the message, in milliseconds since 1970.
        at: u64,
    },
    /// Ends the realm's own charter, so the built-in one stands.
    DefaultCharter,
    /// Copies every worktree to the saved workspace.
    MoveWorktrees,
}

/// The reason typed under a question, and whether its field is shown.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Reason {
    text: String,
    more: bool,
}

/// Closes the question and does `action`, as its first button does.
pub(super) fn answer(world: &mut World, action: &Action) {
    close(world);
    action.run(world);
}

impl Action {
    /// Sends the chief's command that does it, with any reason typed.
    pub(crate) fn run(&self, world: &mut World) {
        if !self.is_available(world) {
            return;
        }
        let reason = world
            .get_resource_mut::<Reason>()
            .map(|mut reason| std::mem::take(&mut *reason).text)
            .unwrap_or_default();
        let reason = reason.trim();
        let name = |agent: Entity| agents::name_of(world, agent);
        // A command with the reason after it, when one was given.
        let with = |command: String| {
            format!("{command} {reason}").trim_end().to_string()
        };
        match self {
            Action::End(agent) => {
                let text = with(format!("/end {}", name(*agent)));
                orders::decree(world, *agent, text);
            }
            Action::EndWithDream(agent) => {
                let text =
                    with(format!("/end {} dream", name(*agent)));
                orders::decree(world, *agent, text);
            }
            Action::Merge { absorbed, kept } => {
                let text = with(format!(
                    "/merge {} into {}",
                    name(*absorbed),
                    name(*kept)
                ));
                orders::decree(world, *kept, text);
            }
            Action::Erase(agent) => {
                let text = format!("/erase {}", name(*agent));
                crate::hub::erasure::expect_agent(world, *agent);
                orders::decree(world, *agent, text);
            }
            Action::Lead(agent) => {
                let text = with(format!("/lead {}", name(*agent)));
                orders::decree(world, *agent, text);
            }
            // The hamlets and the library are the realm's.
            Action::Close(hamlet) => {
                let text = with(format!("/remove-hamlet {hamlet}"));
                chat::decree(world, hamlet, text);
            }
            Action::Unshelve(skill) => {
                chat::command(world, format!("/unshelve {skill}"));
            }
            Action::DefaultCharter => {
                crate::settings::default_charter(world);
            }
            Action::MoveWorktrees => {
                crate::settings::workspace::move_worktrees(world);
            }
            Action::Rewind {
                hub, chat, entry, ..
            } => {
                chat::rewind_to(world, *hub, *chat, entry);
            }
        }
    }

    /// Whether the record keeps a reason for it.
    fn takes_reason(&self) -> bool {
        matches!(
            self,
            Action::End(_)
                | Action::EndWithDream(_)
                | Action::Merge { .. }
                | Action::Lead(_)
                | Action::Close(_)
        )
    }

    fn is_available(&self, world: &World) -> bool {
        let command = match self {
            Self::End(_) | Self::EndWithDream(_) => "end",
            Self::Merge { .. } => "merge",
            Self::Close(_) => "remove-hamlet",
            Self::Erase(_) => "erase",
            Self::Unshelve(_) => "unshelve",
            Self::Lead(_) => "lead",
            Self::DefaultCharter
            | Self::MoveWorktrees
            | Self::Rewind { .. } => return true,
        };
        Actions::is_allowed(world, command)
    }

    fn question(&self, world: &World) -> String {
        let name = |agent: &Entity| agents::shown_of(world, *agent);
        match self {
            Action::End(agent) | Action::EndWithDream(agent) => {
                format!("End {}?", name(agent))
            }
            Action::Merge { absorbed, kept } => format!(
                "Merge {} into {}?",
                name(absorbed),
                name(kept)
            ),
            Action::Close(hamlet) => {
                format!("Close down {}?", hamlet)
            }
            Action::Erase(agent) => {
                format!("Delete {} for good?", name(agent))
            }
            Action::Unshelve(skill) => {
                format!("Take {} out of the library?", skill)
            }
            Action::Lead(agent) => {
                format!("Make {} lead?", name(agent))
            }
            Action::Rewind { .. } => "Rewind to here?".to_string(),
            Action::DefaultCharter => {
                "Go back to the default charter?".to_string()
            }
            Action::MoveWorktrees => {
                "Move every worktree?".to_string()
            }
        }
    }

    fn outcome(&self, world: &World) -> String {
        let name = |agent: &Entity| agents::shown_of(world, *agent);
        match self {
            Action::End(agent) | Action::EndWithDream(agent) => {
                format!(
                    "The agent {} goes to the archive, with its \
                     memory, skills and log. It can be restored. \
                     With a dream, it first brings what it learned \
                     home to its profile, for the profile's later \
                     agents.",
                    name(agent)
                )
            }
            Action::Merge { absorbed, kept } => format!(
                "{} takes the memory, skills, files and channels of \
                 {}, whose worktree is archived. It can be split \
                 out again.",
                name(kept),
                name(absorbed)
            ),
            Action::Close(hamlet) => format!(
                "Everyone in {hamlet} stops and every worktree is \
                 archived. You can restore it from the list of \
                 hamlets."
            ),
            Action::Erase(agent) => {
                let name = name(agent);
                format!(
                    "The archived worktree of {name} is deleted \
                     from disk. This cannot be undone."
                )
            }
            Action::Unshelve(skill) => format!(
                "{skill} is deleted from the library for every \
                 hamlet. This cannot be undone."
            ),
            Action::Lead(agent) => format!(
                "{} runs the hamlet from now on, and today's lead \
                 becomes one of its agents.",
                name(agent)
            ),
            Action::Rewind { chat, .. } => format!(
                "{} forgets this message and everything after it, \
                 and the message goes back in the message box. \
                 Changed files, commands that ran, letters and \
                 spending are not undone.",
                name(
                    &world
                        .get::<hamlet_client::chats::TalksTo>(*chat)
                        .map_or(Entity::PLACEHOLDER, |to| to.0)
                )
            ),
            Action::DefaultCharter => "Your own charter is deleted \
                                       from disk. This cannot be \
                                       undone."
                .to_string(),
            Action::MoveWorktrees => {
                "Each agent's worktree is copied to the saved \
                 workspace, and the worktrees already there are \
                 replaced. This cannot be undone."
                    .to_string()
            }
        }
    }

    /// The other action its question offers beside it.
    fn alternative(&self) -> Option<Action> {
        match self {
            Action::End(agent) => Some(Action::EndWithDream(*agent)),
            _ => None,
        }
    }

    fn verb(&self) -> &'static str {
        match self {
            Action::End(_) => "End",
            Action::EndWithDream(_) => "End with Dream",
            Action::Merge { .. } => "Merge",
            Action::Close(_) => "Close Down",
            Action::Erase(_) => "Delete",
            Action::Unshelve(_) => "Take Out",
            Action::Lead(_) => "Make Lead",
            Action::Rewind { .. } => "Rewind",
            Action::DefaultCharter => "Use the Default",
            Action::MoveWorktrees => "Replace Them",
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
            Action::End(agent) | Action::EndWithDream(agent) => {
                busy(world, &[*agent])
            }
            Action::Merge { absorbed, kept } => {
                busy(world, &[*absorbed, *kept])
            }
            Action::Erase(_)
            | Action::Unshelve(_)
            | Action::Lead(_)
            | Action::Rewind { .. }
            | Action::DefaultCharter
            | Action::MoveWorktrees => Vec::new(),
            Action::Close(hamlet) => {
                let mut living =
                    world.query_filtered::<(Entity, &LivesIn), (
                        With<HamletAgent>,
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

/// The question before `action`, warning of each agent at work.
pub(crate) fn question(action: Action, anchor: Rect) -> impl View {
    built(move |cx| {
        if !action.is_available(cx.world) {
            return cx.build(frame());
        }
        let working = action.working(cx.world);
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
        let done = action.clone();
        let alternative = action.alternative();
        // A merge that completes is purple; the rest end something.
        let variant = match &action {
            Action::Merge { .. } => {
                Variant::Tinted(StatusKind::Merged)
            }
            _ => Variant::Danger,
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
        let reason = if action.takes_reason() {
            super::optional::<Reason>(
                |r| r.more,
                |r| r.more = !r.more,
                || {
                    hamlet_ui::styles::close((
                        hamlet_ui::styles::section("Why"),
                        super::field::<Reason>(
                            |r| &r.text,
                            |r, text| r.text = text,
                        )
                        .tagged(super::TakesFocus),
                    ))
                    .boxed()
                },
            )
            .boxed()
        } else {
            frame().boxed()
        };
        let body = column((
            label(action.question(cx.world)).size(heading).bold(true),
            label(action.outcome(cx.world)).wrap(true),
            warning,
            reason,
            row((
                lettered(action.verb())
                    .variant(variant)
                    .on_activate(move |world| answer(world, &done)),
                match alternative {
                    Some(other) => lettered(other.verb())
                        .variant(variant)
                        .on_activate(move |world| {
                            answer(world, &other);
                        })
                        .boxed(),
                    None => frame().boxed(),
                },
                lettered("Cancel").rules(button).on_activate(close),
                super::send_keys(),
            ))
            .gap(gap)
            .align(AlignItems::Center),
        ))
        .gap(section)
        .width(px(300.0));
        // The chief's own message hangs its question from its right
        // edge.
        let placements = match action {
            Action::Rewind { .. } => {
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
        match action {
            Action::Rewind { .. } => {
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
    fn an_action_goes_to_the_lead_of_the_agents_received_hamlet() {
        let mut app = crate::testing::panel_app();
        app.add_plugins(chat::plugin);
        let mut state = hamlet_testing::realm::state();
        state.actions = hamlet_testing::realm::actions(&["end"]);
        state.hamlets.push(hamlet_testing::realm::hamlet("mill", 4));
        state
            .agents
            .push(hamlet_testing::realm::agent(4, "mill", true));
        state
            .agents
            .push(hamlet_testing::realm::agent(5, "mill", false));
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
            agents::named(app.world_mut(), "agent#5").unwrap();
        Action::End(carder).run(app.world_mut());
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
        assert_eq!(say.text, "/end agent#5");
    }
}
