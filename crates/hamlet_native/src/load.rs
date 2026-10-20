//! Bringing native plugins up: the libraries the chief let in, opened
//! and run on the app while it is built, the scan at startup, and the
//! plugin the chief lets in while the app runs.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_base::HomeDir;
use hamlet_sdk::capability::{
    Appeared, AskToInstall, AskToRemove, Asks, Choices, Granted,
    Installed, Record, Standing, Store, Subject, Uninstalled,
    standing,
};
use hamlet_sdk::command::Notice;
use hamlet_sdk::plugin::{
    Declared, Enable, Entry, EntryPoint, Exports, FullAccess,
    Imports, Manifest, NotLoaded, PluginInfo, Status, is_native,
    plain,
};
use hamlet_sdk::reload::Reload;

use crate::guard::{Guard, Settling};
use crate::install::{Found, Install, manifest_text, read, scan};
use crate::library::EntryFn;
use crate::{cache, library, marker};

/// On the entity of a native plugin: where its files are.
#[derive(Component, Clone, PartialEq, Debug)]
#[component(immutable)]
pub struct Library {
    pub folder: PathBuf,
    pub file: PathBuf,
}

/// Present when no library is opened in this run.
#[derive(Resource, Clone, PartialEq, Eq, Debug)]
pub enum SafeMode {
    /// Hamlet was started with `--safe-mode`.
    Asked,
    /// The last run ended before these plugins were trusted.
    Stopped(Vec<String>),
    /// This many runs in a row opened a library and did not exit by
    /// themselves.
    Crashed(u32),
}

/// Present once this run is counted as unclean until it exits.
#[derive(Resource)]
struct Counted;

/// The plugins found when the app was built, for the scan at
/// startup.
#[derive(Resource)]
struct Scanned(Vec<Found>);

/// What the entry of each library opened while the app was built
/// gave: what it declared, or why it is not loaded.
#[derive(Resource, Default)]
struct Opened(Vec<(String, Outcome)>);

type Outcome = Result<fn(&mut Entry), Failure>;

/// Why a library is not loaded.
#[derive(Debug)]
enum Failure {
    /// Turned down before its entry ran, so nothing of it runs.
    Refused(String),
    /// Its entry panicked, and what it added before that stays.
    Panicked,
    /// Its entry, run while the app runs, added to these schedules
    /// while they ran, which lost it. The rest of what it added
    /// stays.
    Lost(Vec<String>),
}

/// The plugins let in while the app runs, loaded at the start of the
/// next frame.
#[derive(Resource, Default)]
pub struct Late(pub Vec<Entity>);

impl Opened {
    fn take(&mut self, id: &str) -> Option<Outcome> {
        let at =
            self.0.iter().position(|(opened, _)| opened == id)?;
        Some(self.0.swap_remove(at).1)
    }
}

/// Opens the libraries that `grants.toml` records as installed, for
/// the library that is there, and runs their entries on `app` while
/// it is built, so that what they add has the app's plugins, its
/// sub-apps and the steps that finish it. It is called last in the
/// plugin, which is added after every other.
pub fn granted(app: &mut App) {
    let Some((root, plugins)) = app
        .world()
        .get_resource::<HomeDir>()
        .map(|h| (h.0.root.clone(), h.0.plugins()))
    else {
        return;
    };
    let found = prepare(app.world_mut(), &root, &plugins);
    let mut opened = Opened::default();
    if !app.world().contains_resource::<SafeMode>() {
        let store = Store::open(&plugins);
        for plugin in &found {
            if !matches!(
                standing_of(&store, plugin),
                Standing::Installed { .. }
            ) {
                continue;
            }
            let id = &plugin.manifest.id;
            count(app.world_mut(), &root);
            let outcome =
                open(&root, id, &plugin.library, &plugin.hash)
                    .map_err(Failure::Refused)
                    .and_then(|entry| {
                        let ran = run_on(app, entry);
                        settle(app.world_mut(), &root, id, ran)
                    });
            opened.0.push((id.clone(), outcome));
        }
    }
    app.insert_resource(Scanned(found)).insert_resource(opened);
}

