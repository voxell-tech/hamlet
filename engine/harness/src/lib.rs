//! The engine without a window, for testing plugins: load built
//! plugins into a home of their own, answer for the person, run
//! frames, press widgets, read panels as text outlines and assets as
//! bytes, and give the replies to fetches. The network is never
//! reached.

mod outline;
mod steps;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::app::{App, TaskPoolPlugin};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::{With, Without};
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::picking::hover::Hovered as PickingHovered;
use bevy::time::TimePlugin;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button as ButtonBehavior};
use hamlet_engine_api::{
    Action, Asset, AssetId, AssetInfo, CommandId, Failure,
    FetchReply, FetchRequest, PanelId, PluginId, TypeName, WidgetId,
    WidgetTree,
};
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use hamlet_engine_core::{Home, Log, TuningPlugin};
use hamlet_engine_grants::{
    Answer, Petition, Question, answer_install, answer_use,
};
use hamlet_engine_plugins::{
    ContributedBy, Enabled, PluginInfo, Stopped,
};
use hamlet_engine_shell::{
    EngineTheme, Panel, SettingsPage, command,
};
use hamlet_engine_wasm::{
    Drawn, Input, Instance, MANIFEST, NodeOf, Shows, UiRoot, WASM,
};
use serde::Deserialize;

pub use self::outline::{outline, retained_outline};
pub use self::steps::{Step, Steps};

/// Where cargo puts a plugin crate's release build, in its folder.
pub const BUILT: &str = "target/wasm32-wasip2/release/plugin.wasm";

/// How long the harness waits for what a test expects.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessTuning {
    /// Frames to wait at most.
    pub patience: usize,
    /// Milliseconds between two frames, so work off the frame goes
    /// on.
    pub pause: f32,
}

impl Tuning for HarnessTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("harness");
}

/// The engine, without a window, on a home of its own.
pub struct Harness {
    app: App,
    started: bool,
    /// The retained panels whose content was built.
    shown: HashSet<Entity>,
}

