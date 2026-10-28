//! The Sight page: how much the world draws, the hour its sky shows
//! and the fog the sun throws shafts through.

use bevy::app::App;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, label, row, slider,
};
use bevy_fynix::{ModifierExt, ViewExt, keyed, resource};
use hamlet_realm::Sky;
use hamlet_realm::sky::{FogSettings, Quality};

use super::{field, told};
use crate::dial::dial;
use crate::icons::Glyph;
use crate::looks::{action, close, description, parts, section};
use crate::switch::labelled_switch;
use crate::world_view::TimeOfDay;
use crate::{View, built, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<FogOpen>();
}

/// True while the fog's settings are shown.
#[derive(Resource, Default)]
struct FogOpen(bool);

/// The choice of the world's visual quality.
fn quality() -> impl View {
    built(|cx| {
        cx.build(
            dropdown(
                Quality::ALL.map(Quality::name),
                resource::<Quality, _>(|q| {
                    Quality::ALL
                        .iter()
                        .position(|a| a == q)
                        .unwrap_or_default()
                }),
                Glyph::Chevron.image(crate::looks::ICON),
                |world, at| {
                    *world.resource_mut::<Quality>() =
                        Quality::ALL[at];
                },
            )
            .width(percent(100.0))
            .max_width(percent(100.0)),
        )
    })
}

/// The dial of the sky's hour, and whether it follows the clock.
fn hour() -> impl View {
    with_cx(|cx| {
        row((
            dial(
                resource::<Sky, _>(|sky| sky.hour),
                |world, hour| {
                    *world.resource_mut::<TimeOfDay>() =
                        TimeOfDay::At(hour);
                },
            ),
            column((
                labelled_switch(
                    "Follow the clock",
                    resource::<TimeOfDay, _>(|t| {
                        *t == TimeOfDay::Clock
                    }),
                    |world, on| {
                        let to = if on {
                            TimeOfDay::Clock
                        } else {
                            TimeOfDay::At(
                                world.resource::<Sky>().hour,
                            )
                        };
                        *world.resource_mut::<TimeOfDay>() = to;
                    },
                ),
                description(
                    "Drag the sun around the ring to hold the sky \
                     at an hour: noon at the top, midnight at the \
                     foot.",
                ),
            ))
            .gap(cx.theme().space.gap / 2.0)
            .align(AlignItems::FlexStart)
            .grow(1.0)
            .min_width(px(0.0)),
        ))
        .gap(cx.theme().space.section)
        .align(AlignItems::Center)
    })
}

/// One of the fog's numbers: its name, a slider from `least` to
/// `most` and the number itself.
fn amount(
    name: &'static str,
    at: fn(&FogSettings) -> f32,
    set: fn(&mut FogSettings, f32),
    (least, most): (f32, f32),
) -> impl View {
    with_cx(move |cx| {
        row((
            label(name).wide(px(96.0)),
            slider(resource::<FogSettings, _>(at))
                .range(least, most)
                .on_change(move |world, to| {
                    set(&mut world.resource_mut::<FogSettings>(), to);
                })
                .grow(1.0),
            label(resource::<FogSettings, _>(move |fog| {
                format!("{:.3}", at(fog))
            }))
            .wide(px(48.0)),
        ))
        .gap(cx.theme().space.gap)
        .width(percent(100.0))
        .align(AlignItems::Center)
    })
}

/// The fog's switch, its numbers and the way back to the designed
/// fog.
fn fog() -> impl View {
    close((
        labelled_switch(
            "Fog",
            resource::<FogSettings, _>(|fog| fog.on),
            |world, on| world.resource_mut::<FogSettings>().on = on,
        ),
        amount(
            "Density",
            |f| f.density,
            |f, v| f.density = v,
            (0.0, 0.03),
        ),
        amount(
            "Height",
            |f| f.height,
            |f, v| f.height = v,
            (2.0, 200.0),
        ),
        amount(
            "Light",
            |f| f.strength,
            |f, v| f.strength = v,
            (0.0, 40.0),
        ),
        amount(
            "Light at noon",
            |f| f.high_sun,
            |f, v| f.high_sun = v,
            (0.0, 1.0),
        ),
        amount("Glow", |f| f.glow, |f, v| f.glow = v, (0.0, 0.95)),
        amount(
            "Scattering",
            |f| f.scattering,
            |f, v| f.scattering = v,
            (0.0, 1.0),
        ),
        amount(
            "Absorption",
            |f| f.absorption,
            |f, v| f.absorption = v,
            (0.0, 0.5),
        ),
        amount(
            "Ambient",
            |f| f.ambient,
            |f, v| f.ambient = v,
            (0.0, 1.0),
        ),
        action(Glyph::Restore, "RESET FOG", |world: &mut World| {
            world.insert_resource(FogSettings::default());
        }),
    ))
}

/// The fog's name, which shows or hides its settings.
fn fog_head() -> impl View {
    with_cx(|cx| {
        row((section(resource::<FogOpen, _>(|open| {
            if open.0 { "FOG  -" } else { "FOG  +" }.to_owned()
        })),))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
        .on_activate(|world| {
            let mut open = world.resource_mut::<FogOpen>();
            open.0 = !open.0;
        })
    })
}

pub(super) fn page() -> impl View {
    parts((
        field(
            "Visual quality",
            told(
                quality(),
                description("Particles, bloom, fog and shadows."),
            ),
        ),
        field("Time of day", hour()),
        column((
            fog_head(),
            keyed(resource::<FogOpen, _>(|open| open.0), |open| {
                if *open {
                    fog().boxed()
                } else {
                    description("Shafts of light through the fog.")
                        .boxed()
                }
            }),
        ))
        .gap(0.0),
    ))
}