/// Scans the install folder and decides whether this run is in safe
/// mode.
fn prepare(
    world: &mut World,
    root: &Path,
    plugins: &Path,
) -> Vec<Found> {
    let mut found = Vec::new();
    let cap = world
        .get_resource::<Install>()
        .cloned()
        .unwrap_or_default()
        .manifest_bytes;
    for scanned in scan(plugins, cap) {
        match scanned {
            Ok(plugin) => found.push(plugin),
            Err(rejected) => {
                warn!("plugins/{}: {}", rejected.folder, rejected.why)
            }
        }
    }
    let (culprits, gone) = marker::pending(root)
        .into_iter()
        .partition::<Vec<_>, _>(|loading| {
            found.iter().any(|plugin| {
                loading.is(&plugin.manifest.id, &plugin.hash)
            })
        });
    // A plugin that is gone, or whose library was replaced since,
    // cannot be the one that stopped it.
    for loading in &gone {
        marker::settled(root, &loading.id);
    }
    let culprits = culprits
        .into_iter()
        .map(|loading| loading.id)
        .collect::<Vec<_>>();
    if !culprits.is_empty() {
        warn!(
            "native plugins: the last run stopped before `{}` was \
             trusted, so no library is opened",
            culprits.join("`, `")
        );
        world.insert_resource(SafeMode::Stopped(culprits));
    } else if world.contains_resource::<SafeMode>() {
        warn!("native plugins: --safe-mode, no library is opened");
    } else {
        let unclean = marker::unclean(root);
        let limit = world
            .get_resource::<Guard>()
            .cloned()
            .unwrap_or_default()
            .unclean_starts;
        if limit > 0 && unclean >= limit {
            warn!(
                "native plugins: the last {unclean} runs with a \
                 library open did not exit, so no library is opened"
            );
            world.insert_resource(SafeMode::Crashed(unclean));
        }
    }
    found
}

/// Counts this run as unclean until it exits by itself, once, before
/// its first library is opened.
fn count(world: &mut World, root: &Path) {
    if world.contains_resource::<Counted>() {
        return;
    }
    if let Err(e) = marker::opening(root) {
        warn!("native plugins: `unclean` cannot be written: {e}");
    }
    world.insert_resource(Counted);
}

/// Where the chief stands on `plugin`, for the library that is there.
/// One whose entry panicked is asked about again.
fn standing_of(store: &Store, plugin: &Found) -> Standing {
    match store.get(&plugin.manifest.id) {
        Some(record)
            if record.stale(&plugin.hash) || record.failed =>
        {
            Standing::Unknown
        }
        record => standing(
            record,
            &plugin.manifest.version,
            &Asks::default(),
        ),
    }
}

/// Spawns an entity for each plugin found. One the chief installed
/// gets what its entry declared when the app was built, or is loaded
/// now if it was not opened then. Any other is held with nothing of
/// it opened, and the chief is asked to install it unless it was
/// declined as it is. It runs after the records are opened.
pub fn installed(world: &mut World) {
    let Some((root, plugins)) = world
        .get_resource::<HomeDir>()
        .map(|h| (h.0.root.clone(), h.0.plugins()))
    else {
        return;
    };
    if !world.contains_resource::<Store>() {
        warn!("native plugins: no records of the chief's answers");
        return;
    }
    let found = match world.remove_resource::<Scanned>() {
        Some(scanned) => scanned.0,
        None => prepare(world, &root, &plugins),
    };
    let mut opened =
        world.remove_resource::<Opened>().unwrap_or_default();
    let safe = world.get_resource::<SafeMode>().cloned();
    for plugin in found {
        take_in(world, plugin, &safe, &mut opened);
    }
    if let Some(SafeMode::Crashed(runs)) = safe {
        world.trigger(Notice(crashed(runs)));
    }
    world.flush();
}

