//! The Sound page: the sounds, the music and the ambience, each on or
//! off, and their levels.

use bevy::app::App;
use bevy::ui::{AlignItems, percent};
use hamlet_core::fynix::views::{FrameProps, row, slider};
use hamlet_core::fynix::{Binding, Prop, ViewExt, resource};
use hamlet_core::settings::{
    SettingsAppExt, SettingsPage, alone, field, shown,
};
use hamlet_core::ui::styles::parts;
use hamlet_core::ui::widgets::icons::Glyph;
use hamlet_core::ui::widgets::switch::switch;
use hamlet_core::ui::{View, with_cx};

use super::SoundSettings;

pub(super) fn plugin(app: &mut App) {
    app.add_settings_page(
        SettingsPage::new(SOUND, "Sound", Glyph::Speaker, 80, || {
            page().boxed()
        })
        .now(now),
    );
}

/// The id of the Sound page.
pub const SOUND: &str = "sound";

fn now() -> Prop<String> {
    shown::<SoundSettings>(|s| {
        if s.muted { "Off" } else { "On" }.to_string()
    })
}

/// A kind of sound's switch and level.
fn level(
    on: fn(&SoundSettings) -> bool,
    turn: fn(&mut SoundSettings, bool),
    at: fn(&SoundSettings) -> f32,
    set: fn(&mut SoundSettings, f32),
) -> impl View {
    with_cx(move |cx| {
        row((
            switch(
                resource::<SoundSettings, _>(on),
                move |world, to| {
                    turn(
                        &mut world.resource_mut::<SoundSettings>(),
                        to,
                    );
                },
            ),
            slider(Binding::new(
                resource::<SoundSettings, _>(at),
                move |world, to| {
                    set(
                        &mut world.resource_mut::<SoundSettings>(),
                        to,
                    );
                },
            ))
            .grow(1.0),
        ))
        .gap(cx.theme().space.gap)
        .width(percent(100.0))
        .align(AlignItems::Center)
    })
}

/// A switch alone: a group of sounds that is on or off.
fn sound_switch(
    on: fn(&SoundSettings) -> bool,
    turn: fn(&mut SoundSettings, bool),
) -> impl View {
    switch(resource::<SoundSettings, _>(on), move |world, to| {
        turn(&mut world.resource_mut::<SoundSettings>(), to);
    })
}

fn page() -> impl View {
    parts((
        field(
            "Sound",
            level(
                |s| !s.muted,
                |s, on| s.muted = !on,
                |s| s.sound_level,
                |s, level| s.sound_level = level,
            ),
        ),
        field(
            "Music",
            level(
                |s| s.music_on,
                |s, on| s.music_on = on,
                |s| s.music_level,
                |s, level| s.music_level = level,
            ),
        ),
        field(
            "Ambience",
            alone(sound_switch(
                |s| s.ambience_on,
                |s, on| s.ambience_on = on,
            )),
        ),
    ))
}
