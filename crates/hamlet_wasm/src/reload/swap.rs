//! Building the new version of a plugin whose files changed, and
//! putting it in the place of the running one when it starts.

mod code;
mod data;

use std::path::Path;

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{
    AskForMore, Capability, Choices, Record, Standing, Store,
    Uninstalled, standing,
};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    self as sdk, Command, Contributions, PluginInfo, Status,
};
use hamlet_sdk::reload::Reload;

use super::{Modified, Waiting, Watch, keep};
use crate::command;
use crate::engine::Host;
use crate::install::{self, Body, Installed};
use crate::instance::Instance;
use crate::limits::Limits;
use crate::start::{Setting, Source};

/// What the files of a plugin that changed say.
enum Next {
    /// The new version asks for what the chief has not answered.
    Ask {
        installed: Installed,
        fresh: Vec<Capability>,
    },
    /// The new version may be built, with these choices.
    Ready {
        installed: Installed,
        choices: Choices,
    },
}

/// The plugin being reloaded.
struct Case<'a> {
    plugin: Entity,
    info: &'a PluginInfo,
    folder: &'a Path,
    status: Status,
    /// The state the running version holds, which the new one gets.
    state: &'a [u8],
    /// The choices the new version is built with.
    choices: Choices,
    /// The panels the running version has.
    held: Vec<(Entity, &'a Panel)>,
    /// The commands the running version has.
    offered: Vec<Entity>,
}

/// Builds the new version of each plugin whose files changed. It
/// takes the place of the running one when it starts, and where it
/// does not, the running one stays and the plugin says why. A version
/// that asks for more waits for the chief's answer first.
pub fn reload(
    mut modified: MessageReader<Modified>,
    plugins: Query<
        (
            &PluginInfo,
            &Watch,
            &Status,
            &Choices,
            Option<&Source>,
            Option<&Contributions>,
            Option<&Instance>,
        ),
        Without<Uninstalled>,
    >,
    panels: Query<(Entity, &Panel)>,
    offered: Query<(), With<Command>>,
    host: Option<Res<Host>>,
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    store: Option<ResMut<Store>>,
    mut commands: Commands,
) {
    let Some(mut store) = store else {
        return;
    };
    let setting = host.as_deref().map(|host| Setting {
        host,
        limits: &limits,
        agents: &agents,
        papers: &papers,
    });
    for Modified(plugin) in modified.read() {
        let Ok((
            info,
            watch,
            status,
            choices,
            source,
            held,
            instance,
        )) = plugins.get(*plugin)
        else {
            continue;
        };
        let state = instance.map_or(&[][..], Instance::state);
        commands.entity(*plugin).remove::<Waiting>();
        let folder = watch.folder.as_path();
        let next =
            next(info, folder, &store, choices, source.is_some());
        let (installed, kept) = match next {
            Ok(Next::Ask { installed, fresh }) => {
                let case = case(
                    *plugin, info, folder, *status, choices, held,
                    state, &panels, &offered,
                );
                ask(&case, &installed, fresh, &mut commands);
                continue;
            }
            Ok(Next::Ready { installed, choices }) => {
                (installed, choices)
            }
            Err(why) => {
                failed(&mut commands, *plugin, info, folder, why);
                continue;
            }
        };
        let case = case(
            *plugin, info, folder, *status, &kept, held, state,
            &panels, &offered,
        );
        let version = installed.manifest.version.clone();
        let done = match &installed.body {
            Body::Wasm(file) => code::swap(
                &case,
                &installed,
                file,
                setting.as_ref(),
                &mut commands,
            ),
            Body::Data(file) => {
                data::swap(&case, &installed, file, &mut commands)
            }
        };
        match done {
            Ok(()) => {
                store.set(
                    &info.id,
                    Record::installed(&version, &case.choices),
                );
                info!("plugin {}: reloaded {version}", info.id);
            }
            Err(why) => {
                failed(&mut commands, *plugin, info, folder, why)
            }
        }
    }
}

fn case<'a>(
    plugin: Entity,
    info: &'a PluginInfo,
    folder: &'a Path,
    status: Status,
    choices: &Choices,
    held: Option<&Contributions>,
    state: &'a [u8],
    panels: &'a Query<(Entity, &Panel)>,
    offered: &Query<(), With<Command>>,
) -> Case<'a> {
    let held_by = || held.into_iter().flat_map(|held| held.iter());
    Case {
        plugin,
        info,
        folder,
        status,
        state,
        choices: choices.clone(),
        held: panels.iter_many(held_by()).collect(),
        offered: command::held(held_by(), offered),
    }
}

/// Reads the folder again, and says what to do with what is there.
fn next(
    info: &PluginInfo,
    folder: &Path,
    store: &Store,
    current: &Choices,
    is_code: bool,
) -> Result<Next, String> {
    let installed = install::read(folder)
        .map_err(|rejected| rejected.to_string())?;
    if matches!(installed.body, Body::Wasm(_)) != is_code {
        return Err("it changed between code and a panel; restart \
                    Hamlet to load it"
            .into());
    }
    match standing(
        store.get(&info.id),
        &installed.manifest.version,
        &installed.asks,
    ) {
        Standing::Installed { fresh, .. } if !fresh.is_empty() => {
            Ok(Next::Ask { installed, fresh })
        }
        Standing::Installed { .. } => {
            let choices = current
                .clone()
                .within(&installed.asks.capabilities());
            Ok(Next::Ready { installed, choices })
        }
        _ => Err("it is not installed".into()),
    }
}

/// Puts what the new manifest says on the plugin's entity.
fn renew(
    commands: &mut Commands,
    plugin: Entity,
    installed: &Installed,
    choices: Choices,
) {
    let manifest = sdk::Manifest::from(&installed.manifest);
    commands.entity(plugin).insert((
        PluginInfo::from(&manifest),
        manifest,
        installed.asks.clone(),
        choices,
    ));
}

/// Holds the new version, which stays unbuilt, and asks the chief
/// about what it asks for.
fn ask(
    case: &Case,
    installed: &Installed,
    fresh: Vec<Capability>,
    commands: &mut Commands,
) {
    info!("plugin {}: the new version asks for more", case.info.id);
    renew(commands, case.plugin, installed, case.choices.clone());
    commands.entity(case.plugin).insert((
        Waiting(fresh.clone()),
        Reload {
            note: Some(
                "The new version asks for more. It loads when you \
                 answer."
                    .into(),
            ),
            rollback: keep::differs(case.folder),
        },
    ));
    commands.trigger(AskForMore {
        plugin: case.info.id.clone(),
        capabilities: fresh,
    });
}

/// Keeps the running version and says why the new one did not start.
fn failed(
    commands: &mut Commands,
    plugin: Entity,
    info: &PluginInfo,
    folder: &Path,
    why: String,
) {
    warn!("plugin {}: reload failed: {why}", info.id);
    commands.entity(plugin).insert(keep::trouble(folder, why));
}
