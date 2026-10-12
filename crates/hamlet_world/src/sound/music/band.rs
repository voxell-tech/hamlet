//! The music played in the app: each breath written a little ahead,
//! made in the background and started when it is due.

use bevy::audio::Volume;
use bevy::prelude::*;
use bevy::tasks::futures_lite::future::poll_once;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on};
use bevy::window::PrimaryWindow;

use super::{Composer, Moment, Mood, render};
use crate::generator::society::Society;
use crate::keeper::KState;
use crate::sound::clip::Clip;
use crate::sound::{Cue, Cues, SoundSettings, drawn};
use crate::{DayPart, WorldCamera};

/// A breath or a phrase that is playing.
#[derive(Component)]
struct Strain;

/// A breath that has been played, and the seconds to the next.
type Made = (Vec<f32>, f32);

/// The player of the music, and the breaths it has in hand.
#[derive(Resource)]
struct Band {
    composer: Composer,
    /// Share of the music heard, 0 to 1, and the share of that the
    /// hour leaves.
    level: f32,
    dim: f32,
    /// The time the next breath begins, in seconds since the app
    /// began.
    next: Option<f32>,
    making: Option<Task<Made>>,
    made: Option<Made>,
    /// A moment to answer, and when.
    moment: Option<(Moment, f32)>,
    answering: Option<Task<Vec<f32>>>,
    answered: f32,
}

/// Seconds before a breath is due that it is written and played.
const AHEAD: f32 = 6.0;

/// Share of the hamlet at work, 0 to 1.
fn busy(society: Option<&Society>, set: &SoundSettings) -> f32 {
    let at_work = society.map_or(0, |society| {
        society
            .agents
            .iter()
            .filter(|a| !a.archived && a.state == KState::Working)
            .count()
    });
    (at_work as f32 / set.busy_full.max(1.0)).min(1.0)
}

pub(in crate::sound) fn plugin(app: &mut App) {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(7, |since| since.subsec_nanos());
    app.insert_resource(Band {
        composer: Composer::new(seed),
        level: 0.0,
        dim: 1.0,
        next: None,
        making: None,
        made: None,
        moment: None,
        answering: None,
        answered: f32::MIN,
    })
    .add_systems(Update, conduct.after(Cues));
}

/// Keeps the music going while it is wanted: writes each breath a
/// little ahead, has it played in the background, starts it when
/// it is due, and answers the moments that matter.
#[allow(clippy::too_many_arguments)]
fn conduct(
    mut commands: Commands,
    time: Res<Time<Real>>,
    set: Res<SoundSettings>,
    part: Res<State<DayPart>>,
    society: Option<Res<Society>>,
    camera: Query<&Camera, With<WorldCamera>>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut band: ResMut<Band>,
    mut clips: ResMut<Assets<Clip>>,
    mut strains: Query<
        (Entity, Option<&mut AudioSink>),
        With<Strain>,
    >,
    mut cues: MessageReader<Cue>,
) {
    let band = &mut *band;
    let now = time.elapsed_secs();
    let want = set.score() && drawn(&camera, &window);
    let step = time.delta_secs() / set.music_fade.max(1e-3);
    band.level = (band.level + if want { step } else { -step })
        .clamp(0.0, 1.0);
    if !want && band.level <= 0.0 {
        for (strain, _) in &strains {
            commands.entity(strain).try_despawn();
        }
        band.next = None;
        band.making = None;
        band.made = None;
        band.moment = None;
        band.answering = None;
        cues.clear();
        return;
    }
    let dark = *part.get() == DayPart::Night;
    let dim = if dark { set.music_night } else { 1.0 };
    band.dim += (dim - band.dim).clamp(-step, step);
    let volume = Volume::Linear(
        band.level
            * band.dim
            * set.music
            * set.volume
            * set.music_level,
    );
    for (_, sink) in &mut strains {
        if let Some(mut sink) = sink {
            sink.set_volume(volume);
        }
    }
    let mood = Mood {
        part: *part.get(),
        busy: busy(society.as_deref(), &set),
        rest: set.music_rest,
    };
    let mut start = |samples: Vec<f32>| {
        commands.spawn((
            Strain,
            AudioPlayer(clips.add(Clip {
                samples,
                channels: 2,
            })),
            PlaybackSettings::DESPAWN.with_volume(volume),
        ));
    };

    for cue in cues.read() {
        let free = band.moment.is_none()
            && band.answering.is_none()
            && now - band.answered > set.phrase_gap;
        if let Some(moment) = Moment::of(cue.sound).filter(|_| free) {
            band.moment = Some((moment, now + set.phrase_after));
        }
    }
    if let Some((moment, _)) =
        band.moment.filter(|(_, due)| want && now >= *due)
    {
        let phrase = band.composer.answer(moment, &mood);
        band.answering = Some(
            AsyncComputeTaskPool::get()
                .spawn(async move { render(&phrase) }),
        );
        band.moment = None;
        band.answered = now;
    }
    if let Some(task) = &mut band.answering
        && let Some(samples) = block_on(poll_once(task))
    {
        band.answering = None;
        start(samples);
    }

    if !want {
        return;
    }
    let due = *band.next.get_or_insert(now);
    if band.made.is_none()
        && band.making.is_none()
        && now >= due - AHEAD
    {
        let breath = band.composer.breathe(&mood);
        band.making =
            Some(AsyncComputeTaskPool::get().spawn(async move {
                (render(&breath), breath.stride)
            }));
    }
    if let Some(task) = &mut band.making
        && let Some(made) = block_on(poll_once(task))
    {
        band.making = None;
        band.made = Some(made);
    }
    if now >= due
        && let Some((samples, stride)) = band.made.take()
    {
        start(samples);
        band.next = Some(now + stride);
    }
}
