//! The Sight page: how much the world draws, whether the window waits
//! for the screen, the camera put back, the hour its sky shows,
//! how far the distance fog lies, how hard the wind blows and the
//! volumetric fog the sun throws shafts through.

use bevy::app::App;
use bevy::ecs::component::Mutable;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, TooltipExt, dropdown, label, row, slider,
};
use bevy_fynix::{ViewExt, derived, every_frame, resource};
use hamlet_realm::pick::Orbit;
use hamlet_realm::sky::{
    DistanceFogSettings, Quality, SkySettings, VolumetricFogSettings,
};
use hamlet_realm::{North, Sky};
use hamlet_ui::styles::{action, close, note, parts};
use hamlet_ui::widgets::dial::{Face, dial};
use hamlet_ui::widgets::disclosure::disclosure;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::range::range;
use hamlet_ui::widgets::switch::labelled_switch;
use hamlet_ui::{AnyView, View, built, with_cx};
use hamlet_vfx::wind::WindChoice;

use super::{alone, field};
use crate::roll::moved;
use crate::window::{self, Vsync};
use crate::world_view::{NorthTarget, TimeOfDay};

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
                Glyph::Chevron.image(hamlet_ui::styles::ICON),
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

/// The dial of the sky's hour, faint while it follows the clock, and
/// whether it does.
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
                resource::<TimeOfDay, _>(|t| *t == TimeOfDay::Clock),
                |world, hour| {
                    *world.resource_mut::<TimeOfDay>() =
                        TimeOfDay::At(hour);
                },
            ),
            labelled_switch(
                "Follow the clock",
                resource::<TimeOfDay, _>(|t| *t == TimeOfDay::Clock),
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
        ))
        .gap(cx.theme().space.section)
        .align(AlignItems::Center)
    })
}

/// The dial of where the sun's path lies.
fn north() -> impl View {
    dial(
        Face::DEGREES,
        Glyph::Compass,
        resource::<NorthTarget, _>(|north| north.0),
        resource::<North, _>(|north| north.0),
        false,
        |world, degrees| {
            world.resource_mut::<NorthTarget>().0 = degrees;
        },
    )
}

/// The side of the icon that names a slider.
const AMOUNT_ICON: f32 = 16.0;

/// How a slider's number is written.
#[derive(Clone, Copy)]
enum Unit {
    /// A distance in the world.
    Metres,
    /// A share of a whole.
    Share,
    /// A number to two places.
    Plain,
}

impl Unit {
    fn write(self, amount: f32) -> String {
        match self {
            Unit::Metres => format!("{amount:.0} m"),
            Unit::Share => format!("{:.0}%", amount * 100.0),
            Unit::Plain => format!("{amount:.2}"),
        }
    }
}

/// One number of `R`: an icon named by its tooltip, a slider from
/// `least` to `most` and the number itself.
fn amount<R: Resource<Mutability = Mutable>>(
    which: Glyph,
    name: &'static str,
    at: fn(&R) -> f32,
    set: fn(&mut R, f32),
    (least, most): (f32, f32),
    unit: Unit,
) -> impl View {
    with_cx(move |cx| {
        row((
            glyph(which, AMOUNT_ICON)
                .tone(Tone::Dim)
                .tooltip(move || note(name)),
            track(number(at, set, (least, most), unit)),
        ))
        .gap(cx.theme().space.gap)
        .width(percent(100.0))
        .align(AlignItems::Center)
    })
}

/// A slider of one number of `R` from `least` to `most`, and the
/// number itself in `unit`.
fn number<R: Resource<Mutability = Mutable>>(
    at: fn(&R) -> f32,
    set: fn(&mut R, f32),
    (least, most): (f32, f32),
    unit: Unit,
) -> (AnyView, AnyView) {
    (
        slider(resource::<R, _>(at))
            .range(least, most)
            .on_change(move |world, to| {
                set(&mut world.resource_mut::<R>(), to);
            })
            // From nothing, so it takes the room the row has rather
            // than pushing the row past the page.
            .width(px(0.0))
            .min_width(px(0.0))
            .grow(1.0)
            .boxed(),
        value(label(resource::<R, _>(move |settings| {
            unit.write(at(settings))
        }))),
    )
}

