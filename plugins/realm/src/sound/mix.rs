//! The cues that are played: where the realm is heard from, how many
//! of a family sound at once, and the ambience under them.

use std::collections::HashMap;

use glam::Vec3;
use hamlet_engine_bevy::audio::Category;
use hamlet_engine_bevy::prelude::*;

use crate::realm::Clock;
use crate::sound::bank::Bank;
use crate::sound::heard::{DayPart, Due};
use crate::sound::kinds::{Family, Sound};
use crate::sound::{Cue, SoundSettings};
use crate::view::camera::Camera;

/// Seconds past its end a sound is taken away by the realm, should
/// the engine not have played it.
const OVERDUE: f32 = 1.0;

/// True while the realm's view is shown, and so heard.
#[derive(Resource, Default)]
pub struct Hearing(pub bool);

/// The place the realm is heard from.
#[derive(Component)]
pub struct Ears;

/// A one-shot sound that is playing, and the clock time it ends.
#[derive(Component)]
pub struct Voice {
    family: Family,
    ends: f32,
}

/// A loop of the ambience, and how much of it is heard, 0 to 1.
#[derive(Component)]
pub struct Ambient {
    sound: Sound,
    level: f32,
}

/// The times an alert last sounded and each sound last played, in
/// the realm's seconds.
#[derive(Resource, Default)]
pub struct Heard {
    alert: Option<f32>,
    last: HashMap<Sound, f32>,
}

pub fn spawn_ears(mut commands: Commands) {
    commands.spawn((Ears, SpatialListener));
}

/// Hears the realm while its view is shown, and stops every sound of
/// it while it is not.
pub fn listen(
    mut commands: Commands,
    camera: Option<Res<Camera>>,
    mut hearing: ResMut<Hearing>,
    voices: Query<Entity, With<Voice>>,
    beds: Query<Entity, With<Ambient>>,
) {
    let shown = camera.is_some_and(|camera| camera.shown());
    if hearing.0 == shown {
        return;
    }
    hearing.0 = shown;
    if !shown {
        for sound in voices.iter().chain(&beds) {
            commands.entity(sound).despawn();
        }
    }
}

/// Puts the ears between the ground the camera looks at and the
/// camera, turned as the camera is.
pub fn place_ears(
    set: Res<SoundSettings>,
    camera: Option<Res<Camera>>,
    mut ears: Single<&mut Transform, With<Ears>>,
) {
    let Some(camera) = camera else {
        return;
    };
    let frame = camera.stand();
    let eye = frame.at;
    let forward = frame.turn * Vec3::NEG_Z;
    let reach = if forward.y < -0.05 {
        (-eye.y / forward.y).min(set.furthest)
    } else {
        set.furthest
    };
    let at = eye + forward * reach * (1.0 - set.listener);
    let to =
        Transform::from_translation(at).with_rotation(frame.turn);
    if **ears != to {
        **ears = to;
    }
}

/// Starts the loops of the ambience, silent, once their clips are
/// made.
pub fn start_ambience(
    mut commands: Commands,
    hearing: Res<Hearing>,
    bank: Res<Bank>,
    playing: Query<(), With<Ambient>>,
) {
    if !hearing.0 || !playing.is_empty() {
        return;
    }
    let loops = Sound::ALL
        .into_iter()
        .filter(|sound| sound.ambient())
        .map(|sound| bank.clip(sound).map(|clip| (sound, clip)))
        .collect::<Option<Vec<_>>>();
    for (sound, clip) in loops.into_iter().flatten() {
        commands.spawn((
            Ambient { sound, level: 0.0 },
            AudioPlayer(clip),
            PlaybackSettings::LOOP
                .with_volume(0.0)
                .with_category(Category::World),
        ));
    }
}

/// Birds by day, crickets after dark and wind always, each louder or
/// softer with the camera's height.
pub fn mix_ambience(
    time: Res<Time>,
    set: Res<SoundSettings>,
    part: Res<DayPart>,
    ears: Single<&Transform, With<Ears>>,
    mut beds: Query<(&mut Ambient, &mut PlaybackSettings)>,
) {
    let dark = matches!(*part, DayPart::Dusk | DayPart::Night);
    let near = (set.close / ears.translation.y.max(1e-3)).min(1.0);
    let step = time.delta_secs() / set.fade.max(1e-3);
    for (mut bed, mut settings) in &mut beds {
        let (want, heard) = match bed.sound {
            Sound::Birds => (f32::from(!dark), near * set.birds),
            Sound::Crickets => (f32::from(dark), near),
            // The higher the camera, the more of the world is wind.
            _ => (1.0, 1.0 - 0.6 * near),
        };
        bed.level += (want - bed.level).clamp(-step, step);
        let volume = bed.level * heard * set.ambience;
        if settings.volume != volume {
            settings.volume = volume;
        }
    }
}

