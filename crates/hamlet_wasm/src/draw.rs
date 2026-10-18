//! Asking the plugins for the panels that are due, and keeping what
//! they return.

use bevy::prelude::*;
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::agents::AgentList;
use crate::grant::Granted;
use crate::instance::Instance;
use crate::limits::Limits;
use crate::trees::{Drawn, PluginTrees};
use crate::{panel, run, stop};

/// On the entity of an installed plugin: the panels, by their id in
/// the dock, that are to be drawn again.
#[derive(Component, Default, Debug)]
pub struct Dirty(Vec<String>);

impl Dirty {
    pub fn of(panels: Vec<String>) -> Self {
        Self(panels)
    }

    pub fn mark(&mut self, panel: &str) {
        if !self.0.iter().any(|due| due == panel) {
            self.0.push(panel.into());
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn take(&mut self) -> Vec<String> {
        std::mem::take(&mut self.0)
    }
}

/// Whether any plugin has a panel to draw.
pub fn due(plugins: Query<&Dirty>) -> bool {
    plugins.iter().any(|dirty| !dirty.is_empty())
}

/// Calls `panel-view` for each panel that is due, once, and keeps
/// the tree it returns. A plugin whose call fails is marked failed.
pub fn draw(
    limits: Res<Limits>,
    agents: Res<AgentList>,
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Status,
        &Granted,
        &mut Dirty,
        &mut Instance,
    )>,
    mut trees: ResMut<PluginTrees>,
    mut commands: Commands,
) {
    for (plugin, info, status, granted, mut dirty, mut instance) in
        &mut plugins
    {
        if *status != Status::Running || dirty.is_empty() {
            continue;
        }
        instance.see(granted, &agents);
        for dock in dirty.take() {
            let Some(local) = panel::local_id(&info.id, &dock) else {
                continue;
            };
            match instance.panel_view(&limits, local) {
                Ok(drawing) => {
                    drawing
                        .logs
                        .iter()
                        .for_each(|line| run::say(&info.id, line));
                    let drawn = Drawn::Tree(drawing.root);
                    if !trees.shows(&dock, &drawn) {
                        trees.set(&dock, drawn);
                    }
                }
                Err(fault) => {
                    stop::fail(
                        &mut commands,
                        plugin,
                        &info.id,
                        &fault,
                    );
                    break;
                }
            }
        }
    }
}
