//! The form a hamlet is founded with.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, frame, label, popup,
    row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{Deeds, Scholar, scholar_address};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    ICON, Looks, button, description, lettered, section,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{View, built};

use super::{asks, close};
use crate::chat;

/// The name of a hamlet being founded, the scholar that is its mayor,
/// and its purpose.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Founding {
    name: String,
    /// A new scholar named Mayor while None.
    mayor: Option<protocol::ScholarId>,
    purpose: String,
}

impl Founding {
    /// The chief's command that founds it. None until its name and
    /// purpose are given.
    fn command(&self) -> Option<String> {
        // A hamlet's name is lowercase letters and hyphens.
        let name = self
            .name
            .trim()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-");
        let purpose = self.purpose.trim();
        (!name.is_empty() && !purpose.is_empty()).then(|| match self
            .mayor
        {
            Some(mayor) => format!(
                "/found {name} {} {purpose}",
                scholar_address(mayor)
            ),
            None => format!("/found {name} {purpose}"),
        })
    }
}

/// The scholars that can be the mayor: living ones, by name.
fn candidates(world: &World) -> Vec<(protocol::ScholarId, String)> {
    let Some(mut scholars) = world.try_query::<&Scholar>() else {
        return Vec::new();
    };
    let mut found = scholars
        .iter(world)
        .filter(|scholar| scholar.0.left.is_none())
        .map(|scholar| (scholar.0.id, scholar.0.name.clone()))
        .collect::<Vec<_>>();
    found.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.0.cmp(&b.0.0)));
    found
}

/// Who is to be the mayor: a new scholar, or one of the living.
fn mayor() -> impl View {
    keyed(
        derived(|world: &World| {
            (candidates(world), world.resource::<Founding>().mayor)
        })
        .when(every_frame()),
        |(found, chosen)| {
            let shown = std::iter::once(
                "A new scholar named Mayor".to_string(),
            )
            .chain(found.iter().map(|(_, name)| name.clone()))
            .collect::<Vec<_>>();
            let at = chosen
                .and_then(|chosen| {
                    found.iter().position(|(id, _)| *id == chosen)
                })
                .map_or(0, |at| at + 1);
            let found = found.clone();
            hamlet_ui::styles::close((
                section("Its mayor"),
                dropdown(
                    shown,
                    at,
                    Glyph::Chevron.image(ICON),
                    move |world, picked| {
                        world.resource_mut::<Founding>().mayor =
                            picked
                                .checked_sub(1)
                                .and_then(|at| found.get(at))
                                .map(|(id, _)| *id);
                    },
                ),
            ))
            .boxed()
        },
    )
    .within(column(()))
}

/// The form a hamlet is founded with.
pub(super) fn founding(at: Vec2) -> impl View {
    built(move |cx| {
        if !Deeds::is_allowed(cx.world, "found") {
            return cx.build(frame());
        }
        let (gap, between, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.gap,
                theme.space.section,
                theme.space.overlay_padding,
                theme.text.heading,
            )
        };
        let body = column((
            label("Found a hamlet").size(heading).bold(true),
            description(
                "It is built on the next free ground and gets its \
                 own mayor.",
            ),
            asks::<Founding>(
                "Its name",
                |f| &f.name,
                |f, text| f.name = text,
            ),
            mayor(),
            asks::<Founding>(
                "What it is for",
                |f| &f.purpose,
                |f, text| f.purpose = text,
            ),
            description(resource::<Founding, _>(|f| {
                match f.command() {
                    Some(_) => "A name is kept as lowercase letters \
                                and hyphens."
                        .to_string(),
                    None => "Its name and what it is for are needed."
                        .to_string(),
                }
            })),
            row((
                lettered("Found")
                    .variant(Variant::Primary)
                    .on_activate(|world| {
                        if !Deeds::is_allowed(world, "found") {
                            return;
                        }
                        let Some(command) =
                            world.resource::<Founding>().command()
                        else {
                            return;
                        };
                        close(world);
                        *world.resource_mut::<Founding>() =
                            Founding::default();
                        chat::command(world, command);
                    }),
                lettered("Cancel").rules(button).on_activate(close),
            ))
            .gap(gap),
        ))
        .gap(between)
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
    use hamlet_testing::realm;

    use super::*;
    use crate::testing::{panel_app, press};

    #[test]
    fn founding_goes_to_the_hand_even_from_another_chat() {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>();
        let mut state = realm::state();
        state.deeds = realm::deeds(&["found"]);
        state.agents.push(realm::agent(3, "home", 1));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let scout =
            hamlet_client::realm::named(app.world_mut(), "home#1")
                .unwrap();
        crate::convo::talk_to(app.world_mut(), scout);
        app.insert_resource(Founding {
            name: "mill".into(),
            mayor: Some(protocol::ScholarId(2)),
            purpose: "Grinds flour".into(),
        });
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            founding(Vec2::ZERO),
        );
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        press(&mut app, "Found");
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one founding: {sent:?}");
        };
        assert_eq!(say.text, "/found mill scholar#2 Grinds flour");
        assert_eq!(say.to, protocol::AgentId(1));
    }
}
