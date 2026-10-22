//! Plugins that need others. A plugin switched on runs only while
//! each plugin it needs is there at a version that fits, switched on,
//! not stopped and able to run itself; else it is [`Blocked`] with
//! why, and starts again on its own once they are. Plugins start one
//! after another in the [`StartOrder`].

use std::collections::HashMap;

use bevy::app::{App, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::query::{Changed, Has, Or, With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use hamlet_engine_api::{Dependency, PluginId};
use hamlet_engine_core::{Budgets, LogLine};
use hamlet_engine_shell::StartRank;

use crate::PluginInfo;
use crate::entry::{Entry, EntryPoint};
use crate::switch::{
    Disable, Enable, Enabled, PluginCommandsExt, Stopped,
};
use crate::switched_off::SwitchedOff;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<StartOrder>()
        .add_systems(Update, (resolve, start_builtins).chain());
}

/// On a plugin switched on that starts once its turn comes in the
/// [`StartOrder`].
#[derive(Component, Clone, Copy, Debug)]
pub struct Starting;

/// On a plugin switched on that cannot run, as a plugin it needs
/// cannot: why.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Blocked(pub String);

/// Every plugin, in the order they start: by the depth of their
/// dependencies, then by id.
#[derive(Resource, Default, Debug)]
pub struct StartOrder(Vec<(Entity, StartRank)>);

impl StartOrder {
    /// Where `plugin`, of `info`, stands. One not placed yet stands
    /// with those that need nothing.
    pub fn rank(
        &self,
        plugin: Entity,
        info: &PluginInfo,
    ) -> StartRank {
        self.0.iter().find(|(at, _)| *at == plugin).map_or_else(
            || StartRank {
                depth: 0,
                plugin: info.id.clone(),
            },
            |(_, rank)| rank.clone(),
        )
    }

    /// The plugins to start now: those `on_its_way`, in order, up to
    /// the first that is not `ready`.
    pub fn ready(
        &self,
        on_its_way: impl Fn(Entity) -> bool,
        ready: impl Fn(Entity) -> bool,
    ) -> Vec<Entity> {
        self.0
            .iter()
            .map(|(plugin, _)| *plugin)
            .filter(|plugin| on_its_way(*plugin))
            .take_while(|plugin| ready(*plugin))
            .collect()
    }
}

/// A plugin's state as those that need it read it.
pub(crate) type State = (
    Entity,
    &'static PluginInfo,
    Has<Enabled>,
    Has<Starting>,
    Option<&'static Blocked>,
    Has<Stopped>,
);

/// Why a plugin needed is not there for the plugin that needs it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Unmet {
    Missing,
    /// Of a version that does not fit: that one.
    Version(String),
    SwitchedOff,
    NotRunning,
    Stopped,
    CannotStart,
}

impl Unmet {
    /// The end of "Needs X ^0.1, which ...".
    pub(crate) fn which(&self) -> String {
        match self {
            Self::Missing => "isn't installed".into(),
            Self::Version(version) => format!("is {version}"),
            Self::SwitchedOff => "is switched off".into(),
            Self::NotRunning => "isn't running".into(),
            Self::Stopped => "was stopped".into(),
            Self::CannotStart => "cannot start".into(),
        }
    }

    /// A word or two of it, for a list.
    pub(crate) fn short(&self) -> String {
        match self {
            Self::Missing => "Not installed".into(),
            Self::Version(version) => format!("Is {version}"),
            Self::SwitchedOff => "Switched off".into(),
            Self::NotRunning => "Not running".into(),
            Self::Stopped => "Stopped".into(),
            Self::CannotStart => "Cannot start".into(),
        }
    }
}

/// A plugin as those that need it see it.
struct Node<'a> {
    entity: Entity,
    info: &'a PluginInfo,
    /// Running, on its way or blocked.
    on: bool,
    stopped: bool,
    /// The person switched it off.
    kept_off: bool,
}

/// The plugins and what each needs: their depths, and why each
/// cannot run.
pub(crate) struct Graph<'a> {
    nodes: Vec<Node<'a>>,
    by_id: HashMap<&'a PluginId, usize>,
    /// The plugins each needs, of those there.
    edges: Vec<Vec<usize>>,
    depth: Vec<u32>,
    why: Vec<Option<String>>,
}

impl<'a> Graph<'a> {
    /// The plugins as `plugins` holds them, those in `off` kept off.
    pub(crate) fn read(
        plugins: impl Iterator<
            Item = (
                Entity,
                &'a PluginInfo,
                bool,
                bool,
                Option<&'a Blocked>,
                bool,
            ),
        >,
        off: &SwitchedOff,
    ) -> Self {
        let nodes = plugins
            .map(
                |(
                    entity,
                    info,
                    enabled,
                    starting,
                    blocked,
                    stopped,
                )| {
                    Node {
                        entity,
                        info,
                        on: enabled || starting || blocked.is_some(),
                        stopped,
                        kept_off: off.contains(&info.id),
                    }
                },
            )
            .collect::<Vec<_>>();
        Self::new(nodes)
    }