/// True when a cue is let through: its family is not full, the same
/// sound has not just played, and an alert is not one of several.
fn admit(
    cue: &Cue,
    set: &SoundSettings,
    heard: &mut Heard,
    playing: &HashMap<Family, usize>,
    now: f32,
) -> bool {
    let family = cue.sound.family();
    let limit = family.limit(set);
    let of_family = playing.get(&family).copied().unwrap_or(0);
    let recent = |at: &f32| now - at < limit.gap;
    if of_family >= limit.most
        || heard.last.get(&cue.sound).is_some_and(recent)
    {
        return false;
    }
    if cue.sound.alert() {
        // Alerts close together are one.
        if heard.alert.is_some_and(|at| now - at < set.alert_gap) {
            return false;
        }
        heard.alert = Some(now);
    }
    heard.last.insert(cue.sound, now);
    true
}

/// Plays the cues of the frame, and those due now, that are near
/// enough to hear.
#[allow(clippy::too_many_arguments)]
pub fn play(
    mut commands: Commands,
    clock: Res<Clock>,
    set: Res<SoundSettings>,
    hearing: Res<Hearing>,
    bank: Res<Bank>,
    ears: Single<&Transform, With<Ears>>,
    voices: Query<&Voice>,
    sounds: Res<SoundAssets>,
    mut heard: ResMut<Heard>,
    mut due: ResMut<Due>,
    mut cues: MessageReader<Cue>,
) {
    let now = clock.now;
    let cues = due
        .take(now)
        .into_iter()
        .chain(cues.read().copied())
        .collect::<Vec<_>>();
    if !hearing.0 {
        return;
    }
    let mut playing = HashMap::<Family, usize>::new();
    for voice in &voices {
        *playing.entry(voice.family).or_default() += 1;
    }
    let worldly = |playing: &HashMap<Family, usize>| {
        playing
            .iter()
            .filter(|(family, _)| family.worldly())
            .map(|(_, count)| count)
            .sum::<usize>()
    };
    for cue in cues {
        let family = cue.sound.family();
        let everywhere = cue.sound.everywhere()
            || cue.sound.alert() && set.alerts_everywhere;
        let far = ears.translation.distance(cue.at) > set.earshot;
        if family.worldly()
            && !everywhere
            && (far || worldly(&playing) >= set.voices)
        {
            continue;
        }
        if !admit(&cue, &set, &mut heard, &playing, now) {
            continue;
        }
        let Some(clip) = bank.clip(cue.sound) else {
            continue;
        };
        let speed = cue.sound.speed(&set, cue.turn);
        let long =
            sounds.get(&clip).map_or(0.0, |clip| clip.seconds());
        let sway = (cue.turn.wrapping_mul(40_503) >> 4 & 0xff) as f32
            / 255.0
            * 2.0
            - 1.0;
        let volume = family.volume(&set) * (1.0 + sway * set.sway);
        let settings = PlaybackSettings::DESPAWN
            .with_volume(volume)
            .with_speed(speed)
            .with_category(Category::World)
            .with_spatial(!everywhere);
        let voice = Voice {
            family,
            ends: now + long / speed + OVERDUE,
        };
        let mut sound =
            commands.spawn((voice, AudioPlayer(clip), settings));
        if !everywhere {
            // The chief's own doings by hand are never too far to
            // hear.
            let at = if family == Family::Hand {
                let from = cue.at - ears.translation;
                ears.translation + from.clamp_length_max(set.hand_far)
            } else {
                cue.at
            };
            sound.insert(Transform::from_translation(at));
        }
        *playing.entry(family).or_default() += 1;
    }
}

/// Takes away a sound the engine did not play, once it would have
/// ended.
pub fn end_voices(
    mut commands: Commands,
    clock: Res<Clock>,
    voices: Query<(Entity, &Voice)>,
) {
    for (entity, voice) in &voices {
        if voice.ends < clock.now {
            commands.entity(entity).despawn();
        }
    }
}
