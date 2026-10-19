//! The view kept between launches: the places of the panels, the
//! choices of the View menu, and the selection.

use std::path::{Path, PathBuf};

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::query::{Has, With};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::time::Time;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockNode, DockRegistry, DockSplit,
    DockTree, NodeId, SplitAxis,
};
use hamlet_core::agents::{Citizen, Left, Settlement};
use hamlet_core::core::HomeDir;
use hamlet_desktop::awake::KeepAwake;
use hamlet_realm_api::chief;
use hamlet_realm_api::pick::{Selected, Thing, select};
use hamlet_realm_api::sky::{Quality, TimeOfDay};
use hamlet_realm_api::sound::SoundChoice;
use serde::{Deserialize, Serialize};

use crate::me::Me;
use crate::roll::Roll;
use crate::theme::HamletTheme;

/// Seconds a change waits before it is written, so that dragging a
/// divider writes once.
const SETTLE: f32 = 0.8;

/// The frames a remembered selection is looked for before it is given
/// up: an agent removed since never comes.
const PATIENCE: u32 = 240;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Pending>()
        .init_resource::<Kept>()
        .init_resource::<TimeOfDay>()
        .init_resource::<Quality>()
        .add_systems(
            Update,
            (
                reselect
                    .run_if(|pending: Res<Pending>| pending.seeks()),
                note_selected.run_if(
                    resource_changed::<Selected>
                        .or_eager(resource_changed::<Roll>),
                ),
                keep,
            )
                .chain(),
        );
}

/// One node of the dock, with its children named by their place in
/// the list.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Part {
    Tabs {
        area: String,
        tabs: Vec<String>,
        /// The place of the tab in front.
        front: usize,
        #[serde(default)]
        headless: bool,
        /// It stays when its last tab is closed.
        #[serde(default)]
        kept: bool,
    },
    Split {
        /// `a` is above `b`. Beside it otherwise.
        stacked: bool,
        /// The share of the split that `a` takes.
        share: f32,
        a: usize,
        b: usize,
    },
}

/// The kept view. The first part of `dock` is its root.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct View {
    sky: TimeOfDay,
    // On where the file says nothing of them.
    #[serde(default = "on")]
    sound_on: bool,
    #[serde(default = "on")]
    music_on: bool,
    #[serde(default = "full")]
    sound_level: f32,
    #[serde(default = "full")]
    music_level: f32,
    #[serde(default = "on")]
    keep_awake: bool,
    #[serde(default)]
    quality: Quality,
    #[serde(default)]
    dock: Vec<Part>,
    /// The selection when the window closed.
    #[serde(default)]
    selected: Option<Chosen>,
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
    /// Frames it has been looked for.
    tried: u32,
}

impl Pending {
    /// A remembered selection is still looked for.
    pub(crate) fn seeks(&self) -> bool {
        self.chosen.is_some()
    }
}

