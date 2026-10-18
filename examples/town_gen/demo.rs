//! A town at work with nobody at the keys.

use bevy::prelude::*;
use hamlet_realm::generator::Traffic;
use hamlet_realm::generator::society::Society;
use hamlet_realm::keeper::KState;
use hamlet_realm::sound::{Cue, Hearing, Sound};

/// With TOWN_DEMO set to a number of seconds: a town at work with
/// nobody at the keys, and what is heard printed each second.
#[derive(Resource)]
pub struct Demo {
    until: f32,
    /// The whole seconds reported so far.
    told: u32,
    cues: Vec<(Sound, u32)>,
}

impl Demo {
    /// A demo that ends after `until` seconds.
    pub fn until(until: f32) -> Self {
        Demo {
            until,
            told: 0,
            cues: Vec::new(),
        }
    }
}

pub fn demo(
    time: Res<Time>,
    mut demo: ResMut<Demo>,
    mut world: ResMut<Society>,
    mut traffic: ResMut<Traffic>,
    mut cues: MessageReader<Cue>,
    hearing: Option<Res<State<Hearing>>>,
    voices: Query<(), With<SpatialAudioSink>>,
    beds: Query<&AudioSink>,
    mut exit: MessageWriter<AppExit>,
) {
    for cue in cues.read() {
        match demo.cues.iter_mut().find(|(s, _)| *s == cue.sound) {
            Some((_, n)) => *n += 1,
            None => demo.cues.push((cue.sound, 1)),
        }
    }
    let now = time.elapsed_secs();
    if (now as u32) <= demo.told {
        return;
    }
    demo.told = now as u32;
    let Some(town) = world.towns.first().cloned() else {
        return;
    };
    let ids =
        world.citizens(town.id).map(|a| a.id).collect::<Vec<_>>();
    match demo.told {
        2 => {
            for &id in &ids {
                world.set_state(id, KState::Working);
            }
        }
        9 => world.set_state(ids[ids.len() / 2], KState::Waiting),
        15 => world.set_state(ids[ids.len() / 3], KState::Fault),
        _ => {}
    }
    if demo.told % 5 == 3
        && let Some(road) = world
            .roads
            .get(demo.told as usize / 5 % world.roads.len())
    {
        traffic.send(road.pair, road.pair.0);
    }
    let heard = demo
        .cues
        .drain(..)
        .map(|(sound, n)| format!("{} {n}", sound.name()))
        .collect::<Vec<_>>()
        .join(", ");
    let loops = beds
        .iter()
        .map(|sink| format!("{:.2}", sink.volume().to_linear()))
        .collect::<Vec<_>>()
        .join(" ");
    println!(
        "demo: {:>3} s  hearing {:?}  voices {}  loops at [{loops}]  \
         cues: {heard}",
        demo.told,
        hearing.map(|h| *h.get()),
        voices.iter().count(),
    );
    if now > demo.until {
        exit.write(AppExit::Success);
    }
}
