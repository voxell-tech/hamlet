//! Bringing native plugins up: the scan at startup, and the plugin
//! the chief has just let in.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use bevy::prelude::*;
use hamlet_base::HomeDir;
use hamlet_sdk::capability::{
    AskToInstall, AskToRemove, Asks, Choices, Granted, Installed,
    Record, Standing, Store, Uninstalled, standing,
};
use hamlet_sdk::plugin::{
    Declared, Enable, Entry, EntryPoint, Exports, FullAccess,
    Imports, Manifest, NotLoaded, PluginInfo, Status, plain,
};

use crate::guard::{Guard, Settling};
use crate::install::{Found, scan};
use crate::{cache, library, marker};

/// On the entity of a native plugin: where its files are.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Library {
    pub folder: PathBuf,
    pub file: PathBuf,
}

/// Present when the last run ended before a library was trusted:
/// the plugins that were not, and no library is opened in this run.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub struct SafeMode(pub Vec<String>);

/// Scans the install folder. A plugin that `grants.toml` records as
/// installed, for the library that is there, is loaded. Any other is
/// held with nothing of it opened, and the chief is asked to install
/// it unless it was declined as it is. It runs after the records are
/// opened.
pub fn installed(world: &mut World) {
    let Some(home) =
        world.get_resource::<HomeDir>().map(|h| h.0.clone())
    else {
        return;
    };
    if !world.contains_resource::<Store>() {
        warn!("native plugins: no records of the chief's answers");
        return;
    }
    let mut found = Vec::new();
    for scanned in scan(&home.plugins()) {
        match scanned {
            Ok(plugin) => found.push(plugin),
            Err(rejected) => {
                warn!("plugins/{}: {}", rejected.folder, rejected.why)
            }
        }
    }
    let (culprits, gone) = marker::pending(&home.root)
        .into_iter()
        .partition::<Vec<_>, _>(|id| {
            found.iter().any(|plugin| plugin.manifest.id == *id)
        });
    // A plugin that is gone cannot be the one that stopped it.
    for id in &gone {
        marker::settled(&home.root, id);
    }
    let safe = (!culprits.is_empty()).then_some(culprits);
    if let Some(ids) = &safe {
        warn!(
            "native plugins: the last run stopped before `{}` was \
             trusted, so no library is opened",
            ids.join("`, `")
        );
        world.insert_resource(SafeMode(ids.clone()));
    }
    for plugin in found {
        let id = plugin.manifest.id.clone();
        let standing = {
            let store = world.resource::<Store>();
            match store.get(&id) {
                Some(record) if record.stale(&plugin.hash) => {
                    Standing::Unknown
                }
                record => standing(
                    record,
                    &plugin.manifest.version,
                    &Asks::default(),
                ),
            }
        };
        let entity = spawn(world, &plugin);
        match standing {
            Standing::Installed { .. } => {
                world.resource_mut::<Store>().set(
                    &id,
                    Record::installed(
                        &plugin.manifest.version,
                        &Choices::default(),
                    )
                    .hashed(Some(&plugin.hash)),
                );
                match &safe {
                    Some(culprits) if culprits.contains(&id) => {
                        not_loaded(world, entity, STOPPED);
                        world.trigger(AskToRemove { plugin: id });
                    }
                    Some(culprits) => not_loaded(
                        world,
                        entity,
                        &format!(
                            "Not loaded: the last run stopped \
                             before `{}` was trusted.",
                            culprits.join("`, `")
                        ),
                    ),
                    None => start(world, entity),
                }
            }
            Standing::Declined => {
                info!("native plugin {id}: declined, not loaded");
                world.entity_mut(entity).insert(Uninstalled);
            }
            Standing::Unknown => {
                info!(
                    "native plugin {id}: not installed, not loaded"
                );
                world.entity_mut(entity).insert(Uninstalled);
                world.trigger(AskToInstall { plugin: id });
            }
        }
    }
    world.flush();
}

/// What the plugin that was loading when the last run stopped is
/// told.
const STOPPED: &str = "Hamlet stopped before this was trusted, so \
                       no native plugin was loaded. Remove it, or \
                       delete the file `loading` in the home to try \
                       again.";