impl Chosen {
    /// `thing` by the names it will still have at the next launch.
    fn of(
        names: &Query<&Name>,
        hamlets: &Query<&Settlement>,
        thing: Thing,
    ) -> Option<Chosen> {
        let name = |of: Entity| {
            names.get(of).ok().map(|name| name.as_str().to_string())
        };
        Some(match thing {
            Thing::Keeper => Chosen::Mayor,
            Thing::Cottage => Chosen::Hall,
            Thing::Resident(agent) => Chosen::Agent(name(agent)?),
            Thing::ResidentHouse(agent) => {
                Chosen::House(name(agent)?)
            }
            Thing::Ruin(agent) => Chosen::Ruin(name(agent)?),
            Thing::Common(hamlet) => {
                Chosen::Hamlet(hamlets.get(hamlet).ok()?.name.clone())
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
                .query_filtered::<(Entity, &Name, Has<Left>), With<Citizen>>()
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
                Thing::Resident(agent(world, name, false)?)
            }
            Chosen::House(name) => {
                Thing::ResidentHouse(agent(world, name, false)?)
            }
            Chosen::Ruin(name) => {
                Thing::Ruin(agent(world, name, true)?)
            }
            Chosen::Hamlet(name) => Thing::Common(
                world
                    .query::<(Entity, &Settlement)>()
                    .iter(world)
                    .find(|(_, hamlet)| hamlet.name == *name)
                    .map(|(hamlet, _)| hamlet)?,
            ),
            Chosen::Chief | Chosen::Hearth => {
                let user = world.resource::<Me>().user.clone();
                let chief = chief::named(world, &user)?;
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
            let mut pending = world.resource_mut::<Pending>();
            pending.tried += 1;
            if pending.tried > PATIENCE {
                pending.chosen = None;
            }
        }
    }
}

fn on() -> bool {
    true
}

fn full() -> f32 {
    1.0
}

fn file(home: &Path) -> PathBuf {
    home.join("view.toml")
}

/// Writes the node `id` and all under it to `parts`, and gives its
/// place there.
fn note(
    tree: &DockTree,
    id: NodeId,
    parts: &mut Vec<Part>,
) -> Option<usize> {
    let at = parts.len();
    match tree.get(id)? {
        DockNode::Leaf(leaf) => {
            let front = leaf
                .active
                .and_then(|tab| leaf.tab_index(tab))
                .unwrap_or_default();
            parts.push(Part::Tabs {
                area: leaf.area_id.clone(),
                tabs: leaf
                    .tabs()
                    .map(|(window, _)| window.to_string())
                    .collect(),
                front,
                headless: leaf.style == DockAreaStyle::Headless,
                kept: leaf.is_persistent(),
            });
        }
        DockNode::Split(split) => {
            let (stacked, share) =
                (split.axis == SplitAxis::Vertical, split.fraction);
            let (first, second) = (split.a, split.b);
            // Held open until its children have their places.
            parts.push(Part::Split {
                stacked,
                share,
                a: 0,
                b: 0,
            });
            let a = note(tree, first, parts)?;
            let b = note(tree, second, parts)?;
            parts[at] = Part::Split {
                stacked,
                share,
                a,
                b,
            };
        }
    }
    Some(at)
}

fn noted(tree: &DockTree) -> Vec<Part> {
    let mut parts = Vec::new();
    match tree.root.and_then(|root| note(tree, root, &mut parts)) {
        Some(_) => parts,
        None => Vec::new(),
    }
}

/// Builds the part at `at` into `tree`. A tab whose panel is not
/// `known` is left out, a tab bar left empty is dropped, and a split
/// that loses a side becomes its other side. `depth` stops a list
/// that names itself.
fn build(
    parts: &[Part],
    at: usize,
    depth: usize,
    known: &impl Fn(&str) -> bool,
    tree: &mut DockTree,
) -> Option<NodeId> {
    if depth > parts.len() {
        return None;
    }
    match parts.get(at)? {
        Part::Tabs {
            area,
            tabs,
            front,
            headless,
            kept,
        } => {
            let front = tabs.get(*front).filter(|tab| known(tab));
            let open = tabs
                .iter()
                .filter(|tab| known(tab))
                .cloned()
                .collect::<Vec<_>>();
            if open.is_empty() && !kept {
                return None;
            }
            let shown = front
                .and_then(|tab| open.iter().position(|t| t == tab))
                .unwrap_or_default();
            let style = if *headless {
                DockAreaStyle::Headless
            } else {
                DockAreaStyle::TabBar
            };
            let mut leaf =
                DockLeaf::new(area.clone(), style).with_windows(open);
            if *kept {
                leaf = leaf.persistent();
            }
            let id = tree.insert(DockNode::Leaf(leaf));
            let tab = tree
                .leaf(id)
                .and_then(|leaf| leaf.tabs().nth(shown))
                .map(|(_, tab)| tab);
            if let Some(tab) = tab {
                tree.set_active(id, tab);
            }
            Some(id)
        }
        Part::Split {
            stacked,
            share,
            a,
            b,
        } => {
            let first = build(parts, *a, depth + 1, known, tree);
            let second = build(parts, *b, depth + 1, known, tree);
            let (Some(a), Some(b)) = (first, second) else {
                return first.or(second);
            };
            let axis = if *stacked {
                SplitAxis::Vertical
            } else {
                SplitAxis::Horizontal
            };
            let id = tree.insert(DockNode::Split(DockSplit {
                axis,
                fraction: 0.5,
                a,
                b,
            }));
            tree.set_fraction(id, *share);
            Some(id)
        }
    }
}

/// The dock that `parts` describe, or nothing if they describe none.
fn rebuilt(
    parts: &[Part],
    known: &impl Fn(&str) -> bool,
) -> Option<DockTree> {
    let mut tree = DockTree::new();
    let root = build(parts, 0, 0, known, &mut tree)?;
    tree.root = Some(root);
    Some(tree)
}

fn read(home: &Path) -> Option<View> {
    let text = std::fs::read_to_string(file(home)).ok()?;
    toml::from_str::<View>(&text).ok()
}

/// Puts back the kept View choices and the kept dock. False when
/// there is no dock to put back.
pub fn restore(world: &mut World) -> bool {
    let home = world.resource::<HomeDir>().0.root.clone();
    let Some(view) = read(&home) else {
        return false;
    };
    *world.resource_mut::<TimeOfDay>() = view.sky;
    world.insert_resource(SoundChoice {
        muted: !view.sound_on,
        music_on: view.music_on,
        sound_level: view.sound_level.clamp(0.0, 1.0),
        music_level: view.music_level.clamp(0.0, 1.0),
    });
    world.resource_mut::<KeepAwake>().0 = view.keep_awake;
    *world.resource_mut::<Quality>() = view.quality;
    world.insert_resource(Pending {
        chosen: view.selected.clone(),
        tried: 0,
    });
    let registry = world.resource::<DockRegistry<HamletTheme>>();
    let known = |id: &str| registry.get(id).is_some();
    let Some(tree) = rebuilt(&view.dock, &known) else {
        return false;
    };
    *world.resource_mut::<DockTree>() = tree;
    true
}

/// The selection, by the names it is kept under.
#[derive(Resource, Default)]
pub(crate) struct Kept(Option<Chosen>);

/// Notes what is selected by its lasting names, for [`keep`].
pub(crate) fn note_selected(
    selected: Res<Selected>,
    names: Query<&Name>,
    hamlets: Query<&Settlement>,
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
#[allow(clippy::too_many_arguments)]
pub fn keep(
    time: Res<Time>,
    home: Res<HomeDir>,
    tree: Res<DockTree>,
    sky: Res<TimeOfDay>,
    sound: Res<SoundChoice>,
    quality: Res<Quality>,
    awake: Res<KeepAwake>,
    (pending, kept): (Res<Pending>, Res<Kept>),
    mut written: Local<Option<View>>,
    mut due: Local<Option<f32>>,
) {
    let now = time.elapsed_secs();
    if tree.is_changed()
        || kept.is_changed()
        || sky.is_changed()
        || sound.is_changed()
        || quality.is_changed()
        || awake.is_changed()
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
        sound_level: sound.sound_level,
        music_level: sound.music_level,
        keep_awake: awake.0,
        quality: *quality,
        dock: noted(&tree),
        selected: kept.0.clone(),
    };
    if written.as_ref() == Some(&view) {
        return;
    }
    if let Ok(text) = toml::to_string(&view) {
        let _ = std::fs::write(file(&home.0.root), text);
    }
    *written = Some(view);
}

#[cfg(test)]
mod tests {
    use bevy_fynix::dock::Edge;

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

    #[test]
    fn what_was_selected_is_selected_again_once_it_is_there() {
        let mut world = World::new();
        world.init_resource::<Selected>();
        world.init_resource::<hamlet_realm_api::pick::Also>();
        world.init_resource::<Kept>();
        world.insert_resource(Pending {
            chosen: Some(Chosen::House("scout".into())),
            tried: 0,
        });
        // Its agent is not in the world yet: nothing is selected.
        reselect(&mut world);
        assert_eq!(world.resource::<Selected>().0, None);
        let scout =
            world.spawn((Name::new("scout"), Citizen(1))).id();
        reselect(&mut world);
        let house = Thing::ResidentHouse(scout);
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
            tried: 0,
        });
        for _ in 0..=PATIENCE {
            reselect(&mut world);
        }
        assert!(world.resource::<Pending>().chosen.is_none());
        assert_eq!(world.resource::<Selected>().0, None);
    }

