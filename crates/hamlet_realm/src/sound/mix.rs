//! The cues that are played: where the world is heard from, how many
//! of a family sound at once, and the ambience under them.

use std::collections::HashMap;

use bevy::audio::{SpatialScale, Volume};
use bevy::prelude::*;

use super::clip::Bank;
use super::{Cue, Family, Hearing, Sound, SoundSettings};
use crate::{DayPart, WorldCamera};

/// The place the world is heard from.
#[derive(Component)]
pub struct Ears;

/// A one-shot sound that is playing.
#[derive(Component)]
pub(super) struct Voice(Family);

/// A loop of the ambience, and how much of it is heard, 0 to 1.
#[derive(Component)]
pub(super) struct Ambient {
    sound: Sound,
    level: f32,
}

/// The times an alert last sounded and each sound last played, in
/// seconds since the app began.
#[derive(Resource, Default)]
pub(super) struct Heard {
    alert: Option<f32>,
    last: HashMap<Sound, f32>,
}

/// A cue's fate at [`admit`]: let through, or held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Admission {
    Passed,
    Held,
}

pub(super) fn spawn_ears(
    mut commands: Commands,
    set: Res<SoundSettings>,
) {
    commands.spawn((Ears, SpatialListener::new(set.ears)));
}

/// Puts the ears between the ground the camera looks at and the
/// camera, turned as the camera is.
pub(super) fn place_ears(
    set: Res<SoundSettings>,
    camera: Single<&GlobalTransform, With<WorldCamera>>,
    mut ears: Single<
        (&mut Transform, &mut SpatialListener),
        With<Ears>,
    >,
) {
    let eye = camera.translation();
    let forward = camera.forward();
    let reach = if forward.y < -0.05 {
        (-eye.y / forward.y).min(set.furthest)
    } else {
        set.furthest
    };
    let at = eye + forward * reach * (1.0 - set.listener);
    let to = Transform::from_translation(at)
        .with_rotation(camera.rotation());
    ears.0.set_if_neq(to);
    let gap = Vec3::X * set.ears * 0.5;
    if ears.1.right_ear_offset != gap {
        *ears.1 = SpatialListener::new(set.ears);
    }
}

pub(super) fn start_ambience(
    mut commands: Commands,
    bank: Res<Bank>,
    playing: Query<(), With<Ambient>>,
) {
    if !playing.is_empty() {
        return;
    }
    for sound in Sound::ALL.into_iter().filter(|s| s.is_ambient()) {
        let Some(clip) = bank.clip(sound) else {
            continue;
        };
        commands.spawn((
            Ambient { sound, level: 0.0 },
            AudioPlayer(clip),
            PlaybackSettings::LOOP.with_volume(Volume::SILENT),
            DespawnOnExit(Hearing::On),
        ));
    }
}

/// Birds by day, crickets after dark and wind always, each louder or
/// softer with the camera's height.
pub(super) fn mix_ambience(
    time: Res<Time>,
    set: Res<SoundSettings>,
    part: Res<State<DayPart>>,
    ears: Single<&Transform, With<Ears>>,
    mut beds: Query<(&mut Ambient, &mut AudioSink)>,
) {
    let dark = matches!(part.get(), DayPart::Dusk | DayPart::Night);
    let near = (set.close / ears.translation.y.max(1e-3)).min(1.0);
    let step = time.delta_secs() / set.fade.max(1e-3);
    for (mut bed, mut sink) in &mut beds {
        let (want, heard) = match bed.sound {
            Sound::Birds => (f32::from(!dark), near * set.birds),
            Sound::Crickets => (f32::from(dark), near),
            // The higher the camera, the more of the world is wind.
            _ => (1.0, 1.0 - 0.6 * near),
        };
        bed.level += (want - bed.level).clamp(-step, step);
        let volume = bed.level
            * heard
            * set.ambience
            * set.volume
            * set.sound_level;
        sink.set_volume(Volume::Linear(volume));
    }
}

/// Whether a cue is let through: its family is not full, the same
/// sound has not just played, and an alert is not one of several.
/// One let through is noted as heard.
pub(super) fn admit(
    cue: &Cue,
    set: &SoundSettings,
    heard: &mut Heard,
    playing: &HashMap<Family, usize>,
    now: f32,
) -> Admission {
    let family = cue.sound.family();
    let limit = family.limit(set);
    let of_family = playing.get(&family).copied().unwrap_or(0);
    let recent = |at: &f32| now - at < limit.gap;
    if of_family >= limit.most
        || heard.last.get(&cue.sound).is_some_and(recent)
    {
        return Admission::Held;
    }
    if cue.sound.is_alert() {
        // Alerts close together are one.
        if heard.alert.is_some_and(|at| now - at < set.alert_gap) {
            return Admission::Held;
        }
        heard.alert = Some(now);
    }
    heard.last.insert(cue.sound, now);
    Admission::Passed
}

/// Plays the cues that are near enough to hear.
pub(super) fn play(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<SoundSettings>,
    bank: Res<Bank>,
    ears: Single<&Transform, With<Ears>>,
    voices: Query<&Voice>,
    mut heard: ResMut<Heard>,
    mut cues: MessageReader<Cue>,
) {
    let now = time.elapsed_secs();
    let mut playing = HashMap::<Family, usize>::new();
    for voice in &voices {
        *playing.entry(voice.0).or_default() += 1;
    }
    let worldly = |playing: &HashMap<Family, usize>| {
        playing
            .iter()
            .filter(|(family, _)| family.is_worldly())
            .map(|(_, count)| count)
            .sum::<usize>()
    };
    let cues = cues.read().copied().collect::<Vec<_>>();
    // An answer has a sound of its own, so the button that gave it
    // keeps quiet.
    let answered =
        cues.iter().any(|cue| cue.sound.family() == Family::Answers);
    for cue in cues {
        let family = cue.sound.family();
        let button =
            matches!(cue.sound, Sound::Confirm | Sound::Deny);
        if answered && button {
            continue;
        }
        let everywhere = cue.sound.is_heard_everywhere()
            || cue.sound.is_alert() && set.alerts_everywhere;
        let far = ears.translation.distance(cue.at) > set.earshot;
        if family.is_worldly()
            && !everywhere
            && (far || worldly(&playing) >= set.voices)
        {
            continue;
        }
        if admit(&cue, &set, &mut heard, &playing, now)
            == Admission::Held
        {
            continue;
        }
        let Some(clip) = bank.clip(cue.sound) else {
            continue;
        };
        // The chief's own doings by hand are never too far to hear.
        let at = if family == Family::Hand {
            let from = cue.at - ears.translation;
            ears.translation + from.clamp_length_max(set.hand_far)
        } else {
            cue.at
        };
        let sway = (cue.turn.wrapping_mul(40_503) >> 4 & 0xff) as f32
            / 255.0
            * 2.0
            - 1.0;
        let volume = family.volume(&set)
            * set.volume
            * set.sound_level
            * (1.0 + sway * set.sway);
        let settings = PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(volume))
            .with_speed(cue.sound.speed(&set, cue.turn))
            .with_spatial(!everywhere)
            .with_spatial_scale(SpatialScale::new(1.0 / set.reach));
        commands.spawn((
            Voice(family),
            AudioPlayer(clip),
            settings,
            Transform::from_translation(at),
            DespawnOnExit(Hearing::On),
        ));
        *playing.entry(family).or_default() += 1;
    }
}