/// A slider's value, in a column as wide on every row.
fn value(shown: impl View + 'static) -> AnyView {
    row((shown,)).width(px(72.0)).shrink(0.0).boxed()
}

/// A slider's row: the slider and its value.
fn track((slider, value): (AnyView, AnyView)) -> impl View {
    with_cx(move |cx| {
        row((slider, value))
            .gap(cx.theme().space.gap)
            .width(percent(100.0))
            .align(AlignItems::Center)
    })
}

/// How hard the wind blows at its calmest and at its strongest,
/// both on one track.
fn wind_strength() -> impl View {
    track((
        range(
            resource::<WindChoice, _>(|wind| wind.least),
            resource::<WindChoice, _>(|wind| wind.most),
            WindChoice::STRENGTH,
            |world, least, most| {
                let mut wind = world.resource_mut::<WindChoice>();
                if (wind.least, wind.most) != (least, most) {
                    wind.least = least;
                    wind.most = most;
                }
            },
        )
        .boxed(),
        value(label(resource::<WindChoice, _>(|wind| {
            format!(
                "{:.0}-{}",
                wind.least * 100.0,
                Unit::Share.write(wind.most)
            )
        }))),
    ))
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
            Unit::Metres,
        ),
        amount::<Fog>(
            Glyph::Rise,
            "Height",
            |f| f.height,
            |f, v| f.height = v,
            (5.0, 200.0),
            Unit::Metres,
        ),
        amount::<Fog>(
            Glyph::Rays,
            "Light",
            |f| f.strength,
            |f, v| f.strength = v,
            (0.0, 6.0),
            Unit::Plain,
        ),
        amount::<Fog>(
            Glyph::Halo,
            "Glow",
            |f| f.glow,
            |f, v| f.glow = v,
            (0.0, 0.95),
            Unit::Plain,
        ),
        amount::<Fog>(
            Glyph::Scatter,
            "Scattering",
            |f| f.scattering,
            |f, v| f.scattering = v,
            (0.0, 1.0),
            Unit::Plain,
        ),
        amount::<Fog>(
            Glyph::Drop,
            "Absorption",
            |f| f.absorption,
            |f, v| f.absorption = v,
            (0.0, 0.5),
            Unit::Plain,
        ),
        action(Glyph::Restore, "Reset", |world: &mut World| {
            world.insert_resource(Fog::default());
        }),
    ))
}

pub(super) fn page() -> impl View {
    parts((
        field("Visual quality", alone(quality())),
        field(
            "Screen sync",
            alone(labelled_switch(
                "On",
                derived(|world: &World| {
                    world.get_resource::<Vsync>().is_none_or(|v| v.0)
                })
                .when(moved::<Vsync>()),
                window::set_vsync,
            )),
        ),
        field(
            "Camera",
            alone(action(
                Glyph::Aim,
                "Reset",
                |world: &mut World| {
                    *world.resource_mut::<Orbit>() = Orbit::default();
                },
            )),
        ),
        with_cx(|cx| {
            row((
                field("Time of day", hour()),
                field("North", north()),
            ))
            .gap(cx.theme().space.panel * 2.0)
            .align(AlignItems::FlexStart)
        }),
        field(
            "Distance fog",
            track(number::<DistanceFogSettings>(
                |f| f.distance,
                |f, v| f.distance = v,
                (200.0, 2000.0),
                Unit::Metres,
            )),
        ),
        field("Wind strength", wind_strength()),
        field(
            "Wind unevenness",
            track(number::<WindChoice>(
                |wind| wind.turbulence,
                |wind, to| wind.turbulence = to,
                (0.0, 1.0),
                Unit::Share,
            )),
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
