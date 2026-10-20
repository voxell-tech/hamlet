//! A sample plugin whose panel is retained and written with fynix: a
//! count and a button that adds one, a button that takes the primary
//! style under the pointer, two buttons that each fade a label, one
//! moved on by the plugin and one by the engine, and rows enough for
//! a panel of about fifty nodes.

use hamlet_plugin::ui::{
    AnyView, Button, ButtonStyle, Cx, FrameProps, Hovered, Motion,
    ScopedExt, Space, StateExt, Tone, UiExt, ViewExt, World, button,
    column, heading, label, resource, row,
};
use hamlet_plugin::{Entry, Plugin};

/// Rows under the buttons, three nodes each.
const ROWS: u32 = 12;

#[derive(Default)]
struct Counter;

struct Count(u32);

/// Whether the labels are faded: the one the plugin moves, and the
/// one the engine does.
#[derive(Default)]
struct Faded {
    plugin: bool,
    engine: bool,
}

impl Plugin for Counter {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("counter", "Counter");
    }

    fn mount(&mut self, _: &str, world: &mut World) -> AnyView {
        if world.get::<Count>().is_none() {
            world.insert(Count(0));
            world.insert(Faded::default());
        }
        let opacity = |faded: bool| if faded { 0.3 } else { 1.0 };
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
                button("Hover me").when::<Hovered, _>(
                    |cx: &mut Cx| {
                        cx.set::<Button>(|button, _| {
                            button.style(ButtonStyle::Primary)
                        });
                    },
                ),
            )),
            row((
                button("Fade in the plugin").on_click(|world| {
                    let faded = world.resource_mut::<Faded>();
                    faded.plugin = !faded.plugin;
                }),
                button("Fade in the engine").on_click(|world| {
                    let faded = world.resource_mut::<Faded>();
                    faded.engine = !faded.engine;
                }),
            )),
            label("Faded by the plugin")
                .opacity(resource::<Faded, _>(move |f| {
                    opacity(f.plugin)
                }))
                .transition(Motion::Interact),
            label("Faded by the engine")
                .opacity(resource::<Faded, _>(move |f| {
                    opacity(f.engine)
                }))
                .engine_transition(Motion::Interact),
            column(rows).space(Space::Half),
        ))
        .boxed()
    }
}

hamlet_plugin::export!(Counter);