impl Harness {
    /// The engine on the home `root`, made empty first. Fetches are
    /// not sent: each waits for [`Harness::reply`].
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let _ = fs::remove_dir_all(&root);
        let mut app = App::new();
        app.insert_resource(Home::at(&root))
            .insert_resource(hamlet_engine_net::Offline)
            .add_plugins((
                TaskPoolPlugin::default(),
                TimePlugin,
                hamlet_engine_core::plugin,
                TuningPlugin::<HarnessTuning>::default(),
                hamlet_engine_plugins::plugin,
                hamlet_engine_grants::plugin,
                hamlet_engine_assets::plugin,
                hamlet_engine_bus::plugin,
                hamlet_engine_net::plugin,
                hamlet_engine_wasm::plugin,
                hamlet_engine_shell::views_plugin,
            ));
        Self {
            app,
            started: false,
            shown: HashSet::new(),
        }
    }

    /// Installs the built plugin in `folder`, before the first frame:
    /// its `plugin.toml`, and its `plugin.wasm` beside it or a
    /// crate's release build at [`BUILT`].
    pub fn install(
        &mut self,
        folder: &Path,
    ) -> Result<PluginId, String> {
        if self.started {
            return Err("plugins are installed before the first \
                        frame"
                .into());
        }
        let manifest = folder.join(MANIFEST);
        let text = fs::read_to_string(&manifest).map_err(|why| {
            format!("{}: {why}", manifest.display())
        })?;
        let id = toml::from_str::<toml::Table>(&text)
            .ok()
            .and_then(|table| {
                table.get("id")?.as_str().map(str::to_owned)
            })
            .and_then(|id| PluginId::try_from(id).ok())
            .ok_or_else(|| {
                format!("{} names no id", manifest.display())
            })?;
        let wasm = [folder.join(WASM), folder.join(BUILT)]
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                format!(
                    "{} has no {WASM}, nor a build at {BUILT}",
                    folder.display()
                )
            })?;
        let home = self.app.world().resource::<Home>().clone();
        let into = home.path("plugins").join(id.as_str());
        let copied = fs::create_dir_all(&into)
            .and_then(|()| fs::copy(&manifest, into.join(MANIFEST)))
            .and_then(|_| fs::copy(&wasm, into.join(WASM)));
        copied.map_err(|why| {
            format!("{id} cannot be installed: {why}")
        })?;
        Ok(id)
    }

    /// Runs one frame.
    pub fn frame(&mut self) {
        self.timed_frame();
    }

    /// Runs one frame: how long its update took, the pause after it
    /// left out.
    pub fn timed_frame(&mut self) -> Duration {
        self.started = true;
        let started = Instant::now();
        self.app.update();
        let took = started.elapsed();
        let pause =
            self.app.world().resource::<HarnessTuning>().pause;
        std::thread::sleep(Duration::from_secs_f32(pause / 1000.0));
        took
    }

    pub fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.frame();
        }
    }

    /// Runs frames until `done` is, for [`HarnessTuning::patience`]
    /// frames at most: whether it came to be.
    pub fn until(
        &mut self,
        mut done: impl FnMut(&mut Self) -> bool,
    ) -> bool {
        let patience =
            self.app.world().resource::<HarnessTuning>().patience;
        for _ in 0..patience {
            if done(self) {
                return true;
            }
            self.frame();
        }
        done(self)
    }

    /// Lets the plugin `plugin` in once it asks, each of its asks
    /// answered `answer`.
    pub fn allow(
        &mut self,
        plugin: &str,
        answer: Answer,
    ) -> Result<(), String> {
        let petition = self.wait_for_petition(plugin, true)?;
        let world = self.app.world_mut();
        if let Some(mut asked) = world.get_mut::<Petition>(petition)
            && let Question::Install { chosen, .. } = &mut asked.0
        {
            chosen.fill(answer);
        }
        answer_install(world, petition, true);
        Ok(())
    }

    /// Answers the next use of `plugin` the person is asked about.
    pub fn answer_use(
        &mut self,
        plugin: &str,
        answer: Answer,
    ) -> Result<(), String> {
        let petition = self.wait_for_petition(plugin, false)?;
        answer_use(self.app.world_mut(), petition, answer);
        Ok(())
    }

    fn wait_for_petition(
        &mut self,
        plugin: &str,
        install: bool,
    ) -> Result<Entity, String> {
        let mut found = None;
        self.until(|harness| {
            let world = harness.app.world_mut();
            found = world
                .query::<(Entity, &Petition, &ChildOf)>()
                .iter(world)
                .filter(|(_, petition, _)| {
                    matches!(petition.0, Question::Install { .. })
                        == install
                })
                .find(|(.., parent)| {
                    world
                        .get::<PluginInfo>(parent.parent())
                        .is_some_and(|info| {
                            info.id.as_str() == plugin
                        })
                })
                .map(|(petition, ..)| petition);
            found.is_some()
        });
        found.ok_or_else(|| format!("{plugin} was never asked about"))
    }

    /// The tree the panel or settings page `panel` (`plugin.id/name`)
    /// shows now, and its entity.
    pub fn tree(
        &mut self,
        panel: &str,
    ) -> Option<(Entity, WidgetTree)> {
        let id = PanelId::try_from(panel).ok()?;
        let world = self.app.world_mut();
        world
            .query::<(Entity, &Drawn, Option<&Panel>, Option<&SettingsPage>)>()
            .iter(world)
            .filter(|(_, _, panel, page)| {
                panel.is_some_and(|panel| panel.id == id)
                    || page.is_some_and(|page| page.id == id)
            })
            .find_map(|(surface, drawn, ..)| match &drawn.shows {
                Shows::Tree(tree) => Some((surface, tree.clone())),
                _ => None,
            })
    }

    /// The panel or settings page `panel` as a text outline, once it
    /// is drawn; a retained panel once its plugin built it.
    pub fn outline(&mut self, panel: &str) -> Option<String> {
        if let Some((_, tree)) = self.tree(panel) {
            return Some(outline(&tree));
        }
        let root = self.retained(panel)?;
        Some(retained_outline(self.app.world(), root))
    }

    /// The root of the retained panel `panel` (`plugin.id/name`),
    /// once its plugin has it. Its content is built as the dock
    /// builds it when it shows it.
    pub fn retained(&mut self, panel: &str) -> Option<Entity> {
        let id = PanelId::try_from(panel).ok()?;
        let world = self.app.world_mut();
        let (surface, content, root, by) = world
            .query::<(Entity, &Panel, Option<&UiRoot>, &ContributedBy)>()
            .iter(world)
            .find(|(_, found, ..)| found.id == id)
            .map(|(surface, found, root, by)| {
                (surface, found.content, root.map(|root| root.0), by.0)
            })?;
        if root.is_some() {
            return root;
        }
        let retained =
            world.get::<Instance>(by).is_some_and(Instance::retained);
        if retained && self.shown.insert(surface) {
            bevy_fynix::mount::<EngineTheme>(world, content(surface));
        }
        None
    }

    /// Presses the button `widget` of the panel `panel`: of a
    /// retained panel, the button whose text is `widget`.
    pub fn click(
        &mut self,
        panel: &str,
        widget: &str,
    ) -> Result<(), String> {
        if self.retained(panel).is_some() {
            let button =
                self.lettered::<ButtonBehavior>(panel, widget)?;
            self.app.world_mut().trigger(Activate { entity: button });
            return Ok(());
        }
        self.act(panel, widget, Action::Click)
    }

    /// Puts the pointer over the node of the retained panel `panel`
    /// whose text is `text`, or takes it away, as the pointer would
    /// for a node whose plugin watches it.
    pub fn hover(
        &mut self,
        panel: &str,
        text: &str,
        on: bool,
    ) -> Result<(), String> {
        let node = self.lettered::<NodeOf>(panel, text)?;
        let mut node = self.app.world_mut().entity_mut(node);
        if !node.contains::<PickingHovered>() {
            return Err(format!("{text:?} is not watched for hover"));
        }
        node.insert(PickingHovered(on));
        Ok(())
    }

    /// The nearest entity with a `C` at or above the text `text` in
    /// the retained panel `panel`, once there is one.
    fn lettered<C: Component>(
        &mut self,
        panel: &str,
        text: &str,
    ) -> Result<Entity, String> {
        let mut found = None;
        self.until(|harness| {
            let Some(root) = harness.retained(panel) else {
                return false;
            };
            let world = harness.app.world_mut();
            found = world
                .query::<(Entity, &Text)>()
                .iter(world)
                .filter(|(_, shown)| shown.0 == text)
                .map(|(entity, _)| entity)
                .collect::<Vec<_>>()
                .into_iter()
                .find_map(|entity| holder::<C>(world, entity, root));
            found.is_some()
        });
        found.ok_or_else(|| format!("{panel} never showed {text:?}"))
    }

    /// Leaves the text `text` in the field `widget` of `panel`.
    pub fn write(
        &mut self,
        panel: &str,
        widget: &str,
        text: &str,
    ) -> Result<(), String> {
        self.act(panel, widget, Action::Text(text.into()))
    }

    /// Switches the toggle `widget` of `panel` to `on`.
    pub fn toggle(
        &mut self,
        panel: &str,
        widget: &str,
        on: bool,
    ) -> Result<(), String> {
        self.act(panel, widget, Action::Toggle(on))
    }

    fn act(
        &mut self,
        panel: &str,
        widget: &str,
        action: Action,
    ) -> Result<(), String> {
        let mut found = None;
        self.until(|harness| {
            found = harness.tree(panel).filter(|(_, tree)| {
                tree.nodes.iter().any(|node| node.id.0 == widget)
            });
            found.is_some()
        });
        let (surface, _) = found.ok_or_else(|| {
            format!("{panel} never showed a widget {widget:?}")
        })?;
        self.app.world_mut().write_message(Input {
            surface,
            widget: WidgetId(widget.into()),
            action,
        });
        Ok(())
    }

    /// Runs the command `id` (`plugin.id/name`), as its menu does.
    pub fn run(&mut self, id: &str) -> Result<(), String> {
        let id =
            CommandId::try_from(id).map_err(|why| why.to_string())?;
        command::run(self.app.world_mut(), &id);
        Ok(())
    }

    /// The assets of the type `kind`.
    pub fn assets(&self, kind: &str) -> Vec<AssetInfo> {
        let Ok(kind) = TypeName::try_from(kind) else {
            return Vec::new();
        };
        let store = self
            .app
            .world()
            .resource::<hamlet_engine_assets::AssetStore>();
        store.of_type(&kind).cloned().collect()
    }

    pub fn read_asset(&self, id: AssetId) -> Result<Asset, Failure> {
        let world = self.app.world();
        let most = world
            .resource::<hamlet_engine_core::Budgets>()
            .asset_bytes;
        world
            .resource::<hamlet_engine_assets::AssetStore>()
            .read(id, most)
    }

    /// The fetches waiting for a reply.
    pub fn fetches(&mut self) -> Vec<(Entity, FetchRequest)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &hamlet_engine_net::Fetch)>()
            .iter(world)
            .map(|(fetch, made)| (fetch, made.request.clone()))
            .collect()
    }

    /// Replies to the first fetch of `url` with `reply`, once one is
    /// made.
    pub fn reply(
        &mut self,
        url: &str,
        reply: Result<FetchReply, Failure>,
    ) -> Result<(), String> {
        let mut found = None;
        self.until(|harness| {
            found = harness
                .fetches()
                .into_iter()
                .find(|(_, request)| request.url == url)
                .map(|(fetch, _)| fetch);
            found.is_some()
        });
        let fetch = found
            .ok_or_else(|| format!("{url} was never fetched"))?;
        hamlet_engine_net::answer(self.app.world_mut(), fetch, reply);
        Ok(())
    }

    /// The lines of the log kept, oldest first, each as its source
    /// and its text.
    pub fn log(&self) -> Vec<String> {
        self.app
            .world()
            .resource::<Log>()
            .lines()
            .map(|(_, line)| {
                format!("{}: {}", line.source, line.text)
            })
            .collect()
    }

    /// Whether the plugin `plugin` is on and not stopped.
    pub fn running(&mut self, plugin: &str) -> bool {
        let world = self.app.world_mut();
        world
            .query_filtered::<&PluginInfo, (With<Enabled>, Without<Stopped>)>()
            .iter(world)
            .any(|info| info.id.as_str() == plugin)
    }

    /// Why the plugin `plugin` was stopped, if it was.
    pub fn stopped(&mut self, plugin: &str) -> Option<String> {
        let world = self.app.world_mut();
        world
            .query::<(&PluginInfo, &Stopped)>()
            .iter(world)
            .find(|(info, _)| info.id.as_str() == plugin)
            .map(|(_, stopped)| stopped.0.clone())
    }

    /// The world, for what the harness has no call for.
    pub fn world(&mut self) -> &mut World {
        self.app.world_mut()
    }
}

/// The nearest entity with a `C` from `entity` up to `root`, if
/// `entity` is under `root`.
fn holder<C: Component>(
    world: &World,
    entity: Entity,
    root: Entity,
) -> Option<Entity> {
    let mut found = None;
    let mut at = entity;
    while at != root {
        if found.is_none() && world.get::<C>(at).is_some() {
            found = Some(at);
        }
        at = world.get::<ChildOf>(at)?.parent();
    }
    found
}
