//! The view kept between launches: the desks, the choices of the
//! View menu, and the selection.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::{Real, Time};
use hamlet_client::ViewFolder;
use hamlet_client::realm::{AgentNumber, HamletId, Left};
use hamlet_desktop::awake::KeepAwake;
use hamlet_realm::pick::{Selected, Thing, select};
use hamlet_realm::sky::{
    DistanceFogSettings, Quality, VolumetricFogSettings,
};
use hamlet_realm::sound::SoundSettings;
use hamlet_realm::{North, chief};
use serde::{Deserialize, Serialize};

use crate::PanelSystems;
use crate::convo::Convo;
use crate::desk::{self, Desks, Part};
use crate::roll::Roll;
use crate::world_view::{NorthTarget, TimeOfDay};

/// Seconds a change waits before it is written, so that dragging a
/// divider writes once.
const SETTLE: f32 = 0.8;

/// The time a remembered selection is looked for before it is given
/// up: an agent removed since never comes. Longer than the first
/// reconnection tries, for a window opened before its server listens.
const PATIENCE: Duration = Duration::from_secs(10);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Pending>()
        .init_resource::<Kept>()
        .add_systems(
            PreUpdate,
            reselect
                .run_if(|pending: Res<Pending>| pending.is_seeking())
                .in_set(PanelSystems::Follow),
        )
        // After the desk has taken in what fynix moved in the dock.
        .add_systems(
            Update,
            (
                note_selected.run_if(
                    resource_changed::<Selected>
                        .or_eager(resource_changed::<Roll>),
                ),
                keep,
            )
                .chain()
                .after(desk::capture),
        );
}

/// What a switch kept before it existed reads as.
fn on() -> bool {
    true
}

/// The kept settings, or `assets/world.toml`'s where they were kept
/// in a shape they no longer have.
fn kept_or_designed<'de, D, T>(kept: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    Ok(toml::Value::deserialize(kept)
        .ok()
        .and_then(|settings| settings.try_into().ok())
        .unwrap_or_default())
}

/// The kept view.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct View {
    sky: TimeOfDay,
    sound_on: bool,
    music_on: bool,
    #[serde(default = "on")]
    wildlife_on: bool,
    #[serde(default = "on")]
    ambience_on: bool,
    sound_level: f32,
    music_level: f32,
    keep_awake: bool,
    quality: Quality,
    /// The fogs as the chief left them; `assets/world.toml`'s before
    /// they were kept.
    #[serde(default, deserialize_with = "kept_or_designed")]
    volumetric_fog: VolumetricFogSettings,
    #[serde(default, deserialize_with = "kept_or_designed")]
    distance_fog: DistanceFogSettings,
    /// Degrees the sun's path is turned.
    #[serde(default)]
    north: f32,
    /// The one dock kept before there were desks, read into the
    /// first.
    #[serde(default, skip_serializing)]
    dock: Vec<Part>,
    desks: Option<Desks>,
    /// The selection when the window closed.
    selected: Option<Chosen>,
}

/// The view kept in the window's folder, as read at launch; none
/// before the first launch.
#[derive(Resource, Clone, Debug, Default)]
pub struct Remembered(Option<View>);

impl Remembered {
    /// The view `view` keeps; an error naming the file when it does
    /// not parse.
    pub fn read(view: &Path) -> Result<Self> {
        let file = file(view);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self(None));
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("reading {}", file.display())
                });
            }
        };
        toml::from_str::<View>(&text)
            .map(|view| Self(Some(view)))
            .map_err(|e| chief::unparsed(&file, e.message()))
    }
}

/// Something that was selected, by a name that lasts from one
/// launch to the next.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "what", content = "of", rename_all = "lowercase")]
pub enum Chosen {
    /// The first hamlet's mayor, and its hall.
    Mayor,
    Hall,
    /// An agent and its house, by the agent's name.
    Agent(String),
    House(String),
    /// The ruin of one that is gone.
    Ruin(String),
    /// A hamlet's common, by the hamlet's own name.
    Hamlet(String),
    /// This window's chief, and its Hearth.
    Chief,
    Hearth,
}