/// Spawns the entity of a native plugin, switched off, with nothing
/// of its library opened.
fn spawn(world: &mut World, plugin: &Found) -> Entity {
    let manifest = Manifest {
        id: plugin.manifest.id.clone(),
        name: plain(&plugin.manifest.name),
        version: plugin.manifest.version.clone(),
        needs: Vec::new(),
        wants: Vec::new(),
        exports: Exports::default(),
        imports: Imports::default(),
        builtin: false,
        essential: false,
    };
    world
        .spawn((
            PluginInfo::from(&manifest),
            manifest,
            Asks::default(),
            Granted::full(),
            Status::Disabled,
            FullAccess(plugin.hash.clone()),
            Library {
                folder: plugin.folder.clone(),
                file: plugin.library.clone(),
            },
        ))
        .id()
}

fn not_loaded(world: &mut World, plugin: Entity, why: &str) {
    warn!("native plugin: {why}");
    world.entity_mut(plugin).insert(NotLoaded(why.into()));
}

/// Opens the library of the plugin `plugin` and runs its entry with
/// the app. What the entry declares is switched on by the plugin
/// frame, as for any plugin.
pub fn start(world: &mut World, plugin: Entity) {
    let (Some(info), Some(file), Some(granted), Some(home)) = (
        world.get::<PluginInfo>(plugin).cloned(),
        world.get::<Library>(plugin).map(|l| l.file.clone()),
        world.get::<FullAccess>(plugin).map(|a| a.0.clone()),
        world.get_resource::<HomeDir>().map(|h| h.0.root.clone()),
    ) else {
        return;
    };
    // Whatever an earlier entry left is not this one's.
    world.remove_resource::<Declared>();
    let copy =
        match cache::private_copy(&home, &info.id, &file, &granted) {
            Ok(copy) => copy,
            Err(why) => {
                world.entity_mut(plugin).insert(Status::Failed);
                not_loaded(
                    world,
                    plugin,
                    &format!("Not loaded: {why}"),
                );
                return;
            }
        };
    if let Err(e) = marker::begin(&home, &info.id) {
        let why = format!(
            "Not loaded: the `loading` file cannot be written: {e}"
        );
        world.entity_mut(plugin).insert(Status::Failed);
        not_loaded(world, plugin, &why);
        return;
    }
    let opened = library::open(&copy)
        .map_err(|refusal| refusal.to_string())
        .and_then(|entry| with_app(world, entry));
    let declared = world.remove_resource::<Declared>();
    match opened {
        Ok(()) => {
            // The marker stays while the library's systems run.
            let frames = world
                .get_resource::<Guard>()
                .cloned()
                .unwrap_or_default()
                .clean_frames;
            if frames == 0 {
                marker::settled(&home, &info.id);
            } else {
                world
                    .resource_mut::<Settling>()
                    .add(&info.id, frames);
            }
            let declared =
                declared.map_or(nothing as fn(&mut Entry), |d| d.0);
            world.entity_mut(plugin).insert(EntryPoint(declared));
            info!("native plugin {}: loaded", info.id);
            world.trigger(Enable(info.id));
            world.flush();
        }
        Err(why) => {
            marker::settled(&home, &info.id);
            world.entity_mut(plugin).insert(Status::Failed);
            not_loaded(world, plugin, &format!("Not loaded: {why}"));
        }
    }
}

fn nothing(_: &mut Entry) {}

/// Runs `run` with an app that holds this world, so that code written
/// for an app can add its systems, observers and resources to a world
/// that is running. A panic in it is caught and the world is put
/// back.
pub fn with_app<R>(
    world: &mut World,
    run: impl FnOnce(&mut App) -> R,
) -> Result<R, String> {
    let mut app = App::empty();
    std::mem::swap(app.world_mut(), world);
    let ran = catch_unwind(AssertUnwindSafe(|| run(&mut app)));
    std::mem::swap(app.world_mut(), world);
    ran.map_err(|_| "its entry panicked".to_string())
}

/// Loads the plugin the chief has just let in, unless this run is in
/// safe mode.
pub fn admit(
    installed: On<Installed>,
    plugins: Query<
        (Entity, &PluginInfo),
        (With<FullAccess>, Without<Uninstalled>, Without<EntryPoint>),
    >,
    safe: Option<Res<SafeMode>>,
    mut commands: Commands,
) {
    for (plugin, info) in &plugins {
        if info.id != installed.plugin {
            continue;
        }
        if safe.is_some() {
            commands.entity(plugin).insert(NotLoaded(
                "Not loaded: native plugins are off after a stop \
                 while loading one. Restart Hamlet."
                    .into(),
            ));
            continue;
        }
        commands.queue(move |world: &mut World| start(world, plugin));
    }
}

