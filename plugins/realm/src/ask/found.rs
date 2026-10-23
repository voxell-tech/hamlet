//! The form a hamlet is founded with.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Size, Space, Tone, UiExt,
    ViewExt, button, column, description, field, label, resource,
    row,
};

use crate::ask::AskWorldExt;
use crate::realm::HOME;

/// On the card of the form.
#[derive(Component)]
pub struct Forming;

/// The name of a hamlet being founded, the name of its mayor, and its
/// purpose.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct Founding {
    name: String,
    mayor: String,
    purpose: String,
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

/// The form on the card `card`.
pub fn view(card: Entity) -> AnyView {
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
                move |world| {
                    let Some(command) =
                        world.resource::<Founding>().command()
                    else {
                        return;
                    };
                    world.despawn(card);
                    world.insert_resource(Founding::default());
                    world.decree(HOME, command);
                },
            ),
            // The words typed stay for the next time it is opened.
            button("CANCEL").style(ButtonStyle::Quiet).on_click(
                move |world| {
                    world.despawn(card);
                },
            ),
        ))
        .space(Space::Half),
    ))
    .space(Space::Half)
    .boxed()
}
