//! The Sight page: how much the world draws, the hour its sky shows,
//! how far the distance fog lies and the volumetric fog the sun
//! throws shafts through.

use bevy::app::App;
use bevy::ecs::component::Mutable;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, TooltipExt, column, dropdown, label, row, slider,
};
use bevy_fynix::{
    ModifierExt, ViewExt, derived, every_frame, resource,
};
use hamlet_realm::sky::{
    DistanceFogSettings, Quality, SkySettings, VolumetricFogSettings,
};
use hamlet_realm::{North, Sky};

use super::{field, told};
use crate::dial::{Face, dial};
use crate::disclosure::disclosure;
use crate::icons::{Glyph, glyph};
use crate::looks::{action, close, description, note, parts};
use crate::switch::labelled_switch;
use crate::world_view::{NorthTarget, TimeOfDay};
use crate::{View, built, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<FogOpen>()
        .init_resource::<TimeOfDay>()
        .init_resource::<NorthTarget>();
}

/// True while the volumetric fog's settings are shown.
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
                Face::HOURS,
                derived(|world: &World| {
                    let set = world.resource::<SkySettings>();
                    let hour =
                        world.resource::<TimeOfDay>().target(set);
                    if set.is_night(hour) {
                        Glyph::Moon
                    } else {
                        Glyph::Sun
                    }
                })
                .when(every_frame()),
                // The clock's hour moves without any change to
                // watch.
                derived(|world: &World| {
                    world
                        .resource::<TimeOfDay>()
                        .target(world.resource::<SkySettings>())
                })
                .when(every_frame()),
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
                                world.resource::<TimeOfDay>().target(
                                    world.resource::<SkySettings>(),
                                ),
                            )
                        };
                        *world.resource_mut::<TimeOfDay>() = to;
                    },
                ),
                description("Drag to hold an hour."),
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

/// The dial of where the sun's path lies.
fn north() -> impl View {
    with_cx(|cx| {
        row((
            dial(
                Face::DEGREES,
                Glyph::Compass,
                resource::<NorthTarget, _>(|north| north.0),
                resource::<North, _>(|north| north.0),
                |world, degrees| {
                    world.resource_mut::<NorthTarget>().0 = degrees;
                },
            ),
            column((description("Where the sun rises."),))
                .grow(1.0)
                .min_width(px(0.0)),
        ))
        .gap(cx.theme().space.section)
        .align(AlignItems::Center)
    })
}

/// The side of the icon that names a slider.
const AMOUNT_ICON: f32 = 16.0;

/// One number of `R`: an icon named by its tooltip, a slider from
/// `least` to `most` and the number itself.
fn amount<R: Resource<Mutability = Mutable>>(
    which: Glyph,
    name: &'static str,
    at: fn(&R) -> f32,
    set: fn(&mut R, f32),
    (least, most): (f32, f32),
) -> impl View {
    with_cx(move |cx| {
        row((
            glyph(which, AMOUNT_ICON)
                .tone(Tone::Dim)
                .tooltip(move || note(name)),
            slider(resource::<R, _>(at))
                .range(least, most)
                .on_change(move |world, to| {
                    set(&mut world.resource_mut::<R>(), to);
                })
                // From nothing, so it takes the room the row has
                // rather than pushing the row past the page.
                .width(px(0.0))
                .min_width(px(0.0))
                .grow(1.0),
            // As many places as the range needs: none for a
            // distance, three for a share.
            label(resource::<R, _>(move |settings| {
                let places = if most >= 100.0 { 0 } else { 3 };
                format!("{:.*}", places, at(settings))
            }))
            .wide(px(48.0)),
        ))
        .gap(cx.theme().space.gap)
        .width(percent(100.0))
        .align(AlignItems::Center)
    })
}

/// The volumetric fog's switch, its numbers and the way back to the
/// designed fog.
fn volumetric_fog() -> impl View {
    type Fog = VolumetricFogSettings;
    close((
        labelled_switch(
            "On",
            resource::<Fog, _>(|fog| fog.on),
            |world, on| world.resource_mut::<Fog>().on = on,
        ),
        amount::<Fog>(
            Glyph::Eye,
            "Visibility",
            |f| f.visibility,
            |f, v| f.visibility = v,
            (100.0, 4000.0),
        ),
        amount::<Fog>(
            Glyph::Rise,
            "Height",
            |f| f.height,
            |f, v| f.height = v,
            (5.0, 200.0),
        ),
        amount::<Fog>(
            Glyph::Rays,
            "Light",
            |f| f.strength,
            |f, v| f.strength = v,
            (0.0, 6.0),
        ),
        amount::<Fog>(
            Glyph::Halo,
            "Glow",
            |f| f.glow,
            |f, v| f.glow = v,
            (0.0, 0.95),
        ),
        amount::<Fog>(
            Glyph::Scatter,
            "Scattering",
            |f| f.scattering,
            |f, v| f.scattering = v,
            (0.0, 1.0),
        ),
        amount::<Fog>(
            Glyph::Drop,
            "Absorption",
            |f| f.absorption,
            |f, v| f.absorption = v,
            (0.0, 0.5),
        ),
        action(Glyph::Restore, "RESET", |world: &mut World| {
            world.insert_resource(Fog::default());
        }),
    ))
}

pub(super) fn page() -> impl View {
    parts((
        field(
            "Visual quality",
            told(
                quality(),
                description("Particles, bloom, fog, shadows."),
            ),
        ),
        field("Time of day", hour()),
        field("North", north()),
        field(
            "Distance fog",
            told(
                amount::<DistanceFogSettings>(
                    Glyph::Mountains,
                    "Distance",
                    |f| f.distance,
                    |f, v| f.distance = v,
                    (200.0, 2000.0),
                ),
                description("Further draws more land."),
            ),
        ),
        disclosure(
            "Volumetric fog",
            |open: &FogOpen| open.0,
            |world| {
                let mut open = world.resource_mut::<FogOpen>();
                open.0 = !open.0;
            },
            || volumetric_fog().boxed(),
        ),
    ))
}