/// Spawns the entity of `plugin`. One the chief installed for the
/// library that is there gets what its entry declared when the app
/// was built, or is loaded at the next frame if it was not opened
/// then. Any other is held, and the chief is asked to install it
/// unless it was declined as it is.
fn take_in(
    world: &mut World,
    plugin: Found,
    safe: &Option<SafeMode>,
    opened: &mut Opened,
) {
    let id = plugin.manifest.id.clone();
    let standing = standing_of(world.resource::<Store>(), &plugin);
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
            match safe {
                Some(SafeMode::Stopped(culprits))
                    if culprits.contains(&id) =>
                {
                    not_loaded(world, entity, STOPPED);
                    world.trigger(AskToRemove { plugin: id });
                }
                Some(SafeMode::Stopped(culprits)) => not_loaded(
                    world,
                    entity,
                    &format!(
                        "Not loaded: the last run stopped before \
                         `{}` was trusted.",
                        culprits.join("`, `")
                    ),
                ),
                Some(SafeMode::Asked) => {
                    not_loaded(world, entity, ASKED)
                }
                Some(SafeMode::Crashed(runs)) => {
                    not_loaded(world, entity, &crashed(*runs))
                }
                None => match opened.take(&id) {
                    Some(outcome) => {
                        loaded(world, entity, &id, outcome)
                    }
                    None => {
                        world.resource_mut::<Late>().0.push(entity)
                    }
                },
            }
        }
        Standing::Declined => {
            info!("native plugin {id}: declined, not loaded");
            world.entity_mut(entity).insert(Uninstalled);
        }
        Standing::Unknown => {
            if world
                .resource::<Store>()
                .get(&id)
                .is_some_and(|record| record.failed)
            {
                not_loaded(world, entity, FAILED);
            } else {
                info!(
                    "native plugin {id}: not installed, not loaded"
                );
            }
            world.entity_mut(entity).insert(Uninstalled);
            world.trigger(AskToInstall { plugin: id });
        }
    }
}

/// Reads the plugin folder that appeared, when it holds a native
/// plugin, and takes it in as the scan at startup does. A plugin the
/// chief put there replaces one of its id that is not installed,
/// whose question is taken back, and the chief is asked about it even
/// if it was declined; one that is installed takes the new library
/// when Hamlet starts again.
pub fn appeared(appeared: On<Appeared>, mut commands: Commands) {
    let (id, asked) = (appeared.plugin.clone(), appeared.asked);
    commands
        .queue(move |world: &mut World| arrive(world, &id, asked));
}

fn arrive(world: &mut World, id: &str, asked: bool) {
    let Some(plugins) =
        world.get_resource::<HomeDir>().map(|h| h.0.plugins())
    else {
        return;
    };
    if !world.contains_resource::<Store>() {
        return;
    }
    let folder = plugins.join(id);
    let cap = world
        .get_resource::<Install>()
        .cloned()
        .unwrap_or_default()
        .manifest_bytes;
    let Some(text) = manifest_text(&folder, cap) else {
        return;
    };
    if !is_native(&text) {
        return;
    }
    let plugin = match read(&folder, &text) {
        Ok(plugin) => plugin,
        Err(rejected) => {
            warn!("plugins/{}: {}", rejected.folder, rejected.why);
            return;
        }
    };
    let known = world
        .query::<(Entity, &PluginInfo, Has<Uninstalled>)>()
        .iter(world)
        .find(|(_, info, _)| info.id == id)
        .map(|(entity, _, uninstalled)| (entity, uninstalled));
    match known {
        Some((entity, false)) => {
            if asked {
                world.entity_mut(entity).insert(Reload {
                    note: Some(REPLACED.into()),
                    rollback: false,
                });
            }
            return;
        }
        Some(_) if !asked => return,
        Some((entity, true)) => {
            world.entity_mut(entity).despawn();
            let asking = Subject::Install { plugin: id.into() };
            let open = world
                .query::<(Entity, &Subject)>()
                .iter(world)
                .filter(|(_, about)| **about == asking)
                .map(|(petition, _)| petition)
                .collect::<Vec<_>>();
            for petition in open {
                world.entity_mut(petition).despawn();
            }
        }
        None => {}
    }
    let declined = matches!(
        standing_of(world.resource::<Store>(), &plugin),
        Standing::Declined
    );
    let safe = world.get_resource::<SafeMode>().cloned();
    take_in(world, plugin, &safe, &mut Opened::default());
    if declined && asked {
        world.trigger(AskToInstall { plugin: id.into() });
    }
    world.flush();
}

