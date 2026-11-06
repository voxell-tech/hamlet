//! The form a scholar is made in the city with.

use bevy::ecs::resource::Resource;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
    text_field,
};
use bevy_fynix::{ScopedExt, resource};
use hamlet_client::realm::Deeds;
use hamlet_ui::styles::{
    button, description, lettered, primary_button, section,
    text_input,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::platform::DisplayText;
use hamlet_ui::{View, built};

use super::close;
use crate::chat;

/// The name of a scholar being made, and the character it starts
/// from.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Making {
    name: String,
    character: String,
}

impl Making {
    /// The chief's command that makes it. None until it is named.
    fn command(&self) -> Option<String> {
        // A name is free text, so a character after it is set off
        // by a colon.
        let name = self.name.trim();
        let character = self.character.trim();
        (!name.is_empty()).then(|| match character.is_empty() {
            true => format!("/scholar new {name}"),
            false => format!("/scholar new {name}: {character}"),
        })
    }
}

/// One line of the form: its question, and its field.
fn asks(
    what: &'static str,
    read: fn(&Making) -> &String,
    write: fn(&mut Making, String),
) -> impl View {
    hamlet_ui::styles::close((
        section(what),
        text_field(
            resource::<Making, _>(move |m| read(m).clone()),
            move |world, text| {
                write(&mut world.resource_mut::<Making>(), text);
            },
        )
        .rules(text_input),
    ))
}

/// The form a scholar is made with.
pub(super) fn new_scholar(at: Vec2) -> impl View {
    built(move |cx| {
        if !Deeds::is_allowed(cx.world, "scholar") {
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
            label("NEW SCHOLAR")
                .size(heading)
                .bold(true)
                .tagged(DisplayText),
            description("It gets a house in the city at once."),
            asks("Its name", |m| &m.name, |m, text| m.name = text),
            asks(
                "Who it is",
                |m| &m.character,
                |m, text| m.character = text,
            ),
            description(
                "Left empty, it starts from the realm's default \
                 character.",
            ),
            row((
                lettered("MAKE")
                    .rules(primary_button)
                    .tagged(Variant::Primary)
                    .on_activate(|world| {
                        if !Deeds::is_allowed(world, "scholar") {
                            return;
                        }
                        let Some(command) =
                            world.resource::<Making>().command()
                        else {
                            return;
                        };
                        close(world);
                        *world.resource_mut::<Making>() =
                            Making::default();
                        chat::command(world, command);
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
