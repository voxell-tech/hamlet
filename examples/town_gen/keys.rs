//! The keys that grow the society and move the view.

use bevy::prelude::*;
use hamlet_world::generator::land::Land;
use hamlet_world::generator::noise::key;
use hamlet_world::generator::society::{RoadLevel, Society};
use hamlet_world::generator::{GenView, Traffic};
use hamlet_world::keeper::KState;
use hamlet_world::keeper::work::{Paper, Task, Work};
use hamlet_world::sky::SkySettings;
use hamlet_world::sound::SoundSettings;
use hamlet_world::{Hour, Sky, Stage};

/// The town the keys act on, and a counter that makes each key press
/// pick something different.
#[derive(Resource, Default)]
pub struct Hand {
    pub town: usize,
    presses: u64,
    /// The task last given with K, as a place in `TASKS`.
    task: usize,
}

/// The tasks K steps through.
const TASKS: [Task; 11] = [
    Task::Think,
    Task::Run,
    Task::Think,
    Task::Write(Paper::File),
    Task::Read(Paper::Notes),
    Task::Fetch,
    Task::Read(Paper::Skill),
    Task::Write(Paper::Skill),
    Task::Write(Paper::Notes),
    Task::CustomTool,
    Task::Reflect,
];

impl Hand {
    /// A number below `n` that changes with every call.
    fn pick(&mut self, n: usize) -> usize {
        self.presses += 1;
        (key(&[self.presses, 5]) % n.max(1) as u64) as usize
    }
}

/// The level above `level`, up to rail.
fn raised(level: RoadLevel) -> RoadLevel {
    match level {
        RoadLevel::Overgrown => RoadLevel::Dirt,
        RoadLevel::Dirt => RoadLevel::Paved,
        RoadLevel::Paved | RoadLevel::Rail => RoadLevel::Rail,
    }
}

