//! The words of a settings page and the controls a limit is set
//! with, as the realm's page and a hamlet's bylaws show them: its
//! name over a field, read from a resource and handing what is typed
//! on.

use hamlet_engine_bevy::prelude::{Resource, World};
use hamlet_engine_bevy::ui::{
    AnyView, Face, FrameProps, Label, Prop, Size, Space, Tone, UiExt,
    ViewExt, column, field, icon, label, resource, row,
};

/// Small words that stand back.
pub fn note(text: impl Into<Prop<World, String>>) -> Label {
    label(text).size(Size::Small).tone(Tone::Dim)
}

/// The name of a setting, over its control.
pub fn name(text: impl Into<Prop<World, String>>) -> Label {
    label(text)
        .size(Size::Small)
        .bold(true)
        .tone(Tone::Faint)
        .face(Face::Display)
}

/// The head of a part of a page: its icon and its name.
pub fn part(which: &str, text: &str) -> AnyView {
    row((icon(which.to_string()).tone(Tone::Faint), name(text)))
        .space(Space::Half)
        .boxed()
}

/// A setting: its name, its control, and words under it if any.
pub fn setting(
    text: &str,
    control: AnyView,
    help: Option<&str>,
) -> AnyView {
    let mut parts = vec![name(text).boxed(), control];
    if let Some(help) = help {
        parts.push(note(help.to_string()).boxed());
    }
    column(parts).space(Space::Half).boxed()
}

/// A whole number from 0 to `most`, named `what`, as `read` takes it
/// from the resource `R`. A number typed is handed to `write`, and
/// anything else, with why, to `refuse`.
pub fn whole<R: Resource>(
    what: &'static str,
    most: u64,
    read: impl Fn(&R) -> Option<u64> + Send + Sync + 'static,
    write: impl Fn(&mut World, u64) + Send + Sync + 'static,
    refuse: impl Fn(&mut World, String) + Send + Sync + 'static,
) -> AnyView {
    let control = field(resource::<R, _>(move |held| {
        read(held).map(|n| n.to_string()).unwrap_or_default()
    }))
    .on_commit(move |world, typed| {
        match typed.trim().parse::<u64>().ok().filter(|n| *n <= most)
        {
            Some(n) => write(world, n),
            None => refuse(
                world,
                format!("{what}: a whole number from 0 to {most}."),
            ),
        }
    })
    .boxed();
    setting(what, control, None)
}
