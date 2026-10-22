//! The Sound page of Settings: the master volume, then each
//! category's, each a switch and a slider.

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, percent};
use bevy_fynix::resource;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row, slider};
use hamlet_engine_api::PanelId;
use hamlet_engine_api::audio::Category;
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_audio::{Level, SoundSettings};

use crate::icons::Glyph;
use crate::style::{note, switch};
use crate::{AnyView, Cx, SettingsPage};

/// The name of the Sound page.
pub(crate) const SOUND: &str = "sound";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, add);
}

fn add(mut commands: Commands) {
    commands.spawn(SettingsPage {
        id: PanelId::new(ENGINE, SOUND).expect("a valid id"),
        title: "Sound".into(),
        about: "The volume of each kind of sound.".into(),
        icon: Glyph::Speaker,
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    let mut parts = vec![volume("Master", None)];
    for category in Category::ALL {
        parts.push(volume(category.name(), Some(category)));
    }
    parts.push(note(
        "A plugin's own sounds are muted on its page in Plugins.",
    ));
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.panel;
        cx.build(column(parts).gap(gap))
    })
}

/// The master volume, or the volume of a category.
fn level(settings: &SoundSettings, of: Option<Category>) -> Level {
    of.map_or(settings.master, |category| *settings.of(category))
}

fn set(
    world: &mut World,
    of: Option<Category>,
    change: impl FnOnce(&mut Level),
) {
    let mut settings = world.resource_mut::<SoundSettings>();
    change(match of {
        Some(category) => settings.of_mut(category),
        None => &mut settings.master,
    });
}

/// A kind of sound's name over its switch and the slider of its
/// level, dim while it is off.
fn volume(name: &'static str, of: Option<Category>) -> AnyView {
    let on = switch(
        resource::<SoundSettings, _>(move |settings| {
            level(settings, of).on
        }),
        move |world, on| set(world, of, |level| level.on = on),
    );
    let slid =
        slider(resource::<SoundSettings, _>(move |settings| {
            level(settings, of).level
        }))
        .on_change(move |world, to| {
            set(world, of, |level| level.level = to);
        })
        .grow(1.0);
    let tone = resource::<SoundSettings, _>(move |settings| {
        if level(settings, of).on {
            Tone::Body
        } else {
            Tone::Dim
        }
    });
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                label(name).tone(tone),
                row((on, slid))
                    .gap(gap)
                    .width(percent(100.0))
                    .align(AlignItems::Center),
            ))
            .gap(gap / 2.0),
        )
    })
}