/// What an installed plugin whose folder the chief filled again is
/// told.
const REPLACED: &str = "A new version is in its folder. It loads \
                        when Hamlet starts again, which asks you \
                        first where its library changed.";

/// What the chief and each plugin are told in a run that opens no
/// library after `runs` runs in a row did not exit.
fn crashed(runs: u32) -> String {
    format!(
        "Not loaded: Hamlet stopped without exiting the last {runs} \
         times it ran with native plugins, so none is loaded in \
         this run. Remove the one at fault from the Plugins page; \
         the next start loads them again."
    )
}

/// What the plugin that was loading when the last run stopped is
/// told.
const STOPPED: &str = "Hamlet stopped before this was trusted, so \
                       no native plugin was loaded. Remove it, or \
                       delete the file `loading` in the home to try \
                       again.";

/// What a plugin is told in a run started with `--safe-mode`.
const ASKED: &str = "Not loaded: Hamlet was started with \
                     --safe-mode. Start it without to load native \
                     plugins.";

/// What the chief and a plugin whose entry panicked are told.
const PANICKED: &str = "Not loaded: its entry panicked. What it \
                        added before that runs until Hamlet \
                        restarts, and the next start does not open \
                        it until you install it again.";

/// What a plugin whose entry panicked in an earlier run is told.
const FAILED: &str = "Not loaded: its entry panicked in an earlier \
                      run. Install it again to load it.";

/// What a plugin let in while the app runs is told.
const LATE: &str = "Loaded into the running app: the steps that \
                    finish an app, and the render app, reach what \
                    it adds when Hamlet starts again.";

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

/// Copies the library `file` of the plugin `id`, marks it as loading
/// and opens it. The marker goes again when it is refused.
fn open(
    root: &Path,
    id: &str,
    file: &Path,
    granted: &str,
) -> Result<EntryFn, String> {
    let copy = cache::private_copy(root, id, file, granted)
        .map_err(|why| format!("Not loaded: {why}"))?;
    marker::begin(root, id, granted).map_err(|e| {
        format!(
            "Not loaded: the `loading` file cannot be written: {e}"
        )
    })?;
    library::open(&copy).map_err(|refusal| {
        marker::settled(root, id);
        format!("Not loaded: {refusal}")
    })
}

/// Runs `entry` on `app`. A panic in it is caught; one in a library
/// is caught by the library, which gives false.
pub fn run_on(app: &mut App, entry: EntryFn) -> Result<bool, String> {
    // Whatever an earlier entry left is not this one's.
    app.world_mut().remove_resource::<Declared>();
    catch_unwind(AssertUnwindSafe(|| entry(app)))
        .map_err(|_| "its entry panicked".to_string())
}

/// Takes what the entry of the plugin `id` declared, once it `ran`.
/// The marker stays while the library's systems run their first
/// clean frames, those of an entry that panicked too.
fn settle(
    world: &mut World,
    root: &Path,
    id: &str,
    ran: Result<bool, String>,
) -> Outcome {
    let declared = world.remove_resource::<Declared>();
    let frames = world
        .get_resource::<Guard>()
        .cloned()
        .unwrap_or_default()
        .clean_frames;
    if frames == 0 {
        marker::settled(root, id);
    } else {
        world.resource_mut::<Settling>().add(id, frames);
    }
    match ran {
        Ok(true) => Ok(declared.map_or(nothing, |d| d.0)),
        Ok(false) | Err(_) => Err(Failure::Panicked),
    }
}