#[cfg(test)]
mod tests {
    use hamlet_sdk::capability::Subject;

    use super::*;

    #[derive(Resource, Default)]
    struct Added(u32);

    fn adds_a_resource_and_a_system(app: &mut App) {
        app.init_resource::<Added>()
            .add_systems(Update, |mut added: ResMut<Added>| {
                added.0 += 1
            });
    }

    #[test]
    fn code_written_for_an_app_works_on_a_running_world() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.update();
        let ran =
            with_app(app.world_mut(), adds_a_resource_and_a_system);
        assert!(ran.is_ok());
        app.update();
        assert_eq!(app.world().resource::<Added>().0, 1);
    }

    /// An app over a scratch home that holds the native plugin
    /// `spark`, whose library is not a real one.
    fn home_with_spark(name: &str, record: bool) -> (App, PathBuf) {
        let root = std::env::temp_dir()
            .join(format!("hamlet_{name}_{}", std::process::id()));
        let folder = root.join("plugins/spark");
        std::fs::create_dir_all(&folder).unwrap();
        let library =
            format!("libspark.{}", std::env::consts::DLL_EXTENSION);
        std::fs::write(
            folder.join("manifest.toml"),
            format!(
                "id = \"spark\"\nname = \"Spark\"\nversion = \
                 \"0.1.0\"\nkind = \"native\"\nlibrary = \
                 \"{library}\"\n"
            ),
        )
        .unwrap();
        std::fs::write(folder.join(&library), b"not a library")
            .unwrap();
        if record {
            let hash =
                crate::install::hash(&folder.join(&library)).unwrap();
            Store::open(&root.join("plugins")).set(
                "spark",
                Record::installed("0.1.0", &Choices::default())
                    .hashed(Some(&hash)),
            );
        }
        let mut app = App::new();
        app.insert_resource(HomeDir(hamlet_home::Home {
            root: root.clone(),
            workspace: root.join("workspace"),
        }))
        .add_plugins((
            MinimalPlugins,
            hamlet_sdk::capability::plugin,
            hamlet_sdk::plugin::plugin,
            crate::plugin,
        ));
        (app, root)
    }

    fn spark(app: &mut App) -> (bool, Option<String>, Status) {
        let mut query = app.world_mut().query::<(
            &PluginInfo,
            Has<Uninstalled>,
            Option<&NotLoaded>,
            &Status,
        )>();
        let (_, uninstalled, why, status) = query
            .iter(app.world())
            .find(|(info, ..)| info.id == "spark")
            .unwrap();
        (uninstalled, why.map(|w| w.0.clone()), *status)
    }

    #[test]
    fn after_a_stop_while_loading_no_library_is_opened() {
        let (mut app, root) = home_with_spark("safe", true);
        marker::begin(&root, "spark").unwrap();
        app.update();
        let (uninstalled, why, status) = spark(&mut app);
        assert!(!uninstalled);
        assert_eq!(status, Status::Disabled);
        assert!(why.unwrap().contains("Hamlet stopped"));
        assert_eq!(
            app.world().resource::<SafeMode>(),
            &SafeMode(vec!["spark".into()])
        );
        // The marker stays until the plugin is removed.
        assert_eq!(marker::pending(&root), ["spark"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_library_the_chief_did_not_see_is_asked_about() {
        let (mut app, root) = home_with_spark("changed", false);
        app.update();
        let (uninstalled, why, _) = spark(&mut app);
        assert!(uninstalled);
        assert_eq!(why, None);
        let mut asked = app.world_mut().query::<&Subject>();
        assert!(asked.iter(app.world()).any(|s| matches!(
            s,
            Subject::Install { plugin } if plugin == "spark"
        )));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_panicking_entry_leaves_the_world_whole() {
        let mut world = World::new();
        world.init_resource::<Added>();
        let ran = with_app(&mut world, |_| panic!("a bad entry"));
        assert!(ran.is_err());
        assert!(world.contains_resource::<Added>());
    }
}
