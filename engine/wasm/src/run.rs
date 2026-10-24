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
use hamlet_engine_core::{ActiveLimits, Guards, Home, LogLine};
use hamlet_engine_grants::Admitted;
use hamlet_engine_plugins::{
    Asked, Blocked, ContributedBy, Disable, Enable, Enabled, Entry,
    PluginCommandsExt, PluginInfo, StartOrder, Starting, resolve,
};
use hamlet_engine_shell::Command;
use hamlet_engine_shell::icons::Glyph;

use crate::budget::Spending;
use crate::compiled::{self, WasmHomeExt};
use crate::folder::{Package, WASM};
use crate::host::{Host, WasmTuning};
use crate::instance::{
    Caps, Fault, Instance, Linked, Spent, link_compiled,
};
use crate::panel::{Inbox, Moving, Work};
use crate::reload::{Declared, Reloading};
use crate::{PluginPathExt, retained};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(enable).add_observer(disable).add_systems(
        Update,
        (begin, compiled, instantiate, start).chain().after(resolve),
    );
}

/// On a plugin whose component is being compiled off the frame.
#[derive(Component)]
pub struct Compiling(pub(crate) Task<Result<Linked, String>>);

/// On a plugin whose component is being instantiated and asked what
/// it adds, off the frame.
#[derive(Component)]
pub(crate) struct Instantiating(
    Task<
        Result<
            (Instance, Result<Vec<Contribution>, Fault>, Spent),
            Fault,
        >,
    >,
);

/// On a plugin: its component, compiled and linked.
#[derive(Component, Clone)]
pub(crate) struct Built(pub(crate) Linked);

impl Host {
    /// Compiles the `plugin.wasm` at `file`, of at most `most` bytes,
    /// off the frame, or takes it as kept compiled in `kept`.
    pub(crate) fn compile(
        &self,
        file: PathBuf,
        most: u64,
        kept: PathBuf,
    ) -> Compiling {
        let engine = self.0.clone();
        let shipped = file.with_file_name(compiled::SHIPPED);
        Compiling(AsyncComputeTaskPool::get().spawn(async move {
            let bytes = file.read_capped(most)?;
            link_compiled(
                &engine,
                &compiled::component(
                    &engine, &bytes, &kept, &shipped,
                )?,
            )
        }))
    }
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
        Instantiating,
        Declared,
        Reloading,
        Moving,
    )>();
}

/// What `contributions` declare, as the [`Entry`] a built-in plugin
/// fills: the plugin builds its panels and settings pages, and runs
/// its commands.
pub(crate) fn entry(contributions: Vec<Contribution>) -> Entry {
    let mut entry = Entry::default();
    for contribution in contributions {
        match contribution {
            Contribution::Panel {
                name,
                title,
                icon,
                padding,
            } => {
                let panel =
                    match icon.as_deref().and_then(Glyph::named) {
                        Some(icon) => entry.panel_with_icon(
                            &name,
                            &title,
                            icon,
                            retained::view,
                        ),
                        None => {
                            entry.panel(&name, &title, retained::view)
                        }
                    };
                if let Some(padding) = padding {
                    panel.padding(padding);
                }
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
            Contribution::SettingsPage {
                name,
                title,
                about,
                icon,
            } => {
                let icon =
                    Glyph::named(&icon).unwrap_or(Glyph::Sliders);
                entry.settings_page(
                    &name,
                    &title,
                    &about,
                    icon,
                    retained::page,
                );
            }
            Contribution::SettingsFooter { name } => {
                entry.settings_footer(&name, retained::page);
            }
            Contribution::Workspace {
                name,
                title,
                layout,
            } => {
                entry.workspace(&name, &title, layout);
            }
            Contribution::StatusItem { name, grow } => {
                entry.status_item(&name, grow);
            }
            Contribution::Overlay { name } => {
                entry.overlay(&name, retained::page);
            }
            Contribution::PanelSection { panel, name, title } => {
                entry.panel_section(
                    &panel,
                    &name,
                    &title,
                    retained::page,
                );
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
        (Entity, &PluginInfo, &Package),
        (With<Starting>, Without<Built>, Without<Compiling>),
    >,
    host: Option<Res<Host>>,
    home: Res<Home>,
    tuning: Res<WasmTuning>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    for (plugin, info, package) in &plugins {
        let file = package.folder.join(WASM);
        commands.entity(plugin).insert(host.compile(
            file,
            tuning.wasm_bytes,
            home.kept_compiled(info),
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

/// Instantiates each plugin whose component is built, off the frame,
/// and asks it what it adds. Plugins do this at once, however many
/// there are; [`start`] takes what they declared in the start order.
fn instantiate(
    plugins: Query<
        (Entity, &ActiveLimits, &Built),
        (With<Starting>, Without<Blocked>, Without<Instantiating>),
    >,
    host: Option<Res<Host>>,
    guards: Res<Guards>,
    tuning: Res<WasmTuning>,
    mut commands: Commands,
) {
    let Some(host) = host else {
        return;
    };
    for (plugin, limits, built) in &plugins {
        let caps = Caps::of(&limits.0, &guards, &tuning);
        let engine = host.0.clone();
        let linked = built.0.clone();
        commands.entity(plugin).insert(Instantiating(
            AsyncComputeTaskPool::get().spawn(async move {
                let mut instance =
                    Instance::new(&engine, &linked, &caps)?;
                let (declared, spent) = instance.declare(&caps);
                Ok((instance, declared, spent))
            }),
        ));
    }
}

/// Adds what each plugin declared, once it is instantiated and every
/// plugin before it in the start order is not on its way any more,
/// so what plugins add comes in that order.
fn start(
    mut plugins: Query<(&PluginInfo, &mut Instantiating)>,
    waiting: Query<(), (With<Starting>, Without<Blocked>)>,
    order: Res<StartOrder>,
    time: Res<Time<Real>>,
    mut log: MessageWriter<LogLine>,
    mut asked: MessageWriter<Asked>,
    mut commands: Commands,
) {
    let ready = order.ready(
        |plugin| waiting.contains(plugin),
        |plugin| {
            plugins
                .as_readonly()
                .get(plugin)
                .is_ok_and(|(_, made)| made.0.is_finished())
        },
    );
    for plugin in ready {
        let Ok((info, mut instantiating)) = plugins.get_mut(plugin)
        else {
            continue;
        };
        let Some(made) = check_ready(&mut instantiating.0) else {
            continue;
        };
        commands
            .entity(plugin)
            .remove::<(Starting, Instantiating)>();
        let (mut instance, declared, spent) = match made {
            Ok(made) => made,
            Err(fault) => {
                log.write(LogLine::error(
                    info.id.as_str(),
                    format!("{} cannot start: {fault}", info.name),
                ));
                continue;
            }
        };
        let mut spending = Spending::new(time.elapsed());
        instance.pass_requests(plugin, 0, &mut asked);
        spending.charge(spent);
        instance.pass_logs(info, &mut log);
        match declared {
            Ok(contributions) => {
                commands.switch_on(
                    plugin,
                    info,
                    &order.rank(plugin, info),
                    entry(contributions.clone()),
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
