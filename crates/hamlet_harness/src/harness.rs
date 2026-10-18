//! A plugin run headlessly against made-up state.

mod home;

use std::path::Path;

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::*;
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::capability::{
    Asks, Capability, Choice, Decide, Replied,
};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::tree::event::{Act, Interaction};
use hamlet_sdk::tree::id::{dock_id, local_id};
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees, Shape};
use hamlet_wasm::interface::hamlet::plugin::host::Level;
use hamlet_wasm::run::Said;
use hamlet_wasm::stop::Failure;

use self::home::Scratch;
use crate::{outline, render};

/// The frames an action runs, so that the plugin sees it, answers it
/// and has its panels drawn again.
const SETTLE: usize = 3;

/// One installed plugin, in a scratch home of its own, with the host
/// that runs it. Everything the plugin may read is made up here.
pub struct Harness {
    app: App,
    id: String,
    size: UVec2,
    said: MessageCursor<Said>,
    logs: Vec<String>,
    // Last, so that the home goes after the app that reads it.
    _home: Scratch,
}

impl Harness {
    /// Installs the plugin in `folder`, a folder with its
    /// `manifest.toml` and `plugin.wasm` or `panel.ui`, into a
    /// scratch home as a chief would who let it in and has
    /// answered none of its asks, and brings it up. The home goes
    /// with the harness.
    pub fn open(folder: &Path) -> Result<Self, String> {
        let scratch = Scratch::new()?;
        let id = home::install(&scratch, folder)?;
        let mut app = home::app(&scratch);
        app.update();
        let said =
            app.world().resource::<Messages<Said>>().get_cursor();
        let mut harness = Self {
            app,
            id,
            size: render::SIZE,
            said,
            logs: Vec::new(),
            _home: scratch,
        };
        harness.collect();
        Ok(harness)
    }

    /// This, drawing pictures of `size` pixels.
    pub fn sized(mut self, size: UVec2) -> Self {
        self.size = size;
        self
    }

    /// The id of the plugin.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Runs one frame: the plugin gets a tick.
    pub fn tick(&mut self) {
        self.frames(1);
    }

    /// Presses the button `node` of `panel`.
    pub fn click(
        &mut self,
        panel: &str,
        node: &str,
    ) -> Result<(), String> {
        self.touch(panel, node, "button", |kind| {
            matches!(kind, Kind::Button(_)).then_some(Act::Click)
        })
    }

    /// Commits `text` in the text field `node` of `panel`.
    pub fn type_text(
        &mut self,
        panel: &str,
        node: &str,
        text: &str,
    ) -> Result<(), String> {
        self.touch(panel, node, "text field", |kind| {
            matches!(kind, Kind::TextField(_))
                .then(|| Act::Typed(text.into()))
        })
    }

    /// Flips the checkbox `node` of `panel`.
    pub fn toggle(
        &mut self,
        panel: &str,
        node: &str,
    ) -> Result<(), String> {
        self.touch(panel, node, "checkbox", |kind| match kind {
            Kind::Checkbox { checked, .. } => {
                Some(Act::Toggled(!checked))
            }
            _ => None,
        })
    }

    /// Makes `agents`, each a hamlet and a name, the agents the
    /// plugin may read. A panel shows them when it is drawn again.
    pub fn set_agents(&mut self, agents: &[(&str, &str)]) {
        self.app.world_mut().resource_mut::<AgentList>().0 = agents
            .iter()
            .map(|(hamlet, name)| Agent {
                name: name.to_string(),
                hamlet: hamlet.to_string(),
            })
            .collect();
        self.frames(SETTLE);
    }

    /// The chief allows `capability`, which the manifest asks for.
    pub fn grant(&mut self, capability: &str) -> Result<(), String> {
        self.answer(capability, true)
    }

    /// The chief refuses `capability`, which the manifest asks for.
    pub fn deny(&mut self, capability: &str) -> Result<(), String> {
        self.answer(capability, false)
    }

    /// The tree `panel`, by the id the plugin gave it, shows now.
    /// None while it shows nothing, or that its plugin stopped.
    pub fn tree(&self, panel: &str) -> Option<Node> {
        let dock = dock_id(&self.id, panel);
        let trees = self.app.world().resource::<PluginTrees>();
        match trees.shape(&dock) {
            Shape::Tree(_) => trees.node(&dock, &[]).cloned(),
            _ => None,
        }
    }

