//! The folder the chief keeps assets in, which the library reads, and
//! whether the realm builds its houses from them: Hamlet's own
//! assets until the chief chooses theirs, with an asset of theirs
//! standing in for Hamlet's own of the same name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::log::warn;
use bevy::time::Time;
use hamlet_desktop::folder::FolderChosen;
use hamlet_graph::{Ruleset, Shape};
use hamlet_graph_editor::library::{
    Kind, Library, LibraryFolder, Source, lone, parse_book,
};
use hamlet_realm::generator::house::{self, HouseRules};
use hamlet_realm::objects::{Model, Models, read};
use serde::{Deserialize, Serialize};

/// How often the folder's files are looked at for changes the realm
/// builds again from.
const LOOK: Duration = Duration::from_secs(1);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<AssetChoice>()
        .add_observer(chosen)
        .add_systems(Startup, asker)
        .add_systems(Update, (follow, rules));
}

/// Where assets are kept until the chief chooses a folder:
/// `HAMLET_ASSETS`, else `Hamlet` in the chief's documents.
#[derive(Resource, Clone, Debug)]
pub struct DefaultAssets(pub PathBuf);

impl DefaultAssets {
    /// The folder `vars` give on `os`, none without a home folder.
    pub fn of(
        os: &str,
        vars: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Option<Self> {
        if let Some(path) = vars("HAMLET_ASSETS") {
            return Some(Self(PathBuf::from(path)));
        }
        let home = match os {
            "windows" => vars("USERPROFILE"),
            _ => vars("HOME"),
        }?;
        Some(Self(
            PathBuf::from(home).join("Documents").join("Hamlet"),
        ))
    }
}

/// What the chief chose of their assets, kept with the view.
#[derive(
    Resource, Serialize, Deserialize, Clone, PartialEq, Debug, Default,
)]
pub struct AssetChoice {
    /// The folder chosen, none for the default one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// The realm's houses are built with the folder's assets.
    #[serde(default)]
    pub in_realm: bool,
}

/// The folder the library reads now: the one chosen, else the
/// default.
pub(crate) fn folder(
    choice: &AssetChoice,
    default: Option<&DefaultAssets>,
) -> Option<PathBuf> {
    choice
        .folder
        .clone()
        .or_else(|| default.map(|default| default.0.clone()))
}

/// What asks the system's dialog for a folder of assets.
#[derive(Component)]
pub(crate) struct Asker;

fn asker(mut commands: Commands) {
    commands.spawn(Asker);
}

/// Keeps the folder the dialog gave.
fn chosen(
    chosen: On<FolderChosen>,
    askers: Query<(), bevy::ecs::query::With<Asker>>,
    mut choice: ResMut<AssetChoice>,
) {
    if askers.contains(chosen.asker) {
        choice.folder = Some(chosen.folder.clone());
    }
}

/// Points the library at the folder chosen. The models read from the
/// last folder are forgotten, as their paths are in it.
fn follow(
    mut commands: Commands,
    choice: Res<AssetChoice>,
    default: Option<Res<DefaultAssets>>,
    given: Option<Res<LibraryFolder>>,
    models: Option<ResMut<Models>>,
) {
    let Some(folder) = folder(&choice, default.as_deref()) else {
        return;
    };
    if given.as_ref().is_some_and(|given| given.0 == folder) {
        return;
    }
    if given.is_some()
        && let Some(mut models) = models
    {
        *models = Models::default();
    }
    commands.insert_resource(LibraryFolder(folder));
}

/// Gives the realm the rules of the houses the chief chose: Hamlet's
/// own, or those of the folder over Hamlet's own. Made again when
/// the choice, the library or a file of the folder changes.
fn rules(
    mut commands: Commands,
    time: Res<Time>,
    choice: Res<AssetChoice>,
    library: Res<Library>,
    mut looked: Local<Duration>,
    mut built: Local<Option<(bool, u64, Option<SystemTime>)>>,
) {
    let now = time.elapsed();
    if !choice.is_changed() && now - *looked < LOOK {
        return;
    }
    *looked = now;
    let stamp = if choice.in_realm {
        (true, library.revision, newest(&library))
    } else {
        (false, 0, None)
    };
    if built.as_ref() == Some(&stamp) {
        return;
    }
    let first = built.is_none();
    *built = Some(stamp);
    if !choice.in_realm {
        if !first {
            commands.insert_resource(HouseRules::default());
        }
        return;
    }
    let (ruleset, models) = theirs(&library);
    commands.insert_resource(HouseRules::new(ruleset, models));
}

/// When the newest of the folder's blueprints, its catalogue and its
/// meshes was written.
fn newest(library: &Library) -> Option<SystemTime> {
    let catalogue = library.root()?.join("catalogue.ron");
    library
        .items
        .iter()
        .filter_map(|item| match &item.source {
            Source::File(path) => Some(path.as_path()),
            _ => None,
        })
        .chain([catalogue.as_path()])
        .filter_map(|path| path.metadata().ok()?.modified().ok())
        .max()
}

/// Hamlet's own graphs of the house with the folder's blueprints of
/// the same name in their place and its others beside them, its
/// catalogue, and the models of its meshes by their names.
fn theirs(library: &Library) -> (Ruleset, BTreeMap<String, Model>) {
    let mut graphs = BTreeMap::new();
    for (name, text) in house::blueprints() {
        if let Some(book) = parse_book(text) {
            graphs.insert((*name).to_owned(), lone(book).graph);
        }
    }
    for item in library.items.iter().filter(|item| {
        item.kind == Kind::Blueprint
            && matches!(item.source, Source::File(_))
    }) {
        match item.read() {
            Ok(book) => {
                let mut graph = lone(book).graph;
                hamlet_blueprint::upgrade(&mut graph);
                graphs.insert(item.name.clone(), graph);
            }
            Err(error) => warn!("{error:#}"),
        }
    }
    let mut models = BTreeMap::new();
    if let Some(root) = library.root() {
        for (name, asset) in &library.catalogue.0 {
            if let Shape::Mesh(file) = &asset.shape {
                match model(root, file) {
                    Ok(model) => {
                        models.insert(name.clone(), model);
                    }
                    Err(error) => warn!("{name}: {error:#}"),
                }
            }
        }
    }
    let ruleset = Ruleset {
        entry: "house".to_owned(),
        graphs,
        assets: library.catalogue.clone(),
    };
    (ruleset, models)
}

fn model(root: &Path, file: &str) -> anyhow::Result<Model> {
    read(&std::fs::read(root.join(file))?)
}