/// Switches the plugin on with what its entry declared, or marks it
/// failed with why it is not loaded. One whose entry panicked is
/// recorded, so that the next start asks the chief before opening it.
fn loaded(
    world: &mut World,
    plugin: Entity,
    id: &str,
    outcome: Outcome,
) {
    match outcome {
        Ok(declared) => {
            world.entity_mut(plugin).insert(EntryPoint(declared));
            info!("native plugin {id}: loaded");
            world.trigger(Enable(id.into()));
            world.flush();
        }
        Err(Failure::Refused(why)) => {
            world.entity_mut(plugin).insert(Status::Failed);
            not_loaded(world, plugin, &why);
        }
        Err(Failure::Lost(schedules)) => {
            world.entity_mut(plugin).insert(Status::Failed);
            let why = format!(
                "Not loaded: it adds to {} while the app runs it, \
                 which cannot be done until Hamlet starts again. \
                 What else it added runs until then, and the next \
                 start loads it whole.",
                schedules.join(", ")
            );
            not_loaded(world, plugin, &why);
            world.trigger(Notice(why));
        }
        Err(Failure::Panicked) => {
            world.entity_mut(plugin).insert(Status::Failed);
            not_loaded(world, plugin, PANICKED);
            if let Some(mut store) = world.get_resource_mut::<Store>()
                && let Some(record) = store.get(id).cloned()
            {
                store.set(id, record.failed());
            }
            let name = world
                .get::<PluginInfo>(plugin)
                .map_or(id.to_string(), |info| info.name.clone());
            world.trigger(Notice(format!("{name}: {PANICKED}")));
        }
    }
}

/// Loads the plugins let in while the app runs, at the start of a
/// frame, where only `Main` and `First` are running.
pub fn late(world: &mut World) {
    for plugin in std::mem::take(&mut world.resource_mut::<Late>().0)
    {
        if world.get_entity(plugin).is_ok() {
            start(world, plugin);
        }
    }
}

/// Opens the library of the plugin `plugin` and runs its entry with
/// the running world, in an app of its own. What the entry declared
/// is switched on by the plugin frame, as for any plugin.
fn start(world: &mut World, plugin: Entity) {
    let (Some(info), Some(file), Some(granted), Some(root)) = (
        world.get::<PluginInfo>(plugin).cloned(),
        world.get::<Library>(plugin).map(|l| l.file.clone()),
        world.get::<FullAccess>(plugin).map(|a| a.0.clone()),
        world.get_resource::<HomeDir>().map(|h| h.0.root.clone()),
    ) else {
        return;
    };
    count(world, &root);
    let outcome = open(&root, &info.id, &file, &granted)
        .map_err(Failure::Refused)
        .and_then(|entry| {
            world.remove_resource::<Declared>();
            let ran = with_app(world, entry);
            let lost = ran
                .as_ref()
                .map_or(Vec::new(), |(_, lost)| lost.clone());
            let outcome = settle(
                world,
                &root,
                &info.id,
                ran.map(|(ran, _)| ran),
            );
            match lost.is_empty() {
                true => outcome,
                false => outcome.and(Err(Failure::Lost(lost))),
            }
        });
    if outcome.is_ok() {
        world.entity_mut(plugin).insert(Reload {
            note: Some(LATE.into()),
            rollback: false,
        });
    }
    loaded(world, plugin, &info.id, outcome);
}

fn nothing(_: &mut Entry) {}

