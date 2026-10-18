//! The context menu items, the inspector tabs and the settings pages
//! of the plugin in front.

use std::path::Path;

use hamlet_sdk::agents::SelectedAgent;
use hamlet_sdk::command::{RunCommand, Subject, kind};
use hamlet_sdk::context::ContextItems;
use hamlet_sdk::inspector::InspectorSections;
use hamlet_sdk::settings::SettingsPages;
use hamlet_sdk::tree::store::{Drawn, PluginTrees, Shape};

use super::{Harness, SETTLE};
use crate::render::{self, Host};

/// The thing a script names as `kind:name`, such as `agent:Juniper`.
fn subject_of(text: &str) -> Result<Subject, String> {
    let (of, name) = text.split_once(':').ok_or_else(|| {
        format!(
            "`{text}` is not `kind:name`, such as `agent:Juniper`"
        )
    })?;
    if !kind::ALL.contains(&of) {
        return Err(format!(
            "no kind of thing `{of}`; there are {:?}",
            kind::ALL
        ));
    }
    Ok(Subject::new(of, name, name))
}

impl Harness {
    /// Chooses the item `item` of the menu of the thing `subject`,
    /// as `kind:name`, as the chief does: it runs the plugin's
    /// command on the thing. An item shows only while the plugin
    /// holds `ui.context`. `item` is the label of the item, or what
    /// it says of the thing.
    pub fn run_context(
        &mut self,
        subject: &str,
        item: &str,
    ) -> Result<(), String> {
        let subject = subject_of(subject)?;
        let items = self.app.world().resource::<ContextItems>();
        let mine = items
            .of(&subject.kind)
            .filter(|line| line.plugin == self.id)
            .collect::<Vec<_>>();
        let Some(line) = mine.iter().find(|line| {
            line.label == item || line.said(&subject) == item
        }) else {
            return Err(format!(
                "no context item `{item}` for `{}`; there are {:?}",
                subject.kind,
                mine.iter().map(|l| &l.label).collect::<Vec<_>>()
            ));
        };
        let command = line.command.clone();
        self.app
            .world_mut()
            .trigger(RunCommand::on(command, subject));
        self.app.world_mut().flush();
        self.frames(SETTLE);
        Ok(())
    }

    /// Draws the menu of the thing `subject`, as `kind:name`, with
    /// the items of the plugin in front, and saves it as a PNG at
    /// `path`.
    pub fn render_context(
        &mut self,
        subject: &str,
        path: &Path,
    ) -> Result<(), String> {
        let subject = subject_of(subject)?;
        let items = ContextItems(
            self.app
                .world()
                .resource::<ContextItems>()
                .of(&subject.kind)
                .filter(|line| line.plugin == self.id)
                .cloned()
                .collect(),
        );
        if items.0.is_empty() {
            return Err(format!(
                "the plugin has no context item for `{}`",
                subject.kind
            ));
        }
        let host = Host::Context { subject, items };
        render::draw(
            host,
            Drawn::Stopped(String::new()),
            self.size,
            path,
        )
    }

    /// The titles of the tabs of the Inspector's page of an agent
    /// that the plugin in front added and that show.
    pub fn sections(&self) -> Vec<String> {
        let sections =
            self.app.world().resource::<InspectorSections>();
        sections
            .of(kind::AGENT)
            .filter(|line| line.plugin == self.id)
            .map(|line| line.title.clone())
            .collect()
    }

    /// The titles of the pages of the Settings that the plugin in
    /// front added and that show.
    pub fn pages(&self) -> Vec<String> {
        let pages = self.app.world().resource::<SettingsPages>();
        pages
            .0
            .iter()
            .filter(|line| line.plugin == self.id)
            .map(|line| line.title.clone())
            .collect()
    }

    /// Draws the Inspector's page of the agent selected in the world
    /// turned to the tab `title` of the plugin in front, and saves it
    /// as a PNG at `path`.
    pub fn render_section(
        &mut self,
        title: &str,
        path: &Path,
    ) -> Result<(), String> {
        let sections =
            self.app.world().resource::<InspectorSections>();
        let Some(line) = sections.of(kind::AGENT).find(|line| {
            line.plugin == self.id && line.title == title
        }) else {
            return Err(format!(
                "no tab `{title}` shows; there are {:?}",
                self.sections()
            ));
        };
        let (panel, icon) = (line.panel.clone(), line.icon);
        let agent = self
            .app
            .world()
            .resource::<SelectedAgent>()
            .0
            .clone()
            .unwrap_or_else(|| "Agent".into());
        let host = Host::Inspector {
            panel: panel.clone(),
            title: title.into(),
            icon,
            agent,
        };
        let drawn = self.drawn(&panel)?;
        render::draw(host, drawn, self.size, path)
    }

    /// Draws the page `title` of the plugin in front in the Settings,
    /// and saves it as a PNG at `path`.
    pub fn render_page(
        &mut self,
        title: &str,
        path: &Path,
    ) -> Result<(), String> {
        let pages = self.app.world().resource::<SettingsPages>();
        let Some(line) = pages.0.iter().find(|line| {
            line.plugin == self.id && line.title == title
        }) else {
            return Err(format!(
                "no page `{title}` shows; there are {:?}",
                self.pages()
            ));
        };
        let (panel, icon) = (line.panel.clone(), line.icon);
        let host = Host::Settings {
            panel: panel.clone(),
            title: title.into(),
            icon,
        };
        let drawn = self.drawn(&panel)?;
        render::draw(host, drawn, self.size, path)
    }

    /// What the plugin panel with this id in the dock shows now.
    fn drawn(&self, panel: &str) -> Result<Drawn, String> {
        let trees = self.app.world().resource::<PluginTrees>();
        match trees.shape(panel) {
            Shape::Waiting => {
                Err(format!("panel `{panel}` shows nothing"))
            }
            Shape::Tree(_) => trees
                .node(panel, &[])
                .cloned()
                .map(Drawn::Tree)
                .ok_or_else(|| "no tree".into()),
            Shape::Stopped(why) => Ok(Drawn::Stopped(why)),
        }
    }
}