pub fn keys(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut land: ResMut<Land>,
    mut world: ResMut<Society>,
    mut traffic: ResMut<Traffic>,
    mut hand: ResMut<Hand>,
    mut stage: ResMut<Stage>,
    mut view: ResMut<GenView>,
    sky: Res<Sky>,
    sky_set: Res<SkySettings>,
    mut hour: ResMut<Hour>,
    mut sound: ResMut<SoundSettings>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(town) = world.towns.get(hand.town).cloned() else {
        return;
    };
    let citizens =
        world.citizens(town.id).map(|a| a.id).collect::<Vec<_>>();
    let others = citizens
        .iter()
        .copied()
        .filter(|&id| id != town.leader)
        .collect::<Vec<_>>();
    let local = world
        .roads
        .iter()
        .filter(|r| {
            citizens.contains(&r.pair.0)
                && citizens.contains(&r.pair.1)
        })
        .cloned()
        .collect::<Vec<_>>();

    if input.just_pressed(KeyCode::KeyA) {
        world.add_agent(&land, town.id);
    }
    if input.just_pressed(KeyCode::KeyX) && !others.is_empty() {
        let id = others[hand.pick(others.len())];
        world.remove_agent(id);
    }
    if input.just_pressed(KeyCode::KeyZ)
        && let Some(back) = world
            .agents
            .iter()
            .rfind(|a| a.town == town.id && a.archived)
            .cloned()
    {
        match back.into {
            Some(kept) => world.split(&land, kept, back.id),
            None => world.restore(&land, back.id),
        };
    }
    if input.just_pressed(KeyCode::KeyR) && citizens.len() > 2 {
        for _ in 0..40 {
            let a = citizens[hand.pick(citizens.len())];
            let b = citizens[hand.pick(citizens.len())];
            if world.add_road(a, b) {
                break;
            }
        }
    }
    if input.just_pressed(KeyCode::KeyU)
        && let Some(r) = local.iter().min_by_key(|r| r.level)
    {
        world.set_level(r.pair, raised(r.level));
    }
    if input.just_pressed(KeyCode::KeyO) && !local.is_empty() {
        let r = &local[hand.pick(local.len())];
        world.set_level(r.pair, RoadLevel::Overgrown);
    }
    if input.just_pressed(KeyCode::KeyF) {
        let id = world.found_town(&mut land);
        hand.town = world.towns.len() - 1;
        view.focus = land.sites[world.town(id).unwrap().site].at;
    }
    if input.just_pressed(KeyCode::KeyM) && others.len() > 1 {
        let kept = others[hand.pick(others.len())];
        let gone = others[hand.pick(others.len())];
        world.merge(kept, gone);
    }
    if input.just_pressed(KeyCode::KeyG) && !local.is_empty() {
        let r = &local[hand.pick(local.len())];
        traffic.send(r.pair, r.pair.0);
    }
    if input.just_pressed(KeyCode::KeyW) && !others.is_empty() {
        let id = others[hand.pick(others.len())];
        let next = match world.agent(id).map(|a| a.state) {
            Some(KState::Rest) => KState::Waiting,
            Some(KState::Waiting) => KState::Fault,
            _ => KState::Rest,
        };
        world.set_state(id, next);
    }
    if input.just_pressed(KeyCode::KeyB) {
        let busy = citizens.iter().any(|&id| {
            world
                .agent(id)
                .is_some_and(|a| a.state == KState::Working)
        });
        let next = if busy { KState::Rest } else { KState::Working };
        for &id in &citizens {
            world.set_state(id, next);
        }
    }
    let (next, fails) = (
        input.just_pressed(KeyCode::KeyK),
        input.just_pressed(KeyCode::KeyJ),
    );
    if next || fails {
        if next {
            hand.task = (hand.task + 1) % TASKS.len();
        }
        for &id in &citizens {
            let Some(was) = world.agent(id).map(|a| a.work) else {
                continue;
            };
            world.set_work(
                id,
                Work {
                    task: TASKS[hand.task],
                    step: was.step + u32::from(next),
                    failures: was.failures + u32::from(fails),
                },
            );
        }
    }
    if input.just_pressed(KeyCode::Tab) && !world.towns.is_empty() {
        hand.town = (hand.town + 1) % world.towns.len();
        view.focus = land.sites[world.towns[hand.town].site].at;
        view.dist = view.dist.min(90.0);
    }
    if input.just_pressed(KeyCode::Digit0) {
        view.focus = Vec2::ZERO;
        view.dist = 620.0;
    }
    if input.just_pressed(KeyCode::KeyT) {
        stage.night = !stage.night;
        hour.0 = None;
    }
    if input.just_pressed(KeyCode::KeyN) {
        sound.muted = !sound.muted;
    }
    if input.just_pressed(KeyCode::KeyY) {
        sound.music_on = !sound.music_on;
    }
    let louder = f32::from(input.pressed(KeyCode::Period))
        - f32::from(input.pressed(KeyCode::Comma));
    if louder != 0.0 {
        sound.volume = (sound.volume + louder * time.delta_secs())
            .clamp(0.0, 1.0);
    }
    let turn = f32::from(input.pressed(KeyCode::BracketRight))
        - f32::from(input.pressed(KeyCode::BracketLeft));
    if turn != 0.0 {
        let to = (sky.hour + turn * 3.0 * time.delta_secs())
            .rem_euclid(24.0);
        hour.0 = Some(to);
        stage.night = sky_set.night(to);
    }
    if input.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }

    let dt = time.delta_secs();
    let mut pan = Vec2::ZERO;
    for (code, dir) in [
        (KeyCode::ArrowLeft, -Vec2::X),
        (KeyCode::ArrowRight, Vec2::X),
        (KeyCode::ArrowUp, -Vec2::Y),
        (KeyCode::ArrowDown, Vec2::Y),
    ] {
        if input.pressed(code) {
            pan += dir;
        }
    }
    if pan != Vec2::ZERO {
        let turned = Vec2::from_angle(-view.yaw).rotate(pan);
        let step = turned * view.dist * 0.6 * dt;
        view.focus += step;
    }
    if input.pressed(KeyCode::Minus) {
        view.dist = (view.dist * (1.0 + 1.2 * dt)).min(900.0);
    }
    if input.pressed(KeyCode::Equal) {
        view.dist = (view.dist * (1.0 - 1.2 * dt)).max(18.0);
    }
    if input.pressed(KeyCode::KeyQ) {
        view.yaw += 0.9 * dt;
    }
    if input.pressed(KeyCode::KeyE) {
        view.yaw -= 0.9 * dt;
    }
}
