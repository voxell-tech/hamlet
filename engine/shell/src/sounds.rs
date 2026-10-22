//! The interface's sounds: a tick as the pointer comes onto a
//! button, a tap as a control goes down, a chime as the action a
//! panel is for is done and two dull notes as one refuses, air as a
//! menu or popup opens and closes, a click as a tab comes to the
//! front, wood as a dock tab is taken hold of and set down, paper as
//! a fold opens or shuts, tines as a message is sent, the window's
//! glass as it is hidden or shown, and a muted bar as an error is
//! logged. Each is written as a [`Cue`] and played here, of the
//! category interface.

mod synth;

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::window::PrimaryWindow;
use bevy_fynix::dock::{ActiveTab, DockDrag, DockTab};
use bevy_fynix::shortcut::Scope;
use bevy_fynix::views::{Open, POPUP};
use bevy_fynix::{Hovered, Pressed};
use hamlet_engine_api::audio::Category;
use hamlet_engine_audio::{
    AudioCommandsExt, Clip, Placed, Playback, Sounding,
};
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use hamlet_engine_core::{LogLevel, LogLine, TuningPlugin};
use serde::Deserialize;

use crate::style::{Glow, GlowState};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<InterfaceSoundTuning>::default())
        .add_message::<Cue>()
        .add_observer(answered)
        .add_systems(Startup, make_clips)
        .add_systems(
            Update,
            (
                (
                    hovered, pressed, popups, tabs, folds, window,
                    errors,
                ),
                play,
            )
                .chain(),
        );
}

/// How the interface's sounds play.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceSoundTuning {
    /// The most of them playing at once.
    pub most: usize,
    /// Seconds before the same sound may play again.
    pub gap: f32,
    /// The share a sound's pitch may stray from one playing to the
    /// next: of a sound with no note, and of one with a note.
    pub vary: f32,
    pub vary_tuned: f32,
    /// The share a sound's volume may stray from one playing to the
    /// next.
    pub sway: f32,
    /// Seconds after a dock tab is set down in which the tabs
    /// brought to the front came with it, and are not heard.
    pub drag_quiet: f32,
}

impl Tuning for InterfaceSoundTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("interface_sounds");
}

/// One of the interface's sounds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InterfaceSound {
    /// The pointer coming onto a button.
    Tick,
    /// A control going down.
    Tap,
    /// The action a panel is for, done.
    Confirm,
    /// Something refused or stopped.
    Deny,
    /// A menu or a popup opening, and closing.
    PopOpen,
    PopClose,
    /// A tab brought to the front.
    Tab,
    /// A fold opened or shut.
    Fold,
    /// A message sent.
    Send,
    /// A dock tab taken hold of, and set in its place.
    Grab,
    Dock,
    /// The window hidden or shown.
    Sash,
    /// An error logged.
    Error,
}

impl InterfaceSound {
    pub const ALL: [Self; 13] = [
        Self::Tick,
        Self::Tap,
        Self::Confirm,
        Self::Deny,
        Self::PopOpen,
        Self::PopClose,
        Self::Tab,
        Self::Fold,
        Self::Send,
        Self::Grab,
        Self::Dock,
        Self::Sash,
        Self::Error,
    ];

    /// Its samples, one channel at [`synth::RATE`].
    pub fn samples(self) -> Vec<f32> {
        synth::make(self)
    }

    /// The semitones it may be moved by from one playing to the next
    /// and stay in the key. Empty for a sound with no note, whose
    /// pitch strays freely.
    fn steps(self) -> &'static [f32] {
        match self {
            Self::Tap
            | Self::PopOpen
            | Self::PopClose
            | Self::Fold
            | Self::Grab
            | Self::Dock => &[],
            // From the keynote or the fifth, to other notes of the
            // pentatonic scale.
            Self::Tick | Self::Tab => &[0.0, 2.0, -3.0],
            // Figures of several notes stay put.
            _ => &[0.0],
        }
    }

    /// The speed it plays at the `turn`th time: on another note of
    /// the key if it has a note, and a little off either way.
    fn speed(self, tuning: &InterfaceSoundTuning, turn: u32) -> f32 {
        let mixed = turn.wrapping_mul(2_654_435_761);
        let stray = (mixed >> 16) as f32 / 65_535.0 * 2.0 - 1.0;
        let steps = self.steps();
        if steps.is_empty() {
            return 1.0 + stray * tuning.vary;
        }
        let step = steps[(mixed >> 8) as usize % steps.len()];
        2f32.powf(step / 12.0) * (1.0 + stray * tuning.vary_tuned)
    }
}

/// An interface sound to play.
#[derive(Message, Clone, Copy, Debug)]
pub struct Cue(pub InterfaceSound);

/// Each interface sound's clip and its length in seconds.
#[derive(Resource)]
struct Clips(HashMap<InterfaceSound, (Handle<Clip>, f32)>);

fn make_clips(
    mut commands: Commands,
    mut assets: ResMut<Assets<Clip>>,
) {
    let clips = InterfaceSound::ALL
        .into_iter()
        .filter_map(|sound| {
            let clip =
                Clip::new(sound.samples(), 1, synth::RATE).ok()?;
            let seconds = clip.seconds();
            Some((sound, (assets.add(clip), seconds)))
        })
        .collect();
    commands.insert_resource(Clips(clips));
}

