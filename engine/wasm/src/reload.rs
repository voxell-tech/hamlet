//! Hot reload. A running plugin whose `plugin.wasm` changed is
//! compiled again off the frame, and swapped in between two frames:
//! the old version saves its state, the new one is instantiated,
//! restores it and says what it adds. A build that fails, or a new
//! version at fault, leaves the old one running and is told in the
//! log. A plugin whose `plugin.toml` says something new is added
//! again, and asked about again where it asks for more.

use std::collections::HashMap;
use std::time::Duration;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::With;
use bevy::ecs::relationship::RelationshipTarget;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::tasks::futures::check_ready;
use bevy::time::common_conditions::on_real_timer;
use hamlet_engine_api::Contribution;
use hamlet_engine_core::tuning::Tuning;
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_plugins::{
    Asked, Contributions, PluginInfo, switch_on,
};
use wasmtime::Engine;

use crate::budget::{Spending, pass_logs};
use crate::folder::{
    Look, MANIFEST, Package, WASM, look, read_manifest, spawn_plugin,
};
use crate::host::{Host, WasmTuning};
use crate::instance::{Caps, Fault, Instance, Linked};
use crate::mail::pass_requests;
use crate::panel::{Drawn, Redraw, stop};
use crate::run::{Built, Compiling, compile, entry};

pub(crate) fn plugin(app: &mut App) {
    let every = Duration::from_secs_f32(WasmTuning::designed().watch);
    app.add_systems(
        Update,
        (notice.run_if(on_real_timer(every)), swap).chain(),
    );
}

/// On an installed plugin: its `plugin.wasm` and `plugin.toml` as
/// they were when it was last read.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub(crate) struct Loaded {
    pub(crate) wasm: Option<Look>,
    pub(crate) manifest: Option<Look>,
}

/// On a running plugin: what it declared it adds.
#[derive(Component, Clone, PartialEq, Debug)]
pub(crate) struct Declared(pub(crate) Vec<Contribution>);

/// On a running plugin: its new version, being compiled off the
/// frame.
#[derive(Component)]
pub(crate) struct Reloading(Compiling);

/// Why a reload failed.
#[derive(Debug, PartialEq)]
pub(crate) enum Failed {
    /// The running version was at fault, saving its state.
    Old(Fault),
    /// The new version was at fault: it is let go, and the old one
    /// runs on.
    New(Fault),
}

/// Looks for changes in each installed plugin's files that stayed
/// the same from one look to the next, so a file still being written
/// is not read. A new `plugin.wasm` of a running plugin is compiled;
/// one of a plugin that is not running is compiled when it starts.
fn notice(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Package,
        &mut Loaded,
        Option<&Instance>,
        Option<&Reloading>,
    )>,
    host: Option<Res<Host>>,
    tuning: Res<WasmTuning>,
    mut last: Local<HashMap<Entity, Loaded>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    last.retain(|plugin, _| plugins.contains(*plugin));
    for (plugin, info, package, mut loaded, instance, reloading) in
        &mut plugins
    {
        let now = Loaded {
            wasm: look(&package.folder.join(WASM)),
            manifest: look(&package.folder.join(MANIFEST)),
        };
        let settled = last.insert(plugin, now) == Some(now);
        if !settled || now == *loaded || reloading.is_some() {
            continue;
        }
        let was = std::mem::replace(&mut *loaded, now);
        if now.manifest != was.manifest {
            match read_manifest(
                &package.folder,
                tuning.manifest_bytes,
            ) {
                Ok(manifest)
                    if manifest.name != info.name
                        || manifest.version != info.version
                        || manifest.asks != info.asks =>
                {
                    log.write(LogLine::info(
                        "plugins",
                        format!(
                            "{} {} is read again",
                            manifest.name, manifest.version
                        ),
                    ));
                    commands.entity(plugin).despawn();
                    spawn_plugin(
                        &mut commands,
                        manifest,
                        package.folder.clone(),
                    );
                    continue;
                }
                Ok(_) => {}
                Err(why) => {
                    log.write(LogLine::error(
                        info.id.as_str(),
                        format!(
                            "{} cannot be read again: {why}",
                            info.name
                        ),
                    ));
                    continue;
                }
            }
        }
        if now.wasm == was.wasm {
            continue;
        }
        if instance.is_none() {
            commands.entity(plugin).remove::<Built>();
            continue;
        }
        let file = package.folder.join(WASM);
        commands.entity(plugin).insert(Reloading(compile(
            &host,
            file,
            tuning.wasm_bytes,
        )));
    }
}