    /// The lines of the tree `panel` shows, one for each node.
    pub fn outline(&self, panel: &str) -> Vec<String> {
        self.tree(panel)
            .map(|root| outline::lines(&root))
            .unwrap_or_default()
    }

    /// Why the plugin stopped, if it did.
    pub fn stopped(&mut self) -> Option<String> {
        let world = self.app.world_mut();
        let mut failed = world.query::<&Failure>();
        failed.iter(world).next().map(|failure| failure.0.clone())
    }

    /// The panels the plugin declared, by the ids it gave them.
    pub fn panels(&mut self) -> Vec<String> {
        let world = self.app.world_mut();
        let mut panels = world.query::<&Panel>();
        let mut found = panels
            .iter(world)
            .filter_map(|panel| local_id(&self.id, &panel.id))
            .map(str::to_string)
            .collect::<Vec<_>>();
        found.sort();
        found
    }

    /// What the plugin logged, as `level: line`, since it was opened.
    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    /// Draws `panel` as the app draws it, in a dock, and saves it as
    /// a PNG at `path`.
    pub fn render(
        &mut self,
        panel: &str,
        path: &Path,
    ) -> Result<(), String> {
        let dock = dock_id(&self.id, panel);
        let world = self.app.world_mut();
        let mut panels = world.query::<&Panel>();
        let found = panels
            .iter(world)
            .find(|found| found.id == dock)
            .cloned()
            .ok_or_else(|| format!("no panel `{panel}`"))?;
        let trees = world.resource::<PluginTrees>();
        let drawn = match trees.shape(&dock) {
            Shape::Waiting => {
                return Err(format!("panel `{panel}` shows nothing"));
            }
            Shape::Tree(_) => trees
                .node(&dock, &[])
                .cloned()
                .map(Drawn::Tree)
                .ok_or("no tree")?,
            Shape::Stopped(why) => Drawn::Stopped(why),
        };
        render::draw(found, drawn, self.size, path)
    }

    fn answer(
        &mut self,
        capability: &str,
        allow: bool,
    ) -> Result<(), String> {
        let capability = Capability::parse(capability)
            .map_err(|e| e.to_string())?;
        let world = self.app.world_mut();
        let mut asks = world.query::<&Asks>();
        if !asks.iter(world).any(|asks| asks.names(capability)) {
            return Err(format!(
                "the manifest does not ask for `{}`",
                capability.name()
            ));
        }
        let choice = if allow { Choice::Allow } else { Choice::Deny };
        world.trigger(Decide {
            plugin: self.id.clone(),
            capability,
            choice,
            this_run: false,
        });
        world.flush();
        world.trigger(Replied {
            plugin: self.id.clone(),
            capability,
            allowed: allow,
        });
        world.flush();
        self.frames(SETTLE);
        Ok(())
    }

    /// Does what `act` makes of the node `node` of `panel`, if it is
    /// a `what`.
    fn touch(
        &mut self,
        panel: &str,
        node: &str,
        what: &str,
        act: impl FnOnce(&Kind) -> Option<Act>,
    ) -> Result<(), String> {
        let Some(root) = self.tree(panel) else {
            return Err(match self.stopped() {
                Some(why) => format!("the plugin stopped: {why}"),
                None => format!("panel `{panel}` shows nothing"),
            });
        };
        let Some(found) = outline::find(&root, node) else {
            return Err(format!(
                "panel `{panel}` has no node `{node}`; it has {:?}",
                outline::ids(&root)
            ));
        };
        let Some(act) = act(&found.kind) else {
            return Err(format!(
                "`{node}` of panel `{panel}` is not a {what}"
            ));
        };
        Interaction::send(
            self.app.world_mut(),
            &dock_id(&self.id, panel),
            node,
            act,
        );
        self.frames(SETTLE);
        Ok(())
    }

    fn frames(&mut self, count: usize) {
        for _ in 0..count {
            self.app.update();
            // A message is gone two frames after it was written.
            self.collect();
        }
    }

    /// Takes what the plugin logged since the last look.
    fn collect(&mut self) {
        let messages = self.app.world().resource::<Messages<Said>>();
        for said in self.said.read(messages) {
            let level = match said.level {
                Level::Info => "info",
                Level::Warn => "warn",
                Level::Error => "error",
            };
            self.logs.push(format!("{level}: {}", said.text));
        }
    }
}