    #[test]
    fn a_dock_comes_back_as_it_was_written() {
        let view = View {
            sky: TimeOfDay::Night,
            sound_on: false,
            music_on: false,
            sound_level: 0.5,
            music_level: 0.25,
            keep_awake: false,
            quality: Quality::Medium,
            dock: noted(&dock()),
            selected: Some(Chosen::House("juniper".into())),
        };
        let text = toml::to_string(&view).unwrap();
        let read = toml::from_str::<View>(&text).unwrap();
        assert_eq!(read, view);
        // A view that says nothing of sound or music has both.
        let before = text
            .replace("sound_on = false\n", "")
            .replace("music_on = false\n", "");
        let before = toml::from_str::<View>(&before).unwrap();
        assert!(before.sound_on && before.music_on);

        let tree = rebuilt(&read.dock, &|_| true).unwrap();
        assert_eq!(noted(&tree), view.dock);
        let side = tree.find_leaf_with_window("agent").unwrap();
        let front = tree.active(side).unwrap();
        let leaf = tree.leaf(side).unwrap();
        assert_eq!(leaf.tabs().count(), 2);
        assert_eq!(leaf.tab_index(front), Some(0));
    }

    #[test]
    fn a_panel_that_is_gone_is_left_out() {
        let parts = noted(&dock());
        let tree =
            rebuilt(&parts, &|id| id != "needs" && id != "agent")
                .unwrap();
        assert!(tree.find_leaf_with_window("needs").is_none());
        let side = tree.find_leaf_with_window("skills").unwrap();
        assert_eq!(tree.leaf(side).unwrap().tabs().count(), 1);
        assert!(tree.find_leaf_with_window("world").is_some());
        assert!(rebuilt(&parts, &|_| false).is_none());
    }

    #[test]
    fn a_list_that_names_itself_builds_nothing() {
        let parts = vec![Part::Split {
            stacked: false,
            share: 0.5,
            a: 0,
            b: 0,
        }];
        assert!(rebuilt(&parts, &|_| true).is_none());
    }
}