/// Runs `run` with an app that holds this world, so that code written
/// for an app can add its systems, observers and resources to a world
/// that is running, and names the schedules it added to that were
/// running then: what it added to those is taken out, as it would be
/// lost when they are put back. That app has none of the running
/// app's plugins and sub-apps, and is never finished. A panic in it
/// is caught and the world is put back; one in a library is caught by
/// the library.
pub fn with_app<R>(
    world: &mut World,
    run: impl FnOnce(&mut App) -> R,
) -> Result<(R, Vec<String>), String> {
    let running = world
        .get_resource::<Schedules>()
        .map(Schedules::get_temporarily_removed)
        .unwrap_or_default();
    let mut app = App::empty();
    std::mem::swap(app.world_mut(), world);
    let ran = catch_unwind(AssertUnwindSafe(|| run(&mut app)));
    std::mem::swap(app.world_mut(), world);
    // A schedule that runs is out of `Schedules`, so what was added
    // to it went to a new one of the same label.
    let mut lost = match world.get_resource_mut::<Schedules>() {
        Some(mut schedules) => running
            .iter()
            .filter(|label| schedules.remove(**label).is_some())
            .map(|label| format!("{label:?}"))
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    lost.sort();
    ran.map(|ran| (ran, lost))
        .map_err(|_| "its entry panicked".to_string())
}

/// Loads the plugin the chief has just let in at the start of the
/// next frame, unless this run is in safe mode.
pub fn admit(
    installed: On<Installed>,
    plugins: Query<
        (Entity, &PluginInfo),
        (With<FullAccess>, Without<Uninstalled>, Without<EntryPoint>),
    >,
    safe: Option<Res<SafeMode>>,
    mut late: ResMut<Late>,
    mut commands: Commands,
) {
    for (plugin, info) in &plugins {
        if info.id != installed.plugin {
            continue;
        }
        if let Some(safe) = &safe {
            let why = match **safe {
                SafeMode::Asked => ASKED,
                SafeMode::Stopped(_) => {
                    "Not loaded: native plugins are off after a stop \
                     while loading one. Restart Hamlet."
                }
                SafeMode::Crashed(_) => {
                    "Not loaded: native plugins are off in this run, \
                     after runs that did not exit. Restart Hamlet."
                }
            };
            commands.entity(plugin).insert(NotLoaded(why.into()));
            continue;
        }
        if !late.0.contains(&plugin) {
            late.0.push(plugin);
        }
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

    #[derive(Resource, Default)]
    struct Lost(Vec<String>);

    #[test]
    fn what_a_late_entry_adds_to_a_running_schedule_is_named() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).init_resource::<Lost>();
        app.add_systems(First, |world: &mut World| {
            if world.contains_resource::<Added>() {
                return;
            }
            let (_, lost) = with_app(world, |app| {
                adds_a_resource_and_a_system(app);
                app.add_systems(First, |mut added: ResMut<Added>| {
                    added.0 += 100
                });
            })
            .unwrap();
            world.resource_mut::<Lost>().0 = lost;
        });
        app.update();
        app.update();
        assert_eq!(app.world().resource::<Lost>().0, ["First"]);
        // What it added to `Update`, which was not running, runs.
        assert_eq!(app.world().resource::<Added>().0, 2);
    }

    /// An app over a scratch home that holds the native plugin
    /// `spark`, whose library is not a real one. `before` runs on the
    /// home and the app before the native plugin is added.
    fn home_with_spark(
        name: &str,
        record: bool,
        before: impl FnOnce(&Path, &mut App),
    ) -> (App, PathBuf) {
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
        }));
        before(&root, &mut app);
        app.add_plugins((
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
    fn a_granted_library_is_opened_while_the_app_is_built() {
        let (mut app, root) =
            home_with_spark("built", true, |_, _| {});
        // Refused when the app was built, before any frame ran.
        assert!(matches!(
            app.world().resource::<Opened>().0.as_slice(),
            [(id, Err(Failure::Refused(why)))] if id == "spark"
                && why.contains("cannot be opened")
        ));
        app.update();
        let (uninstalled, why, status) = spark(&mut app);
        assert!(!uninstalled);
        assert_eq!(status, Status::Failed);
        assert!(why.unwrap().contains("cannot be opened"));
        assert!(marker::pending(&root).is_empty());
        // Counted until it exits by itself.
        assert_eq!(marker::unclean(&root), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn after_runs_that_did_not_exit_no_library_is_opened() {
        let (mut app, root) =
            home_with_spark("crashed", true, |root, _| {
                marker::opening(root).unwrap();
                marker::opening(root).unwrap();
            });
        app.update();
        let (_, why, status) = spark(&mut app);
        assert_eq!(status, Status::Disabled);
        assert_eq!(why, Some(crashed(2)));
        assert_eq!(
            app.world().resource::<SafeMode>(),
            &SafeMode::Crashed(2)
        );
        // An exit by itself starts the count again.
        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(marker::unclean(&root), 0);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn after_a_stop_while_loading_no_library_is_opened() {
        let library = |root: &Path| {
            let file = format!(
                "plugins/spark/libspark.{}",
                std::env::consts::DLL_EXTENSION
            );
            crate::install::hash(&root.join(file)).unwrap()
        };
        let (mut app, root) =
            home_with_spark("safe", true, |root, _| {
                marker::begin(root, "spark", &library(root)).unwrap();
            });
        app.update();
        let (uninstalled, why, status) = spark(&mut app);
        assert!(!uninstalled);
        assert_eq!(status, Status::Disabled);
        assert!(why.unwrap().contains("Hamlet stopped"));
        assert_eq!(
            app.world().resource::<SafeMode>(),
            &SafeMode::Stopped(vec!["spark".into()])
        );
        // The marker stays until the plugin is removed, or its
        // library is replaced.
        assert_eq!(marker::pending(&root).len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
        let (mut app, root) =
            home_with_spark("replaced", true, |root, _| {
                marker::begin(root, "spark", "another").unwrap();
            });
        app.update();
        assert!(!app.world().contains_resource::<SafeMode>());
        assert!(marker::pending(&root).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn safe_mode_on_demand_opens_no_library() {
        let (mut app, root) =
            home_with_spark("asked", true, |_, app| {
                app.insert_resource(SafeMode::Asked);
            });
        app.update();
        let (uninstalled, why, status) = spark(&mut app);
        assert!(!uninstalled);
        assert_eq!(status, Status::Disabled);
        assert_eq!(why.as_deref(), Some(ASKED));
        assert!(marker::pending(&root).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_library_the_chief_did_not_see_is_asked_about() {
        let (mut app, root) =
            home_with_spark("changed", false, |_, _| {});
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
    fn a_folder_the_chief_put_there_is_asked_about_again() {
        let (mut app, root) =
            home_with_spark("appeared", false, |_, _| {});
        app.update();
        app.world_mut().trigger(Appeared {
            plugin: "spark".into(),
            asked: true,
        });
        app.world_mut().flush();
        let (uninstalled, ..) = spark(&mut app);
        assert!(uninstalled);
        let mut asked = app.world_mut().query::<&Subject>();
        assert_eq!(asked.iter(app.world()).count(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_panicked_entry_is_not_opened_until_installed_again() {
        let (mut app, root) =
            home_with_spark("panicked", true, |_, _| {});
        app.update();
        let plugin = app
            .world_mut()
            .query::<(Entity, &PluginInfo)>()
            .iter(app.world())
            .find(|(_, info)| info.id == "spark")
            .unwrap()
            .0;
        loaded(
            app.world_mut(),
            plugin,
            "spark",
            Err(Failure::Panicked),
        );
        assert!(
            app.world()
                .resource::<Store>()
                .get("spark")
                .unwrap()
                .failed
        );

        let (mut again, _) =
            home_with_spark("panicked", false, |_, _| {});
        assert!(again.world().resource::<Opened>().0.is_empty());
        again.update();
        let (uninstalled, why, _) = spark(&mut again);
        assert!(uninstalled);
        assert_eq!(why.as_deref(), Some(FAILED));
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
