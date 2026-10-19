//! A plugin run headlessly against made-up state.

mod home;
mod surfaces;

use std::path::Path;

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use hamlet_agents_api::tools::{
    AgentTool, AgentToolCalled, TurnObserved,
};
use hamlet_realm_api::overlay::{
    Anchor, Form, Mark, Overlays, Paint,
};
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::{Agent, AgentList, SelectedAgent};
use hamlet_sdk::capability::{
    Asks, Capability, Choice, Decide, Replied,
};
use hamlet_sdk::command::{Commands as Registry, RunCommand};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::keys::chord;
use hamlet_sdk::menu::Menus;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::PluginInfo;
use hamlet_sdk::tree::event::{Act, Interaction};
use hamlet_sdk::tree::id::{dock_id, local_id};
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees, Shape};
use hamlet_wasm::interface::hamlet::plugin::host::Level;
use hamlet_wasm::run::Said;
use hamlet_wasm::stop::Failure;

use self::home::{Scratch, Told};
use crate::render::Host;
use crate::{outline, render};

/// The frames an action runs, so that the plugin sees it, answers it
/// and has its panels drawn again.
const SETTLE: usize = 3;

/// The plugins of a scratch home of their own, installed, with the
/// host that runs them. Everything a plugin may read is made up
/// here. What is done to a plugin is done to the one in front, which
/// is the first until [`Harness::select`] says another.
pub struct Harness {
    app: App,
    /// Every plugin installed, in the order they were given.
    ids: Vec<String>,
    /// The plugin in front.
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
        Self::open_all(&[folder])
    }

    /// Installs the plugins in `folders` into one scratch home, each
    /// as `open` does, and brings them up together. The first is in
    /// front.
    pub fn open_all(folders: &[&Path]) -> Result<Self, String> {
        let scratch = Scratch::new()?;
        let ids = folders
            .iter()
            .map(|folder| home::install(&scratch, folder))
            .collect::<Result<Vec<_>, _>>()?;
        let id = ids.first().ok_or("no plugin was given")?.clone();
        let mut app = home::app(&scratch);
        app.update();
        let said =
            app.world().resource::<Messages<Said>>().get_cursor();
        let mut harness = Self {
            app,
            ids,
            id,
            size: render::SIZE,
            said,
            logs: Vec::new(),
            _home: scratch,
        };
        harness.collect();
        Ok(harness)
    }

    /// Puts the plugin `id` in front: what is done next is done to
    /// it.
    pub fn select(&mut self, id: &str) -> Result<(), String> {
        if !self.ids.iter().any(|known| known == id) {
            return Err(format!(
                "no plugin `{id}`; there are {:?}",
                self.ids
            ));
        }
        self.id = id.into();
        Ok(())
    }

    /// Quits the app and starts it again on the same home: the
    /// plugins are brought up as at a launch, with what the chief
    /// decided and what they kept on disk, and nothing they held in
    /// memory. The notices the chief was given are forgotten.
    pub fn restart(&mut self) {
        let mut app = home::app(&self._home);
        app.update();
        self.said =
            app.world().resource::<Messages<Said>>().get_cursor();
        self.app = app;
        self.frames(SETTLE);
    }

    /// This, drawing pictures of `size` pixels.
    pub fn sized(mut self, size: UVec2) -> Self {
        self.size = size;
        self
    }

    /// The id of the plugin in front.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The ids of the plugins, in the order they were given.
    pub fn ids(&self) -> &[String] {
        &self.ids
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

    /// Commits `text` in the text field or text area `node` of
    /// `panel`.
    pub fn type_text(
        &mut self,
        panel: &str,
        node: &str,
        text: &str,
    ) -> Result<(), String> {
        self.touch(panel, node, "text field or text area", |kind| {
            matches!(kind, Kind::TextField(_) | Kind::TextArea(_))
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

    /// Makes these the papers of the selected agent that the plugin
    /// may read: its notes, its character and the names of its
    /// skills.
    pub fn set_papers(
        &mut self,
        notes: &str,
        character: &str,
        skills: &[String],
    ) {
        *self.app.world_mut().resource_mut::<Papers>() = Papers {
            notes: notes.into(),
            character: character.into(),
            skills: skills
                .iter()
                .map(|name| (name.clone(), String::new()))
                .collect(),
        };
        self.frames(SETTLE);
    }

    /// Makes `survey` the realm a plugin that may read it sees.
    pub fn set_world(&mut self, survey: Survey) {
        *self.app.world_mut().resource_mut::<Survey>() = survey;
        self.frames(SETTLE);
    }

    /// The lines of what the plugin `id` draws over the world, one
    /// for each mark.
    pub fn overlay_of(&self, id: &str) -> Vec<String> {
        let overlays = &self.app.world().resource::<Overlays>().0;
        overlays
            .iter()
            .filter(|overlay| overlay.plugin == id)
            .flat_map(|overlay| overlay.marks.iter().map(marked))
            .collect()
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
        self.tree_of(&self.id, panel)
    }

    /// The tree the panel `panel` of the plugin `id` shows now.
    pub fn tree_of(&self, id: &str, panel: &str) -> Option<Node> {
        let dock = dock_id(id, panel);
        let trees = self.app.world().resource::<PluginTrees>();
        match trees.shape(&dock) {
            Shape::Tree(_) => trees.node(&dock, &[]).cloned(),
            _ => None,
        }
    }

    /// The lines of the tree `panel` shows, one for each node.
    pub fn outline(&self, panel: &str) -> Vec<String> {
        self.outline_of(&self.id, panel)
    }

    /// The lines of the tree the panel `panel` of the plugin `id`
    /// shows.
    pub fn outline_of(&self, id: &str, panel: &str) -> Vec<String> {
        self.tree_of(id, panel)
            .map(|root| outline::lines(&root))
            .unwrap_or_default()
    }

    /// Runs the command `id` as the chief does from a menu or a key:
    /// `plugin/command`, or the name alone for a command of the
    /// plugin in front.
    pub fn run_command(&mut self, id: &str) -> Result<(), String> {
        let id = match id.contains('/') {
            true => id.to_string(),
            false => format!("{}/{id}", self.id),
        };
        let known = &self.app.world().resource::<Registry>().0;
        if !known.iter().any(|command| command.id == id) {
            return Err(format!(
                "no command `{id}`; there are {:?}",
                known.iter().map(|c| &c.id).collect::<Vec<_>>()
            ));
        }
        self.app.world_mut().trigger(RunCommand::of(id));
        self.app.world_mut().flush();
        self.frames(SETTLE);
        Ok(())
    }

    /// Chooses the line of a menu with this label, as the chief does,
    /// which runs its command. A line shows only while the plugin
    /// holds `ui.menu`.
    pub fn run_menu(&mut self, label: &str) -> Result<(), String> {
        let lines = &self.app.world().resource::<Menus>().0;
        let Some(line) =
            lines.iter().find(|line| line.label == label)
        else {
            return Err(format!(
                "no menu line `{label}`; there are {:?}",
                lines.iter().map(|l| &l.label).collect::<Vec<_>>()
            ));
        };
        let command = line.command.clone();
        self.run_command(&command)
    }

    /// Presses a chord such as `Cmd+Shift+N`, as the chief does. It
    /// runs a command only while the plugin holds `ui.keys` and no
    /// other plugin has the chord.
    pub fn press(&mut self, chord: &str) -> Result<(), String> {
        let input = chord::pressed(&chord::parse(chord)?);
        let world = self.app.world_mut();
        *world.resource_mut::<ButtonInput<KeyCode>>() = input;
        self.frames(1);
        let world = self.app.world_mut();
        *world.resource_mut::<ButtonInput<KeyCode>>() =
            ButtonInput::default();
        self.frames(SETTLE);
        Ok(())
    }

    /// Tells the plugins that the turn of `agent` began or ended, as
    /// the app does.
    pub fn observe_turn(&mut self, turn: TurnObserved) {
        self.app.world_mut().trigger(turn);
        self.app.world_mut().flush();
        self.frames(SETTLE);
    }

    /// Whether an agent of the hamlet `hamlet` is offered the tool
    /// `tool` of the plugin in front.
    pub fn offers(&mut self, hamlet: &str, tool: &str) -> bool {
        self.tool(hamlet, tool).is_some()
    }

    /// Has an agent of the hamlet `hamlet` call the tool `tool` of
    /// the plugin in front with `args`, as a model writes them in
    /// JSON, the way the app carries it. The text the plugin
    /// returned, or why there is none.
    pub fn call_tool(
        &mut self,
        hamlet: &str,
        tool: &str,
        args: &str,
    ) -> Result<String, String> {
        let found = self.tool(hamlet, tool).ok_or_else(|| {
            format!(
                "`{tool}` is not offered to the agents of {hamlet}"
            )
        })?;
        let (mut called, mut waiting) = AgentToolCalled::new(
            "Juniper",
            hamlet,
            &found.plugin,
            &found.name,
            found.arguments(args)?,
        );
        self.app.world_mut().trigger_ref(&mut called);
        self.app.world_mut().flush();
        for _ in 0..SETTLE {
            if let Some(answer) = waiting.take() {
                return answer;
            }
            self.frames(1);
        }
        waiting.take().unwrap_or_else(|| {
            Err("the plugin did not answer in time".into())
        })
    }

    fn tool(
        &mut self,
        hamlet: &str,
        tool: &str,
    ) -> Option<AgentTool> {
        let id = self.id.clone();
        let world = self.app.world_mut();
        let mut tools = world.query::<&AgentTool>();
        tools
            .iter(world)
            .find(|found| {
                found.plugin == id
                    && found.name == tool
                    && found.offered_to(hamlet)
            })
            .cloned()
    }

    /// Makes `name`, or none, the agent selected in the world, which
    /// a plugin that may read the agents hears of.
    pub fn set_selected(&mut self, name: Option<&str>) {
        self.app.world_mut().resource_mut::<SelectedAgent>().0 =
            name.map(str::to_string);
        self.frames(SETTLE);
    }

    /// Why the plugin in front stopped, if it did.
    pub fn stopped(&mut self) -> Option<String> {
        self.stopped_of(&self.id.clone())
    }

    /// Why the plugin `id` stopped, if it did.
    pub fn stopped_of(&mut self, id: &str) -> Option<String> {
        let world = self.app.world_mut();
        let mut failed = world.query::<(&PluginInfo, &Failure)>();
        failed
            .iter(world)
            .find(|(info, _)| info.id == id)
            .map(|(_, failure)| failure.0.clone())
    }

    /// The panels the plugin in front declared, by the ids it gave
    /// them.
    pub fn panels(&mut self) -> Vec<String> {
        self.panels_of(&self.id.clone())
    }

    /// The panels the plugin `id` declared, by the ids it gave them.
    pub fn panels_of(&mut self, id: &str) -> Vec<String> {
        let world = self.app.world_mut();
        let mut panels = world.query::<&Panel>();
        let mut found = panels
            .iter(world)
            .filter_map(|panel| local_id(id, &panel.id))
            .map(str::to_string)
            .collect::<Vec<_>>();
        found.sort();
        found
    }

    /// What the plugin logged, as `level: line`, since it was opened.
    pub fn logs(&self) -> &[String] {
        &self.logs
    }

    /// The notices the chief was given in the chat since the
    /// plugins were opened, such as those that tell of an act done
    /// as the chief.
    pub fn told(&self) -> &[String] {
        &self.app.world().resource::<Told>().0
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
        render::draw(Host::Panel(found), drawn, self.size, path)
    }

    fn answer(
        &mut self,
        capability: &str,
        allow: bool,
    ) -> Result<(), String> {
        let (capability, target) = match capability.split_once(':') {
            Some((name, target)) => (name, Some(target.to_string())),
            None => (capability, None),
        };
        let capability = Capability::parse(capability)
            .map_err(|e| e.to_string())?;
        let world = self.app.world_mut();
        let mut asks = world.query::<(&PluginInfo, &Asks)>();
        if !asks.iter(world).any(|(info, asks)| {
            info.id == self.id && asks.names(capability)
        }) {
            return Err(format!(
                "the manifest does not ask for `{}`",
                capability.name()
            ));
        }
        let choice = if allow { Choice::Allow } else { Choice::Deny };
        world.trigger(Decide {
            plugin: self.id.clone(),
            capability,
            target: target.clone(),
            choice,
            this_run: false,
        });
        world.flush();
        world.trigger(Replied {
            plugin: self.id.clone(),
            capability,
            target,
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
            self.logs.push(match self.ids.len() {
                1 => format!("{level}: {}", said.text),
                _ => {
                    format!("{} {level}: {}", said.plugin, said.text)
                }
            });
        }
    }
}

/// One mark as a line: its shape, where it hangs and its paint.
fn marked(mark: &Mark) -> String {
    let at = |anchor: &Anchor, offset: Vec2| {
        let thing = match anchor {
            Anchor::Agent(name) => format!("agent:{name}"),
            Anchor::Hamlet(id) => format!("hamlet:{id}"),
            Anchor::Point(p) => {
                format!("point:{},{},{}", p.x, p.y, p.z)
            }
        };
        format!("{thing} {:+},{:+}", offset.x, offset.y)
    };
    let shape = match &mark.form {
        Form::Rect { w, h } => format!("rect {w}x{h}"),
        Form::Circle { r } => format!("circle {r}"),
        Form::Line { to, end, width } => {
            let to = to.as_ref().unwrap_or(&mark.anchor);
            format!("line {width} to {}", at(to, *end))
        }
        Form::Text { text, size } => format!("text {text:?} {size}"),
    };
    let paint = match &mark.paint {
        Paint::Theme(name) => name.clone(),
        Paint::Rgba([r, g, b, a]) => {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    };
    format!("{shape} at {} {paint}", at(&mark.anchor, mark.offset))
}