/// The selection to make again once the world has it. Nothing is
/// selected until then.
#[derive(Resource, Default)]
pub(crate) struct Pending {
    chosen: Option<Chosen>,
    /// The real time it was first looked for.
    began: Option<Duration>,
}

impl Pending {
    /// A remembered selection is still looked for.
    pub(crate) fn is_seeking(&self) -> bool {
        self.chosen.is_some()
    }
}

impl Chosen {
    /// `thing` by the names it will still have at the next launch.
    fn of(
        names: &Query<&Name>,
        hamlets: &Query<&HamletId>,
        thing: Thing,
    ) -> Option<Chosen> {
        let name = |of: Entity| {
            names.get(of).ok().map(|name| name.as_str().to_string())
        };
        Some(match thing {
            Thing::Keeper => Chosen::Mayor,
            Thing::Cottage => Chosen::Hall,
            Thing::Agent(agent) => Chosen::Agent(name(agent)?),
            Thing::House(agent) => Chosen::House(name(agent)?),
            Thing::Ruin(agent) => Chosen::Ruin(name(agent)?),
            Thing::Common(hamlet) => {
                Chosen::Hamlet(hamlets.get(hamlet).ok()?.0.clone())
            }
            Thing::Chief(_) => Chosen::Chief,
            Thing::ChiefPlace(_) => Chosen::Hearth,
            Thing::Library
            | Thing::Clockwork(_)
            | Thing::Workbench(_) => {
                return None;
            }
        })
    }

    /// The thing it names, once the world has it.
    fn thing(&self, world: &mut World) -> Option<Thing> {
        // An agent by its name, living or gone as asked.
        let agent = |world: &mut World, name: &str, gone: bool| {
            world
                .query_filtered::<(Entity, &Name, Has<Left>), With<AgentNumber>>()
                .iter(world)
                .find(|(_, called, left)| {
                    called.as_str() == name && *left == gone
                })
                .map(|(agent, ..)| agent)
        };
        Some(match self {
            Chosen::Mayor => Thing::Keeper,
            Chosen::Hall => Thing::Cottage,
            Chosen::Agent(name) => {
                Thing::Agent(agent(world, name, false)?)
            }
            Chosen::House(name) => {
                Thing::House(agent(world, name, false)?)
            }
            Chosen::Ruin(name) => {
                Thing::Ruin(agent(world, name, true)?)
            }
            Chosen::Hamlet(name) => Thing::Common(
                hamlet_client::realm::hamlet(world, name)?,
            ),
            Chosen::Chief | Chosen::Hearth => {
                let name = world.resource::<Convo>().name.clone();
                let chief = chief::named(world, &name)?;
                match self {
                    Chosen::Chief => Thing::Chief(chief),
                    _ => Thing::ChiefPlace(chief),
                }
            }
        })
    }
}

/// Selects again what was selected when the window last closed, once
/// its hamlet and agents are in the world.
pub(crate) fn reselect(world: &mut World) {
    let Some(chosen) = world.resource::<Pending>().chosen.clone()
    else {
        return;
    };
    if world.resource::<Selected>().0.is_some() {
        // The chief was quicker.
        world.resource_mut::<Pending>().chosen = None;
        return;
    }
    match chosen.thing(world) {
        Some(thing) => {
            select(world, Some(thing));
            world.resource_mut::<Pending>().chosen = None;
        }
        None => {
            let now = world.resource::<Time<Real>>().elapsed();
            let mut pending = world.resource_mut::<Pending>();
            if now - *pending.began.get_or_insert(now) > PATIENCE {
                pending.chosen = None;
            }
        }
    }
}

fn file(view: &Path) -> PathBuf {
    view.join("view.toml")
}

