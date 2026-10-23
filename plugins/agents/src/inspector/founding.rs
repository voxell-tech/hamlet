//! The form a hamlet is founded with, on a card over the dock.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Size, Space, Tone, UiExt,
    ViewExt, button, column, description, field, label, resource,
    row, show_overlay,
};
use serde::{Deserialize, Serialize};

use crate::shared::AgentsWorldExt;

/// The name of the form's overlay, and of the command that shows it.
pub(crate) const FOUND: &str = "found";

/// What is typed in the form is kept across a reload.
pub(crate) fn plugin(app: &mut App) {
    app.init_saved_resource::<Founding>();
}

/// The name of a hamlet being founded, the name of its mayor, and its
/// purpose.
#[derive(
    Resource, Default, Clone, PartialEq, Serialize, Deserialize,
)]
pub(crate) struct Founding {
    pub(crate) name: String,
    pub(crate) mayor: String,
    pub(crate) purpose: String,
}

impl Founding {
    /// The chief's command that founds it. None until all three are
    /// given.
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

/// Shows the form, with what was typed in it before.
pub(crate) fn open() {
    show_overlay(FOUND, true);
}

/// One line of the form: its question, and its field.
fn asks(
    what: &'static str,
    read: fn(&Founding) -> &String,
    write: fn(&mut Founding, String),
) -> AnyView {
    column((
        label(what).size(Size::Small).tone(Tone::Dim),
        field(resource::<Founding, _>(move |founding| {
            read(founding).clone()
        }))
        .on_commit(move |world, text| {
            write(&mut world.resource_mut::<Founding>(), text)
        }),
    ))
    .space(Space::None)
    .boxed()
}

/// The form.
pub(crate) fn founding() -> AnyView {
    column((
        label("FOUND A HAMLET").bold(true).face(Face::Display),
        description(
            "It is built on the next free ground and gets its own \
             mayor.",
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
        description(resource::<Founding, _>(
            |founding| match founding.command() {
                Some(_) => "Names are kept as lowercase letters and \
                            hyphens."
                    .to_string(),
                None => "All three are needed.".to_string(),
            },
        )),
        row((
            button("FOUND").style(ButtonStyle::Primary).on_click(
                |world| {
                    let Some(command) =
                        world.resource::<Founding>().command()
                    else {
                        return;
                    };
                    show_overlay(FOUND, false);
                    world.insert_resource(Founding::default());
                    world.tell(command);
                },
            ),
            // The words typed stay for the next time it is opened.
            button("CANCEL")
                .style(ButtonStyle::Quiet)
                .on_click(|_| show_overlay(FOUND, false)),
        ))
        .space(Space::Half),
    ))
    .space(Space::Half)
    .boxed()
}
