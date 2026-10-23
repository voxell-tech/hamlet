//! The audio service, on Bevy's audio. A clip is samples handed over
//! as they are to be heard, whole or a part at a time while it
//! plays; a sound plays one unplaced, or placed at
//! a point or an entity and heard from its owner's listener. Each is
//! of a [`Category`] the person's settings scale, under a master
//! volume, and a plugin's are kept by its [`Ledger`].
//! The owner is an entity; this crate knows nothing of how it runs.
//!
//! [`Category`]: hamlet_engine_api::audio::Category

mod clip;
mod ledger;
mod play;
mod settings;

use bevy::app::App;
use bevy::asset::{AssetApp, AssetPlugin, Assets};
use bevy::audio::{AddAudioSource, AudioPlugin};

pub use crate::clip::{Clip, RATES};
pub use crate::ledger::{
    AppendError, AudioCaps, AudioChange, Ledger, Place, Playback,
    SPEEDS,
};
pub use crate::play::{
    Audio, AudioCommandsExt, AudioWorldExt, Placed, Sounding,
};
pub use crate::settings::{
    Level, Muted, SoundFile, SoundSettings, SoundTuning,
};

pub fn plugin(app: &mut App) {
    // Without Bevy's audio, as in the harness, sounds are kept and
    // come to their end all the same, and nothing is heard.
    if app.is_plugin_added::<AudioPlugin>() {
        app.add_audio_source::<Clip>();
    } else if app.is_plugin_added::<AssetPlugin>() {
        app.init_asset::<Clip>();
    } else {
        app.init_resource::<Assets<Clip>>();
    }
    app.add_plugins((settings::plugin, play::plugin));
}