/// Swaps in each new version that was compiled.
fn swap(
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &mut Instance,
        &mut Spending,
        &mut Reloading,
        &Declared,
        Option<&Contributions>,
    )>,
    surfaces: Query<(), With<Drawn>>,
    host: Option<Res<Host>>,
    budgets: Res<Budgets>,
    tuning: Res<WasmTuning>,
    mut log: MessageWriter<LogLine>,
    mut asked: MessageWriter<Asked>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    let caps = Caps::of(&budgets, &tuning);
    for (
        plugin,
        info,
        mut instance,
        mut spending,
        mut reloading,
        declared,
        contributions,
    ) in &mut plugins
    {
        let Some(built) = check_ready(&mut reloading.0.0) else {
            continue;
        };
        commands.entity(plugin).remove::<Reloading>();
        let source = info.id.as_str();
        let linked = match built {
            Ok(linked) => linked,
            Err(why) => {
                log.write(LogLine::error(
                    source,
                    format!(
                        "{}'s new build cannot be loaded: {why}; \
                         the old one runs on",
                        info.name
                    ),
                ));
                continue;
            }
        };
        let reloaded = reload(
            &mut instance,
            &host.0,
            &linked,
            &caps,
            &mut spending,
        );
        pass_logs(
            info,
            &mut instance,
            &mut spending,
            &budgets,
            &mut log,
        );
        pass_requests(plugin, &mut instance, 0, &mut asked);
        match reloaded {
            Ok((mut fresh, contributions_now)) => {
                pass_logs(
                    info,
                    &mut fresh,
                    &mut spending,
                    &budgets,
                    &mut log,
                );
                pass_requests(plugin, &mut fresh, 0, &mut asked);
                if contributions_now == declared.0 {
                    for surface in contributions
                        .into_iter()
                        .flat_map(|c| c.iter())
                    {
                        if surfaces.contains(surface) {
                            commands.entity(surface).insert(Redraw);
                        }
                    }
                } else {
                    commands
                        .entity(plugin)
                        .despawn_related::<Contributions>();
                    switch_on(
                        &mut commands,
                        plugin,
                        info,
                        entry(contributions_now.clone()),
                        &budgets,
                    );
                }
                commands.entity(plugin).insert((
                    fresh,
                    Built(linked),
                    Declared(contributions_now),
                ));
                log.write(LogLine::info(
                    source,
                    format!("{} is reloaded", info.name),
                ));
            }
            Err(Failed::New(fault)) => {
                log.write(LogLine::error(
                    source,
                    format!(
                        "{}'s new build cannot start: {fault}; the \
                         old one runs on",
                        info.name
                    ),
                ));
            }
            Err(Failed::Old(fault)) => {
                stop(&mut commands, &mut log, plugin, info, &fault)
            }
        }
    }
}

/// The plugin running in `old` moved to its new version `linked`: the
/// old version's state is saved, and the new one instantiated, given
/// the state and asked what it adds.
pub(crate) fn reload(
    old: &mut Instance,
    engine: &Engine,
    linked: &Linked,
    caps: &Caps,
    spending: &mut Spending,
) -> Result<(Instance, Vec<Contribution>), Failed> {
    let (state, spent) = old.save(caps);
    spending.charge(spent, false);
    let state = state.map_err(Failed::Old)?;
    let mut fresh =
        Instance::new(engine, linked, caps).map_err(Failed::New)?;
    fresh.follow(old);
    let (restored, spent) = fresh.restore(caps, &state);
    spending.charge(spent, false);
    restored.map_err(Failed::New)?;
    let (declared, spent) = fresh.declare(caps);
    spending.charge(spent, false);
    Ok((fresh, declared.map_err(Failed::New)?))
}
