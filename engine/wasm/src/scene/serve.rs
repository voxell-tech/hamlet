//! Serving a plugin's scenes: its render capabilities kept in step
//! with the grants, what the person did in its views handed to it,
//! its ticks while it animates, and its changes made in the world.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::query::{Added, Changed, With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::time::Time;
use hamlet_engine_api::capability::GPU;
use hamlet_engine_core::{ActiveLimits, Guards, LogLine};
use hamlet_engine_grants::{Decided, FirstUse, Grants, Verdict};
use hamlet_engine_plugins::{Asked, PluginInfo};
use hamlet_engine_render::{
    GpuInfo, GpuLost, SceneEvent, SceneInput, SceneWorldExt, Scenes,
    Viewport,
};
use hamlet_engine_shell::Backdrop;

use crate::WasmCommandsExt;
use crate::budget::Spending;
use crate::host::WasmTuning;
use crate::instance::{Caps, Fault, Instance, Spent};
use crate::panel::{Moving, serve};
use crate::retained::{Holders, NodeOf};
use crate::scene::{Powers, convert};

pub(crate) fn plugin(app: &mut App) {
    // The render crate's messages, for an engine that runs without
    // it.
    app.add_message::<GpuLost>()
        .add_message::<SceneInput>()
        .add_systems(Update, (lose, grant, route).before(serve))
        .add_systems(
            Update,
            (play, apply, attach, forget).chain().after(serve),
        );
}

/// On a running plugin: what the person did in its scenes' views,
/// oldest first.
#[derive(Component, Default)]
pub struct SceneInbox(pub VecDeque<SceneEvent>);

/// On the holder of a frame: the number of the plugin's camera it
/// shows.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub struct ShowsCamera(pub u32);

/// Gives each running plugin the render capabilities the person
/// allows, asks the person about those it tried while they were
/// answered Ask next time, and tells it whether another draws behind
/// the dock.
fn grant(
    mut plugins: Query<(Entity, &PluginInfo, &mut Instance)>,
    grants: Res<Grants>,
    backdrop: Option<Res<Backdrop>>,
    gpu: Option<Res<GpuInfo>>,
    mut decided: MessageReader<Decided>,
    mut first_use: MessageWriter<FirstUse>,
    mut given: Local<HashMap<Entity, Powers>>,
    mut asked: Local<HashMap<Entity, Powers>>,
) {
    for decision in decided.read() {
        let Some(&(power, _)) =
            Powers::ALL.iter().find(|(_, capability)| {
                *capability == decision.capability
            })
        else {
            continue;
        };
        let was = asked.entry(decision.plugin).or_default();
        *was = was.without(power);
        if decision.allowed {
            let now = given.entry(decision.plugin).or_default();
            *now = now.with(power);
        }
    }
    given.retain(|plugin, _| plugins.contains(*plugin));
    asked.retain(|plugin, _| plugins.contains(*plugin));
    // Without the render crate nothing is drawn, so no power is
    // given.
    let Some(gpu) = gpu else {
        return;
    };
    for (plugin, info, mut instance) in &mut plugins {
        let answered =
            given.get(&plugin).copied().unwrap_or_default();
        let petitioned = asked.entry(plugin).or_default();
        let scene = instance.scene();
        let (mut powers, mut asking) =
            (Powers::default(), Powers::default());
        for (power, capability) in Powers::ALL {
            match grants.verdict(info, capability, GPU) {
                Verdict::Allowed => powers = powers.with(power),
                Verdict::Asking if answered.has(power) => {
                    powers = powers.with(power);
                }
                Verdict::Asking => asking = asking.with(power),
                Verdict::Refused => {}
            }
            if scene.wanted.has(power) && !petitioned.has(power) {
                *petitioned = petitioned.with(power);
                first_use.write(FirstUse {
                    plugin,
                    capability,
                    target: GPU.into(),
                });
            }
        }
        scene.powers = powers;
        scene.asking = asking;
        scene.wanted = Powers::default();
        scene.backdrop_taken = backdrop
            .as_ref()
            .and_then(|backdrop| backdrop.0)
            .is_some_and(|owner| owner != plugin);
        scene.gpu = *gpu;
    }
}

/// Stops the plugins whose GPU work was on a device that failed.
fn lose(
    mut lost: MessageReader<GpuLost>,
    plugins: Query<&PluginInfo>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    for lost in lost.read() {
        for &plugin in &lost.owners {
            if let Ok(info) = plugins.get(plugin) {
                let fault = Fault::Trap(format!(
                    "the GPU failed while it drew: {}",
                    lost.why
                ));
                commands.stop_plugin(&mut log, plugin, info, &fault);
            }
        }
    }
}

/// Puts what the person did in a scene's views in its owner's inbox.
fn route(
    mut inputs: MessageReader<SceneInput>,
    mut plugins: Query<Option<&mut SceneInbox>, With<Instance>>,
    mut commands: Commands,
) {
    // The inboxes of plugins told of something for the first time.
    let mut first = HashMap::<Entity, VecDeque<SceneEvent>>::new();
    for input in inputs.read() {
        let Ok(inbox) = plugins.get_mut(input.owner) else {
            continue;
        };
        let events = match inbox {
            Some(inbox) => &mut inbox.into_inner().0,
            None => first.entry(input.owner).or_default(),
        };
        events.push_back(input.event.clone());
    }
    for (plugin, events) in first {
        commands.entity(plugin).insert(SceneInbox(events));
    }
}

