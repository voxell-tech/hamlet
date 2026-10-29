//! The form a hamlet is founded with.

use bevy::ecs::resource::Resource;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
    text_field,
};
use bevy_fynix::{ScopedExt, resource};
use hamlet_client::realm::{Deeds, HOME};
use hamlet_ui::styles::{
    button, description, lettered, primary_button, section,
    text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{View, built};

use super::close;
use crate::chat;

/// The name of a hamlet being founded, the name of its mayor, and its
/// purpose.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Founding {
    name: String,
    mayor: String,
    purpose: String,
}

impl Founding {
    /// The chief's command that founds it. None until all three
    /// are given.
    fn command(&self) -> Option<String> {
        // A name is lowercase letters and hyphens.
        let named = |text: &str| {
            text.trim()
                .to_lowercase()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join("-")
        };
        let (name, mayor) = (named(&self.name), named(&self.mayor));
        let purpose = self.purpose.trim();
        (!name.is_empty() && !mayor.is_empty() && !purpose.is_empty())
            .then(|| format!("/found {name} {mayor} {purpose}"))
    }
}

/// One line of the founding form: its question, and its field.
fn asks(
    what: &'static str,
    read: fn(&Founding) -> &String,
    write: fn(&mut Founding, String),
) -> impl View {
    hamlet_ui::styles::close((
        section(what),
        text_field(
            resource::<Founding, _>(move |f| read(f).clone()),
            move |world, text| {
                write(&mut world.resource_mut::<Founding>(), text);
            },
        )
        .rules(text_input),
    ))
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
            label("FOUND A HAMLET")
                .size(heading)
                .bold(true)
                .tagged(DisplayText),
            description(
                "It is built on the next free ground and gets its \
                 own mayor.",
            ),
            asks("Its name", |f| &f.name, |f, text| f.name = text),
            asks(
                "Its mayor's name",
                |f| &f.mayor,
                |f, text| f.mayor = text,
            ),
            asks(
                "What it is for",
                |f| &f.purpose,
                |f, text| f.purpose = text,
            ),
            description(resource::<Founding, _>(|f| {
                match f.command() {
                    Some(_) => "Names are kept as lowercase letters \
                                and hyphens."
                        .to_string(),
                    None => "All three are needed.".to_string(),
                }
            })),
            row((
                lettered("FOUND")
                    .rules(primary_button)
                    .tagged(Variant::Primary)
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
                        chat::decree(world, HOME, command);
                    }),
                lettered("CANCEL").rules(button).on_activate(close),
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
    fn founding_goes_to_the_first_mayor_even_from_another_chat() {
        let mut app = panel_app();
        app.add_plugins(super::super::plugin)
            .init_resource::<hamlet_realm::pick::Asked>()
            .init_resource::<hamlet_realm::pick::AskedGround>();
        let mut state = realm::state();
        state.deeds = realm::deeds(&["found"]);
        state.agents.push(realm::agent("home", 1, "scout"));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let scout =
            hamlet_client::realm::named(app.world_mut(), "scout")
                .unwrap();
        crate::convo::talk_to(app.world_mut(), scout);
        app.insert_resource(Founding {
            name: "mill".into(),
            mayor: "miller".into(),
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
        press(&mut app, "FOUND");
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [protocol::ClientMessage::Say(say)] = &sent[..] else {
            panic!("one founding: {sent:?}");
        };
        assert_eq!(say.text, "/found mill miller Grinds flour");
        assert_eq!(
            say.to,
            protocol::Address {
                hamlet: "home".into(),
                agent: "mayor".into()
            }
        );
    }
}