/// Puts back the kept View choices and the kept desks.
pub fn restore(world: &mut World) {
    let Some(view) = world.resource::<Remembered>().0.clone() else {
        return;
    };
    *world.resource_mut::<TimeOfDay>() = view.sky;
    let mut sound = world.resource_mut::<SoundSettings>();
    sound.muted = !view.sound_on;
    sound.music_on = view.music_on;
    sound.wildlife_on = view.wildlife_on;
    sound.ambience_on = view.ambience_on;
    sound.sound_level = view.sound_level.clamp(0.0, 1.0);
    sound.music_level = view.music_level.clamp(0.0, 1.0);
    world.resource_mut::<KeepAwake>().0 = view.keep_awake;
    *world.resource_mut::<Quality>() = view.quality;
    world.insert_resource(view.volumetric_fog);
    world.insert_resource(view.distance_fog);
    // North starts where it was left, without a glide.
    let north = view.north.rem_euclid(360.0);
    world.insert_resource(NorthTarget(north));
    world.insert_resource(North(north));
    world.insert_resource(Pending {
        chosen: view.selected.clone(),
        began: None,
    });
    let desks = match view.desks {
        Some(desks) => desks.cleaned(),
        None => Desks::first(view.dock),
    };
    world.insert_resource(desks);
}

/// The selection, by the names it is kept under.
#[derive(Resource, Default)]
pub(crate) struct Kept(Option<Chosen>);

/// Notes what is selected by its lasting names, for [`keep`].
pub(crate) fn note_selected(
    selected: Res<Selected>,
    names: Query<&Name>,
    hamlets: Query<&HamletId>,
    mut kept: ResMut<Kept>,
) {
    let chosen = selected
        .0
        .and_then(|thing| Chosen::of(&names, &hamlets, thing));
    if kept.0 != chosen {
        kept.0 = chosen;
    }
}