/// Plays each cue, as many at once as [`InterfaceSoundTuning::most`]
/// and none again within its `gap`, each a little varied.
fn play(
    mut cues: MessageReader<Cue>,
    clips: Option<Res<Clips>>,
    tuning: Res<InterfaceSoundTuning>,
    time: Res<Time>,
    sounding: Query<&Sounding>,
    mut last: Local<HashMap<InterfaceSound, f32>>,
    mut turn: Local<u32>,
    mut commands: Commands,
) {
    let Some(clips) = clips else {
        cues.clear();
        return;
    };
    let now = time.elapsed_secs();
    let mut playing = sounding
        .iter()
        .filter(|sound| {
            sound.owner.is_none()
                && sound.playback.category == Category::Interface
        })
        .count();
    for &Cue(sound) in cues.read() {
        let recent =
            last.get(&sound).is_some_and(|at| now - at < tuning.gap);
        let Some((clip, seconds)) = clips.0.get(&sound) else {
            continue;
        };
        if playing >= tuning.most || recent {
            continue;
        }
        last.insert(sound, now);
        playing += 1;
        *turn = turn.wrapping_add(1);
        let sway = (turn.wrapping_mul(40_503) >> 4 & 0xff) as f32
            / 255.0
            * 2.0
            - 1.0;
        let playback = Playback {
            category: Category::Interface,
            volume: 1.0 + sway * tuning.sway,
            speed: sound.speed(&tuning, *turn),
            looped: false,
        };
        commands.play_sound(
            clip.clone(),
            *seconds,
            playback,
            Placed::Unplaced,
        );
    }
}

/// The pointer came onto a lettered button or a row.
fn hovered(
    entered: Query<&Glow, Added<Hovered>>,
    mut cues: MessageWriter<Cue>,
) {
    let ticks = |glow: &Glow| {
        matches!(
            glow,
            Glow::Plain
                | Glow::Text
                | Glow::Primary
                | Glow::Critical
                | Glow::Inner
        )
    };
    if entered.iter().any(ticks) {
        cues.write(Cue(InterfaceSound::Tick));
    }
}

/// A button, a switch, a row, a tab or a field went down.
fn pressed(
    pressed: Query<(), (Added<Pressed>, With<GlowState>)>,
    mut cues: MessageWriter<Cue>,
) {
    if !pressed.is_empty() {
        cues.write(Cue(InterfaceSound::Tap));
    }
}

/// The action a panel is for done, or something refused.
fn answered(
    activated: On<Activate>,
    glows: Query<&Glow>,
    mut cues: MessageWriter<Cue>,
) {
    match glows.get(activated.entity) {
        Ok(Glow::Primary) => {
            cues.write(Cue(InterfaceSound::Confirm));
        }
        Ok(Glow::Critical) => {
            cues.write(Cue(InterfaceSound::Deny));
        }
        _ => {}
    }
}

/// A menu, a context menu, a dropdown's list or another popup opened
/// or closed. One taking another's place in a frame is neither.
fn popups(
    scopes: Query<&Scope>,
    mut open: Local<usize>,
    mut cues: MessageWriter<Cue>,
) {
    let now = scopes.iter().filter(|scope| scope.0 == POPUP).count();
    let was = std::mem::replace(&mut *open, now);
    if now > was {
        cues.write(Cue(InterfaceSound::PopOpen));
    } else if now < was {
        cues.write(Cue(InterfaceSound::PopClose));
    }
}

/// The state the dock was last seen in.
#[derive(Default)]
struct Docked {
    dragging: bool,
    /// When a tab was last set down, in seconds.
    let_go: f32,
    /// A tab has been in front.
    tabs: bool,
}

/// A dock tab brought to the front, and one taken hold of and set
/// down. The tabs a tab set down brings with it, and the first tabs
/// of all, were not brought to the front by hand.
fn tabs(
    time: Res<Time>,
    tuning: Res<InterfaceSoundTuning>,
    drag: Res<DockDrag>,
    fronted: Query<(), (Added<ActiveTab>, With<DockTab>)>,
    fronts: Query<(), With<ActiveTab>>,
    mut docked: Local<Docked>,
    mut cues: MessageWriter<Cue>,
) {
    let now = time.elapsed_secs();
    let dragging = matches!(*drag, DockDrag::Dragging { .. });
    if dragging != docked.dragging {
        docked.dragging = dragging;
        let sound = if dragging {
            InterfaceSound::Grab
        } else {
            InterfaceSound::Dock
        };
        cues.write(Cue(sound));
        docked.let_go = now;
    }
    let by_hand =
        docked.tabs && now - docked.let_go > tuning.drag_quiet;
    if !fronted.is_empty() && by_hand {
        cues.write(Cue(InterfaceSound::Tab));
    }
    docked.tabs = !fronts.is_empty();
}

/// A fold opened or shut on a node that was there before.
fn folds(
    opened: Query<Ref<Node>, Added<Open>>,
    mut shut: RemovedComponents<Open>,
    nodes: Query<(), With<Node>>,
    mut cues: MessageWriter<Cue>,
) {
    let opened = opened.iter().any(|node| !node.is_added());
    let shut = shut.read().any(|node| nodes.contains(node));
    if opened || shut {
        cues.write(Cue(InterfaceSound::Fold));
    }
}

/// The window hidden, as it runs in the background, or shown again.
fn window(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut shown: Local<Option<bool>>,
    mut cues: MessageWriter<Cue>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if shown
        .replace(window.visible)
        .is_some_and(|was| was != window.visible)
    {
        cues.write(Cue(InterfaceSound::Sash));
    }
}

/// An error reached the log.
fn errors(
    mut lines: MessageReader<LogLine>,
    mut cues: MessageWriter<Cue>,
) {
    if lines.read().any(|line| line.level == LogLevel::Error) {
        cues.write(Cue(InterfaceSound::Error));
    }
}
