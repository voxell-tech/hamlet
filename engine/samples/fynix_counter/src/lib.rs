//! A sample plugin whose panel is retained and written with fynix: a
//! count and a button that adds one, a button that takes the primary
//! style under the pointer, a button that fades and dims a label, and
//! rows enough for a panel of about fifty nodes.

use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    Button, ButtonStyle, Cx, FrameProps, Hovered, Motion, ScopedExt,
    Space, StateExt, Tone, UiExt, button, column, heading, label,
    resource, row,
};

/// Rows under the buttons, three nodes each.
const ROWS: u32 = 12;

#[derive(Default)]
struct Counter;

impl Plugin for Counter {
    fn build(&self, app: &mut App) {
        app.init_resource::<Count>()
            .init_resource::<Faded>()
            .add_panel("counter", "Counter", counter);
    }
}

hamlet_engine_bevy::export!(Counter);

#[derive(Resource, Default)]
struct Count(u32);

/// Whether the label is faded.
#[derive(Resource, Default)]
struct Faded(bool);

fn counter() -> AnyView {
    let rows = (0..ROWS)
        .map(|at| {
            row((
                label(format!("Row {at}")),
                label("a quiet note").tone(Tone::Dim),
            ))
            .boxed()
        })
        .collect::<Vec<_>>();
    column((
        heading("Counter"),
        label(resource::<Count, _>(|count| {
            format!("Count: {}", count.0)
        })),
        row((
            button("Add one").on_click(|world| {
                world.resource_mut::<Count>().0 += 1;
            }),
            button("Hover me").when::<Hovered, _>(|cx: &mut Cx| {
                cx.set::<Button>(|button, _| {
                    button.style(ButtonStyle::Primary)
                });
            }),
        )),
        button("Fade").on_click(|world| {
            let mut faded = world.resource_mut::<Faded>();
            faded.0 = !faded.0;
        }),
        label("Faded by the engine")
            .opacity(resource::<Faded, _>(|faded| {
                if faded.0 { 0.3 } else { 1.0 }
            }))
            .tone(resource::<Faded, _>(|faded| {
                if faded.0 { Tone::Dim } else { Tone::Body }
            }))
            .transition(Motion::Interact),
        column(rows).space(Space::Half),
    ))
    .boxed()
}
