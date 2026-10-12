//! The form a hamlet is founded with.

use bevy::ecs::resource::Resource;
use bevy::math::{Rect, Vec2};
use bevy::ui::px;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, popup, row, text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, resource};

use super::close;
use crate::feel::Mood;
use crate::looks::{brass, lamp, lettered, note, well};
use crate::platform::DisplayText;
use crate::{Ui, chat};

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
) -> Ui {
    column((
        label(what).size(11.0).tone(Tone::Dim),
        text_field(
            resource::<Founding, _>(move |f| read(f).clone()),
            move |world, text| {
                write(&mut world.resource_mut::<Founding>(), text)
            },
        )
        .rules(well),
    ))
    .gap(3.0)
    .boxed()
}

/// The form a hamlet is founded with.
pub(super) fn founding(at: Vec2) -> Ui {
    let body = column((
        label("FOUND A HAMLET")
            .size(13.0)
            .bold(true)
            .tagged(DisplayText),
        note(
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
        label(resource::<Founding, _>(|f| match f.command() {
            Some(_) => "Names are kept as lowercase letters and \
                        hyphens."
                .to_string(),
            None => "All three are needed.".to_string(),
        }))
        .size(11.0)
        .tone(Tone::Dim),
        row((
            lettered("FOUND")
                .rules(lamp)
                .tagged(Mood::Good)
                .on_activate(|world| {
                    let Some(command) =
                        world.resource::<Founding>().command()
                    else {
                        return;
                    };
                    close(world);
                    *world.resource_mut::<Founding>() =
                        Founding::default();
                    chat::send(world, command);
                }),
            lettered("CANCEL").rules(brass).on_activate(close),
        ))
        .gap(8.0),
    ))
    .gap(8.0)
    .width(px(300.0));
    popup(Rect::from_corners(at, at), (body,))
        .on_dismiss(|world, _| close(world))
        .padding(bevy::ui::UiRect::all(px(14.0)))
        .boxed()
}