/// Hands each plugin what the person did in its views, and a tick
/// while it animates. Only a plugin past the CPU time a second the
/// person set waits. A plugin at fault is stopped.
fn play(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &ActiveLimits,
        &mut Instance,
        &mut Spending,
        Option<&mut SceneInbox>,
    )>,
    guards: Res<Guards>,
    tuning: Res<WasmTuning>,
    time: Res<Time>,
    mut log: MessageWriter<LogLine>,
    mut asked: MessageWriter<Asked>,
    mut commands: Commands,
) {
    let micros = u32::try_from(time.delta().as_micros()).unwrap_or(0);
    for (plugin, info, limits, mut instance, mut spending, inbox) in
        &mut plugins
    {
        let caps = Caps::of(&limits.0, &guards, &tuning);
        let mut calls = Vec::new();
        if let Some(mut inbox) = inbox
            && !inbox.0.is_empty()
            && spending.may_call(limits)
        {
            let inputs = inbox
                .0
                .drain(..)
                .map(convert::input)
                .collect::<Vec<_>>();
            calls.push(instance.scene_input(&caps, &inputs));
        }
        if calls.iter().all(|(done, _)| done.is_ok())
            && instance.scene().animate
            && spending.may_call(limits)
        {
            calls.push(instance.scene_tick(&caps, micros));
        }
        for (done, spent) in calls {
            instance.settle(
                &mut commands,
                plugin,
                &mut spending,
                spent,
                &mut asked,
            );
            instance.pass_logs(info, &mut log);
            if let Err(fault) = done {
                commands.stop_plugin(&mut log, plugin, info, &fault);
                break;
            }
        }
    }
}

impl Instance {
    /// Passes on what a call of its scenes changed and asked, and
    /// charges what it spent.
    fn settle(
        &mut self,
        commands: &mut Commands,
        plugin: Entity,
        spending: &mut Spending,
        spent: Spent,
        asked: &mut MessageWriter<Asked>,
    ) {
        let changes = self.take_node_changes();
        if !changes.is_empty() {
            commands.apply_node_changes(plugin, None, changes);
            // Its next frame says whether its views still move.
            commands.entity(plugin).insert(Moving);
        }
        commands.apply_surface_changes(
            plugin,
            self.take_surface_changes(),
        );
        self.pass_requests(plugin, 0, asked);
        spending.charge(spent);
    }
}

/// Makes what each plugin changed in its scenes, and shows its
/// cameras in the frames it named. A plugin numbering anew, as after
/// a reload, has its scene made afresh.
pub(crate) fn apply(
    mut plugins: Query<(Entity, &mut Instance)>,
    mut applied: Local<HashMap<Entity, Instant>>,
    mut commands: Commands,
) {
    applied.retain(|plugin, _| plugins.contains(*plugin));
    for (plugin, mut instance) in &mut plugins {
        let scene = instance.scene();
        let since = scene.since;
        let changes = std::mem::take(&mut scene.changes);
        let shown = std::mem::take(&mut scene.shown);
        if !changes.is_empty() || applied.get(&plugin) != Some(&since)
        {
            applied.insert(plugin, since);
            commands.queue(move |world: &mut World| {
                world.apply_scene(plugin, since, changes);
            });
        }
        for (camera, node) in shown {
            commands.queue(move |world: &mut World| {
                let Some(holder) = world
                    .get::<Holders>(plugin)
                    .and_then(|holders| holders.0.get(&node))
                    .map(|holder| holder.entity)
                else {
                    return;
                };
                if let Ok(mut holder) = world.get_entity_mut(holder) {
                    holder.insert(ShowsCamera(camera));
                }
            });
        }
    }
}

/// Has the widget of each frame that shows a camera show its image.
fn attach(
    widgets: Query<(Entity, &NodeOf, Option<&Viewport>)>,
    built: Query<(), Added<NodeOf>>,
    shows: Query<&ShowsCamera>,
    shown: Query<(), Changed<ShowsCamera>>,
    scenes: Query<&Scenes>,
    mut commands: Commands,
) {
    if built.is_empty() && shown.is_empty() {
        return;
    }
    for (widget, of, viewport) in &widgets {
        let Ok(&ShowsCamera(id)) = shows.get(of.holder) else {
            continue;
        };
        let Some(root) =
            scenes.get(of.plugin).ok().and_then(Scenes::root)
        else {
            continue;
        };
        let plugin = of.plugin;
        let had = viewport.map(|viewport| viewport.camera);
        commands.queue(move |world: &mut World| {
            let camera = world
                .get::<hamlet_engine_render::Scene>(root)
                .and_then(|scene| scene.camera(id));
            if let Some(camera) = camera
                && had != Some(camera)
                && world.scene_of(plugin).is_some()
                && let Ok(mut widget) = world.get_entity_mut(widget)
            {
                widget.insert(Viewport { camera });
            }
        });
    }
}

/// Takes away the scenes of plugins that no longer run.
fn forget(
    plugins: Query<Entity, (With<Scenes>, Without<Instance>)>,
    mut commands: Commands,
) {
    for plugin in &plugins {
        commands.queue(move |world: &mut World| {
            world.clear_scene(plugin)
        });
    }
}
