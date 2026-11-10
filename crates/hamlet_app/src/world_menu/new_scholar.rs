//! The form a scholar is made in the city with: its name, and folded
//! under it who it is, which may be left out.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
};
use bevy_fynix::{ScopedExt, ViewExt};
use hamlet_client::realm::Deeds;
use hamlet_ui::styles::{Looks, button, description, lettered};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::{AnyView, View, built};

use super::{asks, asks_first, close, optional};
use crate::chat;

/// The name of a scholar being made, and the character it starts
/// from.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Making {
    name: String,
    character: String,
    /// Whether what may be left out is shown.
    more: bool,
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

/// Makes the scholar the form holds, once it is named.
pub(super) fn submit(world: &mut World) {
    if !Deeds::is_allowed(world, "scholar") {
        return;
    }
    let Some(command) = world.resource::<Making>().command() else {
        return;
    };
    close(world);
    *world.resource_mut::<Making>() = Making::default();
    chat::command(world, command);
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
            label("New scholar").size(heading).bold(true),
            description("It gets a house in the city at once."),
            asks_first::<Making>(
                "Its name",
                |m| &m.name,
                |m, text| m.name = text,
            ),
            optional::<Making>(
                |m| m.more,
                |m| m.more = !m.more,
                || -> AnyView {
                    column((
                        asks::<Making>(
                            "Who it is",
                            |m| &m.character,
                            |m, text| m.character = text,
                        ),
                        description(
                            "Left empty, it starts from the realm's \
                             default character.",
                        ),
                    ))
                    .gap(8.0)
                    .boxed()
                },
            ),
            row((
                lettered("Make")
                    .variant(Variant::Primary)
                    .on_activate(submit),
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
