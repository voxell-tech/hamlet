//! Switching a WASM plugin on and off: its component compiled off
//! the frame, then instantiated and asked what it adds.

use std::path::PathBuf;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Has, With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::ecs::world::World;
use bevy::tasks::futures::check_ready;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::time::{Real, Time};
use hamlet_engine_api::Contribution;
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_grants::Admitted;
use hamlet_engine_plugins::{
    ContributedBy, Disable, Enable, Enabled, Entry, PluginInfo,
    switch_on,
};
use hamlet_engine_shell::Command;

use crate::budget::{Spending, pass_logs};
use crate::folder::{Package, WASM, read_capped};
use crate::host::{Host, WasmTuning};
use crate::instance::{Caps, Instance, Linked, link};
use crate::panel::{Inbox, Work, view};
use crate::reload::{Declared, Reloading};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(enable)
        .add_observer(disable)
        .add_systems(Update, (begin, compiled, start).chain());
}

/// On a plugin to be started once its component is built.
#[derive(Component)]
pub(crate) struct Starting;

/// On a plugin whose component is being compiled off the frame.
#[derive(Component)]
pub(crate) struct Compiling(pub(crate) Task<Result<Linked, String>>);

/// On a plugin: its component, compiled and linked.
#[derive(Component, Clone)]
pub(crate) struct Built(pub(crate) Linked);

/// Compiles the `plugin.wasm` at `file`, of at most `most` bytes, off
/// the frame.
pub(crate) fn compile(
    host: &Host,
    file: PathBuf,
    most: u64,
) -> Compiling {
    let engine = host.0.clone();
    Compiling(AsyncComputeTaskPool::get().spawn(async move {
        link(&engine, &read_capped(&file, most)?)
    }))
}

/// Starts a plugin the person let in. The grants ask about one they
/// did not.
fn enable(
    event: On<Enable>,
    plugins: Query<(Has<Enabled>, Has<Admitted>), With<Package>>,
    mut commands: Commands,
) {
    if plugins.get(event.entity) == Ok((false, true)) {
        commands.entity(event.entity).insert(Starting);
    }
}

fn disable(event: On<Disable>, mut commands: Commands) {
    commands.entity(event.entity).try_remove::<(
        Instance,
        Inbox,
        Spending,
        Starting,
        Compiling,
        Declared,
        Reloading,
    )>();
}

/// What `contributions` declare, as the [`Entry`] a built-in plugin
/// fills: the plugin draws its panels and settings pages, and runs
/// its commands.
pub(crate) fn entry(contributions: Vec<Contribution>) -> Entry {
    let mut entry = Entry::default();
    for contribution in contributions {
        match contribution {
            Contribution::Panel { name, title } => {
                entry.panel(&name, &title, view);
            }
            Contribution::Command { name, label } => {
                entry.command(&name, &label, run_command);
            }
            Contribution::MenuEntry { menu, command } => {
                entry.menu_entry(&menu, &command);
            }
            Contribution::Shortcut { keys, command } => {
                entry.shortcut(&keys, &command);
            }
            Contribution::SettingsPage { name, title } => {
                entry.settings_page(&name, &title, view);
            }
            Contribution::Workspace {
                name,
                title,
                layout,
            } => {
                entry.workspace(&name, &title, layout);
            }
        }
    }
    entry
}

/// Puts a WASM plugin's command in its inbox, to be run on the next
/// frame.
fn run_command(world: &mut World, command: Entity) {
    let Some(name) =
        world.get::<Command>(command).and_then(|command| {
            let (_, name) = command.id.as_str().split_once('/')?;
            Some(name.to_owned())
        })
    else {
        return;
    };
    let Some(&ContributedBy(plugin)) =
        world.get::<ContributedBy>(command)
    else {
        return;
    };
    if let Some(mut inbox) = world.get_mut::<Inbox>(plugin) {
        inbox.0.push_back(Work::Run(name));
    }
}

/// Compiles the component of each plugin to be started that has
/// none yet.
fn begin(
    plugins: Query<
        (Entity, &Package),
        (With<Starting>, Without<Built>, Without<Compiling>),
    >,
    host: Option<Res<Host>>,
    tuning: Res<WasmTuning>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    for (plugin, package) in &plugins {
        let file = package.folder.join(WASM);
        commands.entity(plugin).insert(compile(
            &host,
            file,
            tuning.wasm_bytes,
        ));
    }
}

/// Keeps each component compiled, or tells why it could not be.
fn compiled(
    mut plugins: Query<(Entity, &PluginInfo, &mut Compiling)>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    for (plugin, info, mut compiling) in &mut plugins {
        let Some(done) = check_ready(&mut compiling.0) else {
            continue;
        };
        let mut entity = commands.entity(plugin);
        entity.remove::<Compiling>();
        match done {
            Ok(linked) => {
                entity.insert(Built(linked));
            }
            Err(why) => {
                entity.remove::<Starting>();
                log.write(LogLine::error(
                    info.id.as_str(),
                    format!("{} cannot be loaded: {why}", info.name),
                ));
            }
        }
    }
}

/// Instantiates each plugin to be started whose component is built,
/// and adds what it declares.
fn start(
    plugins: Query<(Entity, &PluginInfo, &Built), With<Starting>>,
    host: Option<Res<Host>>,
    budgets: Res<Budgets>,
    tuning: Res<WasmTuning>,
    time: Res<Time<Real>>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    let caps = Caps::of(&budgets, &tuning);
    for (plugin, info, built) in &plugins {
        commands.entity(plugin).remove::<Starting>();
        let mut instance =
            match Instance::new(&host.0, &built.0, &caps) {
                Ok(instance) => instance,
                Err(fault) => {
                    log.write(LogLine::error(
                        info.id.as_str(),
                        format!(
                            "{} cannot start: {fault}",
                            info.name
                        ),
                    ));
                    continue;
                }
            };
        let mut spending = Spending::new(time.elapsed());
        let (declared, spent) = instance.declare(&caps);
        spending.charge(spent, false);
        pass_logs(
            info,
            &mut instance,
            &mut spending,
            &budgets,
            &mut log,
        );
        match declared {
            Ok(contributions) => {
                switch_on(
                    &mut commands,
                    plugin,
                    info,
                    entry(contributions.clone()),
                    &budgets,
                );
                commands.entity(plugin).insert((
                    instance,
                    Inbox::default(),
                    spending,
                    Declared(contributions),
                ));
            }
            Err(fault) => {
                log.write(LogLine::error(
                    info.id.as_str(),
                    format!("{} cannot start: {fault}", info.name),
                ));
            }
        }
    }
}