/// Writes the view once it has changed and then stood still.
pub fn keep(
    time: Res<Time>,
    folder: Res<ViewFolder>,
    desks: Res<Desks>,
    sky: Res<TimeOfDay>,
    sound: Res<SoundSettings>,
    quality: Res<Quality>,
    awake: Res<KeepAwake>,
    (pending, kept, north): (
        Res<Pending>,
        Res<Kept>,
        Res<NorthTarget>,
    ),
    (fog, far): (
        Res<VolumetricFogSettings>,
        Res<DistanceFogSettings>,
    ),
    mut written: Local<Option<View>>,
    mut due: Local<Option<f32>>,
) {
    let now = time.elapsed_secs();
    if desks.is_changed()
        || kept.is_changed()
        || sky.is_changed()
        || sound.is_changed()
        || quality.is_changed()
        || awake.is_changed()
        || fog.is_changed()
        || far.is_changed()
        || north.is_changed()
    {
        *due = Some(now + SETTLE);
    }
    // Nothing is written over what is still to be selected again.
    if pending.chosen.is_some() || due.is_none_or(|at| now < at) {
        return;
    }
    *due = None;
    let view = View {
        sky: *sky,
        sound_on: !sound.muted,
        music_on: sound.music_on,
        wildlife_on: sound.wildlife_on,
        ambience_on: sound.ambience_on,
        sound_level: sound.sound_level,
        music_level: sound.music_level,
        keep_awake: awake.0,
        quality: *quality,
        volumetric_fog: fog.clone(),
        distance_fog: far.clone(),
        north: north.0,
        dock: Vec::new(),
        desks: Some(desks.clone()),
        selected: kept.0.clone(),
    };
    if written.as_ref() == Some(&view) {
        return;
    }
    if let Ok(text) = toml::to_string(&view)
        && std::fs::create_dir_all(&folder.0).is_ok()
    {
        let _ = std::fs::write(file(&folder.0), text);
    }
    *written = Some(view);
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::{
        DockAreaStyle, DockLeaf, DockRegistry, DockTree, Edge,
    };
    use hamlet_ui::theme::HamletTheme;

    use super::*;

    fn dock() -> DockTree {
        let mut tree = DockTree::new();
        let centre = tree.set_root_leaf(
            DockLeaf::new("centre", DockAreaStyle::TabBar)
                .with_windows(vec!["world".into()]),
        );
        let (queue, _) =
            tree.split(centre, Edge::Left, "needs".into()).unwrap();
        let parent = tree.parent_of(queue).unwrap();
        tree.set_fraction(parent, 0.2);
        let (side, agent) =
            tree.split(centre, Edge::Right, "agent".into()).unwrap();
        tree.add_tab(side, "skills");
        tree.set_active(side, agent);
        tree
    }

    fn view(desks: Option<Desks>) -> View {
        View {
            sky: TimeOfDay::At(18.5),
            sound_on: false,
            music_on: false,
            wildlife_on: false,
            ambience_on: true,
            sound_level: 0.5,
            music_level: 0.25,
            keep_awake: false,
            quality: Quality::Medium,
            volumetric_fog: VolumetricFogSettings {
                visibility: 400.0,
                ..VolumetricFogSettings::default()
            },
            distance_fog: DistanceFogSettings { distance: 1200.0 },
            north: 45.0,
            dock: Vec::new(),
            desks,
            selected: Some(Chosen::House("juniper".into())),
        }
    }

    #[test]
    fn a_changed_dock_and_view_choices_return_from_the_view_folder() {
        use bevy_fynix::ViewExt;
        use bevy_fynix::dock::DockWindowKind;
        use bevy_fynix::views::column;
        use hamlet_client::ViewFolder;
        use hamlet_testing::Scratch;

        let dir = Scratch::new("view-dock");
        let make = || {
            let mut app = App::new();
            app.insert_resource(ViewFolder(dir.join("view")))
                .init_resource::<Time>()
                .init_resource::<Selected>()
                .init_resource::<Roll>()
                .init_resource::<DockTree>()
                .init_resource::<TimeOfDay>()
                .init_resource::<SoundSettings>()
                .init_resource::<Quality>()
                .init_resource::<VolumetricFogSettings>()
                .init_resource::<DistanceFogSettings>()
                .init_resource::<North>()
                .init_resource::<NorthTarget>()
                .init_resource::<KeepAwake>()
                .init_resource::<DockRegistry<HamletTheme>>()
                .init_resource::<Desks>()
                .init_resource::<desk::Shown>()
                .add_systems(Update, desk::capture)
                .add_plugins(plugin);
            for name in ["world", "needs", "agent", "skills"] {
                app.world_mut()
                    .resource_mut::<DockRegistry<HamletTheme>>()
                    .register(
                        name,
                        DockWindowKind::new(name, || {
                            column(()).boxed()
                        }),
                    );
            }
            app
        };
        let mut app = make();
        app.world_mut().run_system_cached(desk::show).unwrap();
        app.update();
        *app.world_mut().resource_mut::<DockTree>() = dock();
        app.world_mut().resource_mut::<Desks>().add();
        *app.world_mut().resource_mut::<TimeOfDay>() =
            TimeOfDay::Night;
        app.world_mut().resource_mut::<SoundSettings>().music_on =
            false;
        *app.world_mut().resource_mut::<Quality>() = Quality::Medium;
        app.world_mut().resource_mut::<KeepAwake>().0 = false;
        app.update();
        assert!(!dir.join("view/view.toml").exists());
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        assert!(dir.join("view/view.toml").exists());
        assert!(!dir.join("view.toml").exists());

        let mut relaunched = make();
        relaunched.insert_resource(
            Remembered::read(&dir.join("view")).unwrap(),
        );
        restore(relaunched.world_mut());
        assert_eq!(
            *relaunched.world().resource::<TimeOfDay>(),
            TimeOfDay::Night
        );
        assert!(
            !relaunched.world().resource::<SoundSettings>().music_on
        );
        assert_eq!(
            *relaunched.world().resource::<Quality>(),
            Quality::Medium
        );
        assert!(!relaunched.world().resource::<KeepAwake>().0);
        let mut desks =
            relaunched.world_mut().resource_mut::<Desks>();
        assert_eq!(desks.list().len(), 2);
        assert_eq!(desks.shown(), 1);
        desks.show(0);
        relaunched
            .world_mut()
            .run_system_cached(desk::show)
            .unwrap();
        let tree = relaunched.world().resource::<DockTree>();
        let needs = tree.find_leaf_with_window("needs").unwrap();
        assert_eq!(
            tree.get(tree.parent_of(needs).unwrap())
                .unwrap()
                .as_split()
                .unwrap()
                .fraction,
            0.2
        );
        let side = tree.find_leaf_with_window("agent").unwrap();
        assert_eq!(
            tree.leaf(side)
                .unwrap()
                .tabs()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            ["agent", "skills"]
        );
    }

    #[test]
    fn what_was_selected_is_selected_again_once_it_is_there() {
        let mut world = World::new();
        world.init_resource::<Time<Real>>();
        world.init_resource::<Selected>();
        world.init_resource::<hamlet_realm::pick::Also>();
        world.init_resource::<Kept>();
        world.insert_resource(Pending {
            chosen: Some(Chosen::House("scout".into())),
            began: None,
        });
        // Its agent is not in the world yet: nothing is selected.
        reselect(&mut world);
        assert_eq!(world.resource::<Selected>().0, None);
        let scout =
            world.spawn((Name::new("scout"), AgentNumber(1))).id();
        reselect(&mut world);
        let house = Thing::House(scout);
        assert_eq!(world.resource::<Selected>().0, Some(house));
        assert!(world.resource::<Pending>().chosen.is_none());
        // And it is kept under the same name for the next launch.
        world.run_system_cached(note_selected).unwrap();
        assert_eq!(
            world.resource::<Kept>().0,
            Some(Chosen::House("scout".into()))
        );

        // One that never comes is given up, and nothing is selected.
        select(&mut world, None);
        world.insert_resource(Pending {
            chosen: Some(Chosen::Agent("nobody".into())),
            began: None,
        });
        reselect(&mut world);
        world.resource_mut::<Time<Real>>().advance_by(PATIENCE);
        reselect(&mut world);
        assert!(world.resource::<Pending>().is_seeking());
        world
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(16));
        reselect(&mut world);
        assert!(!world.resource::<Pending>().is_seeking());
        assert_eq!(world.resource::<Selected>().0, None);
    }

    #[test]
    fn desks_come_back_as_they_were_written() {
        let mut desks = Desks::first(desk::noted(&dock()));
        desks.duplicate(0);
        desks.rename(1, "Night shift");
        let view = view(Some(desks));
        let text = toml::to_string(&view).unwrap();
        assert_eq!(toml::from_str::<View>(&text).unwrap(), view);
    }

    #[test]
    fn the_dock_kept_before_desks_is_read_into_the_first() {
        #[derive(Serialize)]
        struct Before {
            dock: Vec<Part>,
        }

        let parts = desk::noted(&dock());
        let text = toml::to_string(&view(None)).unwrap()
            + &toml::to_string(&Before {
                dock: parts.clone(),
            })
            .unwrap();
        let mut world = World::new();
        world.insert_resource(Remembered(Some(
            toml::from_str::<View>(&text).unwrap(),
        )));
        world.init_resource::<TimeOfDay>();
        world.init_resource::<SoundSettings>();
        world.init_resource::<KeepAwake>();
        world.init_resource::<Quality>();
        restore(&mut world);
        assert_eq!(*world.resource::<Desks>(), Desks::first(parts));
    }

    #[test]
    fn a_view_that_does_not_parse_is_refused_and_left() {
        use hamlet_testing::Scratch;

        let dir = Scratch::new("view-unparsed");
        assert!(Remembered::read(&dir).unwrap().0.is_none());
        let text = "sky = \"night\"\n";
        std::fs::write(file(&dir), text).unwrap();
        let error = Remembered::read(&dir).unwrap_err().to_string();
        assert!(
            error.starts_with(&format!(
                "{} does not parse",
                file(&dir).display()
            )),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(file(&dir)).unwrap(),
            text
        );
    }
}