    fn new(nodes: Vec<Node<'a>>) -> Self {
        let by_id = nodes
            .iter()
            .enumerate()
            .map(|(at, node)| (&node.info.id, at))
            .collect::<HashMap<_, _>>();
        let edges = nodes
            .iter()
            .map(|node| {
                node.info
                    .dependencies
                    .iter()
                    .filter_map(|needed| {
                        by_id.get(&needed.id).copied()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let count = nodes.len();
        let mut graph = Self {
            nodes,
            by_id,
            edges,
            depth: vec![0; count],
            why: vec![None; count],
        };
        // Each group comes after the groups it needs.
        for group in Tarjan::groups(&graph.edges) {
            let depth = group
                .iter()
                .flat_map(|&at| &graph.edges[at])
                .filter(|needed| !group.contains(needed))
                .map(|&needed| graph.depth[needed] + 1)
                .max()
                .unwrap_or(0);
            let why = if let [at] = group[..] {
                graph.nodes[at].info.dependencies.iter().find_map(
                    |needed| {
                        let unmet = graph.unmet(needed)?;
                        Some(format!(
                            "Needs {} {}, which {}",
                            needed.id,
                            needed.requirement,
                            unmet.which()
                        ))
                    },
                )
            } else {
                let mut ids = group
                    .iter()
                    .map(|&at| graph.nodes[at].info.id.as_str())
                    .collect::<Vec<_>>();
                ids.sort_unstable();
                Some(format!(
                    "In a cycle of dependencies: {}",
                    ids.join(", ")
                ))
            };
            for &at in &group {
                graph.depth[at] = depth;
                graph.why[at].clone_from(&why);
            }
        }
        graph
    }

    fn at(&self, plugin: Entity) -> Option<usize> {
        self.nodes.iter().position(|node| node.entity == plugin)
    }

    /// Why `needed` is not there for a plugin that needs it.
    pub(crate) fn unmet(&self, needed: &Dependency) -> Option<Unmet> {
        let Some(&at) = self.by_id.get(&needed.id) else {
            return Some(Unmet::Missing);
        };
        let node = &self.nodes[at];
        if !needed.fits(&node.info.version) {
            return Some(Unmet::Version(node.info.version.clone()));
        }
        if !node.on {
            return Some(if node.kept_off {
                Unmet::SwitchedOff
            } else {
                Unmet::NotRunning
            });
        }
        if node.stopped {
            return Some(Unmet::Stopped);
        }
        self.why[at].as_ref().map(|_| Unmet::CannotStart)
    }

    /// Why `plugin` cannot run.
    pub(crate) fn why(&self, plugin: Entity) -> Option<&str> {
        self.why[self.at(plugin)?].as_deref()
    }

    /// The plugins that need `plugin`, by name.
    pub(crate) fn needed_by(
        &self,
        plugin: Entity,
    ) -> Vec<&'a PluginInfo> {
        let Some(at) = self.at(plugin) else {
            return Vec::new();
        };
        let mut found = (0..self.nodes.len())
            .filter(|&other| self.edges[other].contains(&at))
            .map(|other| self.nodes[other].info)
            .collect::<Vec<_>>();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    /// The plugins on that need `plugin`, however indirectly, by
    /// name.
    pub(crate) fn stops(
        &self,
        plugin: Entity,
    ) -> Vec<&'a PluginInfo> {
        self.reach(plugin, |graph, at, other| {
            graph.edges[other].contains(&at) && graph.nodes[other].on
        })
    }

    /// The plugins off that `plugin` needs, however indirectly, by
    /// name.
    pub(crate) fn wakes(
        &self,
        plugin: Entity,
    ) -> Vec<(Entity, &'a PluginInfo)> {
        self.reach(plugin, |graph, at, other| {
            graph.edges[at].contains(&other) && !graph.nodes[other].on
        })
        .into_iter()
        .filter_map(|info| {
            let at = *self.by_id.get(&info.id)?;
            Some((self.nodes[at].entity, info))
        })
        .collect()
    }

    /// The plugins reached from `plugin` along `step`, from one
    /// reached to another, by name.
    fn reach(
        &self,
        plugin: Entity,
        step: impl Fn(&Self, usize, usize) -> bool,
    ) -> Vec<&'a PluginInfo> {
        let Some(start) = self.at(plugin) else {
            return Vec::new();
        };
        let mut reached = vec![false; self.nodes.len()];
        let mut next = vec![start];
        while let Some(at) = next.pop() {
            for (other, seen) in reached.iter_mut().enumerate() {
                if other != start && !*seen && step(self, at, other) {
                    *seen = true;
                    next.push(other);
                }
            }
        }
        let mut found = (0..self.nodes.len())
            .filter(|&at| reached[at])
            .map(|at| self.nodes[at].info)
            .collect::<Vec<_>>();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    /// Every plugin in the order they start.
    pub(crate) fn order(&self) -> Vec<(Entity, StartRank)> {
        let mut order = self
            .nodes
            .iter()
            .zip(&self.depth)
            .map(|(node, &depth)| {
                (
                    node.entity,
                    StartRank {
                        depth,
                        plugin: node.info.id.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        order.sort_by(|(_, a), (_, b)| a.cmp(b));
        order
    }
}

/// Tarjan's search for the groups of plugins that need one another.
struct Tarjan<'e> {
    edges: &'e [Vec<usize>],
    index: Vec<Option<usize>>,
    low: Vec<usize>,
    stack: Vec<usize>,
    on_stack: Vec<bool>,
    next: usize,
    found: Vec<Vec<usize>>,
}

impl Tarjan<'_> {
    /// The groups of `edges`, each after every group it reaches.
    fn groups(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
        let count = edges.len();
        let mut tarjan = Tarjan {
            edges,
            index: vec![None; count],
            low: vec![0; count],
            stack: Vec::new(),
            on_stack: vec![false; count],
            next: 0,
            found: Vec::new(),
        };
        for at in 0..count {
            if tarjan.index[at].is_none() {
                tarjan.visit(at);
            }
        }
        tarjan.found
    }

    fn visit(&mut self, at: usize) {
        self.index[at] = Some(self.next);
        self.low[at] = self.next;
        self.next += 1;
        self.stack.push(at);
        self.on_stack[at] = true;
        let edges = self.edges;
        for &to in &edges[at] {
            match self.index[to] {
                None => {
                    self.visit(to);
                    self.low[at] = self.low[at].min(self.low[to]);
                }
                Some(index) if self.on_stack[to] => {
                    self.low[at] = self.low[at].min(index);
                }
                Some(_) => {}
            }
        }
        if self.index[at] == Some(self.low[at]) {
            let mut group = Vec::new();
            while let Some(top) = self.stack.pop() {
                self.on_stack[top] = false;
                group.push(top);
                if top == at {
                    break;
                }
            }
            self.found.push(group);
        }
    }
}

/// Blocks each plugin switched on whose dependencies cannot be there
/// for it, stopping it, and starts each blocked one again once they
/// are; keeps the [`StartOrder`].
pub fn resolve(
    changed: Query<
        (),
        Or<(
            Changed<PluginInfo>,
            Changed<Enabled>,
            Changed<Starting>,
            Changed<Blocked>,
            Changed<Stopped>,
        )>,
    >,
    mut gone: RemovedComponents<PluginInfo>,
    mut off: RemovedComponents<Enabled>,
    mut started: RemovedComponents<Starting>,
    mut unblocked: RemovedComponents<Blocked>,
    mut restarted: RemovedComponents<Stopped>,
    plugins: Query<State>,
    switched_off: Res<SwitchedOff>,
    mut order: ResMut<StartOrder>,
    mut log: MessageWriter<LogLine>,
    mut commands: Commands,
) {
    let went = gone.read().count()
        + off.read().count()
        + started.read().count()
        + unblocked.read().count()
        + restarted.read().count();
    if changed.is_empty() && went == 0 && !switched_off.is_changed() {
        return;
    }
    let graph = Graph::read(plugins.iter(), &switched_off);
    for (plugin, info, enabled, starting, blocked, _) in &plugins {
        let on = enabled || starting || blocked.is_some();
        let why = graph.why(plugin).filter(|_| on);
        match (blocked, why) {
            (None, Some(why)) => {
                log.write(LogLine::warn(
                    "plugins",
                    format!("{} cannot run: {why}", info.name),
                ));
                commands.entity(plugin).insert(Blocked(why.into()));
                commands.trigger(Disable { entity: plugin });
            }
            (Some(was), Some(why)) if was.0 != why => {
                commands.entity(plugin).insert(Blocked(why.into()));
            }
            (Some(_), None) => {
                log.write(LogLine::info(
                    "plugins",
                    format!("{} has what it needs", info.name),
                ));
                commands.entity(plugin).remove::<Blocked>();
                commands.trigger(Enable { entity: plugin });
            }
            _ => {}
        }
    }
    let now = graph.order();
    if order.0 != now {
        order.0 = now;
    }
}

/// Runs the entry point of each plugin built in whose turn it is.
fn start_builtins(
    plugins: Query<(&PluginInfo, &EntryPoint)>,
    waiting: Query<(), (With<Starting>, Without<Blocked>)>,
    order: Res<StartOrder>,
    budgets: Res<Budgets>,
    mut commands: Commands,
) {
    let ready = order.ready(
        |plugin| waiting.contains(plugin),
        |plugin| plugins.contains(plugin),
    );
    for plugin in ready {
        let Ok((info, point)) = plugins.get(plugin) else {
            continue;
        };
        let mut entry = Entry::default();
        (point.0)(&mut entry);
        commands.entity(plugin).remove::<Starting>();
        let rank = order.rank(plugin, info);
        commands.switch_on(plugin, info, &rank, entry, &budgets);
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::world::World;
    use bevy::time::TimePlugin;
    use hamlet_engine_api::Manifest;
    use hamlet_engine_core::Home;

    use super::*;

    /// An app with a home at `name` in the temporary folder and the
    /// plugins built in `plugins`, each an id and what it needs,
    /// added in that order.
    fn app(
        name: &str,
        plugins: &[(&str, &[(&str, &str)])],
    ) -> (App, Vec<Entity>) {
        let root = std::env::temp_dir().join(format!(
            "hamlet_engine_test_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = App::new();
        app.insert_resource(Home::at(&root)).add_plugins((
            TimePlugin,
            hamlet_engine_core::plugin,
            hamlet_engine_audio::plugin,
            crate::plugin,
        ));
        let added = plugins
            .iter()
            .map(|(id, needs)| {
                let mut manifest = Manifest::new(
                    PluginId::try_from(*id).unwrap(),
                    id,
                    "0.1.0",
                );
                for (needed, requirement) in *needs {
                    manifest.dependencies.insert(
                        (*needed).into(),
                        (*requirement).into(),
                    );
                }
                app.world_mut()
                    .commands()
                    .add_builtin(manifest, |_| {})
                    .unwrap()
            })
            .collect();
        app.update();
        app.update();
        (app, added)
    }

    /// Why `plugin` is blocked, or "on" or "off".
    fn state(world: &World, plugin: Entity) -> String {
        let plugin = world.entity(plugin);
        match plugin.get::<Blocked>() {
            Some(Blocked(why)) => why.clone(),
            None if plugin.contains::<Enabled>() => "on".into(),
            None => "off".into(),
        }
    }

    /// Switches `plugin` as the person does.
    fn switch(world: &mut World, plugin: Entity, on: bool) {
        let id = world.get::<PluginInfo>(plugin).unwrap().id.clone();
        world.resource_mut::<SwitchedOff>().set(&id, !on).unwrap();
        if on {
            world.trigger(Enable { entity: plugin });
        } else {
            world.entity_mut(plugin).remove::<Blocked>();
            world.trigger(Disable { entity: plugin });
        }
    }

    #[test]
    fn a_plugin_runs_only_while_what_it_needs_runs() {
        let (mut app, added) = app(
            "needs",
            &[
                ("test.c", &[("test.b", "^0.1")]),
                ("test.e", &[("test.missing", "^0.1")]),
                ("test.b", &[("test.a", "^0.1")]),
                ("test.d", &[("test.a", "^0.2")]),
                ("test.a", &[]),
            ],
        );
        let [c, e, b, d, a] = added[..] else {
            unreachable!()
        };
        let states = |app: &App| {
            [a, b, c, d, e].map(|plugin| state(app.world(), plugin))
        };
        assert_eq!(
            states(&app),
            [
                "on",
                "on",
                "on",
                "Needs test.a ^0.2, which is 0.1.0",
                "Needs test.missing ^0.1, which isn't installed",
            ]
        );
        let order = app
            .world()
            .resource::<StartOrder>()
            .0
            .iter()
            .map(|(_, rank)| rank.plugin.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            ["test.a", "test.e", "test.b", "test.d", "test.c"]
        );

        switch(app.world_mut(), a, false);
        app.update();
        assert_eq!(
            states(&app)[..3],
            [
                "off",
                "Needs test.a ^0.1, which is switched off",
                "Needs test.b ^0.1, which cannot start",
            ]
        );

        switch(app.world_mut(), a, true);
        app.update();
        app.update();
        assert_eq!(states(&app)[..3], ["on", "on", "on"]);
    }

    #[test]
    fn a_cycle_is_refused_naming_each_plugin_in_it() {
        let (app, added) = app(
            "cycle",
            &[
                ("test.x", &[("test.y", "^0.1")]),
                ("test.y", &[("test.x", "^0.1")]),
                ("test.z", &[("test.x", "^0.1")]),
            ],
        );
        let states = added
            .iter()
            .map(|plugin| state(app.world(), *plugin))
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            [
                "In a cycle of dependencies: test.x, test.y",
                "In a cycle of dependencies: test.x, test.y",
                "Needs test.x ^0.1, which cannot start",
            ]
        );
    }
}
