//! Asking the plugins for the panels that are due, and keeping what
//! they return.

use bevy::prelude::*;
use bevy_fynix::dock::DockTree;
use hamlet_openui::Source;
use hamlet_openui::source::Program;
use hamlet_realm_api::survey::Survey;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{
    Contributions, Manifest, PluginInfo, Status,
};
use hamlet_sdk::tree::id::local_id;
use hamlet_sdk::tree::store::{Drawn, PluginTrees};

use crate::budget::{self, Frame};
use crate::instance::Instance;
use crate::limits::Limits;
use crate::reach::{self, Sharing};
use crate::wire::Shown;
use crate::{run, stop};

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

/// Has the panels of a plugin whose grants changed drawn again, since
/// what a tree may hold (a canvas) depends on them.
pub fn regrant(
    mut plugins: Query<
        (&mut Dirty, &Contributions),
        Changed<Granted>,
    >,
    panels: Query<&Panel>,
) {
    for (mut dirty, held) in &mut plugins {
        for panel in panels.iter_many(held.iter()) {
            dirty.mark(&panel.id);
        }
    }
}

/// Whether the panel `dock` is a tab of the dock that another tab is
/// in front of. A panel that is not in the dock may be shown in a
/// page of another, and is drawn.
fn behind(tree: Option<&DockTree>, dock: &str) -> bool {
    let Some(tree) = tree else {
        return false;
    };
    let Some(leaf) = tree.find_leaf_with_window(dock) else {
        return false;
    };
    let tab = tree.leaf(leaf).and_then(|leaf| {
        leaf.tabs()
            .find(|(window, _)| *window == dock)
            .map(|(_, tab)| tab)
    });
    tab.is_some() && tree.active(leaf) != tab
}

/// Whether any plugin has a panel to draw.
pub fn due(plugins: Query<&Dirty>) -> bool {
    plugins.iter().any(|dirty| !dirty.is_empty())
}

/// Calls `panel-view` for each panel that is due, once, and keeps
/// the tree it returns, or hands the panel the program it returns,
/// while the frame has time, the plugin served longest ago first. A
/// panel behind another tab of the dock, and the panels of a plugin
/// that spent its time or fuel of this second, stay due. A plugin
/// whose call fails is marked failed.
pub fn draw(
    limits: Res<Limits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    survey: Res<Survey>,
    mut plugins: Query<(
        Entity,
        &PluginInfo,
        &Manifest,
        &Status,
        &Granted,
        &mut Dirty,
        &mut Instance,
    )>,
    sharing: Sharing,
    panels: Query<(Entity, &Panel, Option<&Source>)>,
    tree: Option<Res<DockTree>>,
    mut trees: ResMut<PluginTrees>,
    mut frame: ResMut<Frame>,
    mut commands: Commands,
) {
    let due = plugins
        .iter()
        .filter_map(|(plugin, _, _, status, _, dirty, instance)| {
            (*status == Status::Running
                && !dirty.is_empty()
                && !instance.spent.over(&limits))
            .then_some((plugin, instance.spent.served))
        })
        .collect::<Vec<_>>();
    for plugin in budget::turns(due) {
        let Ok((
            plugin,
            info,
            manifest,
            _,
            granted,
            mut dirty,
            mut instance,
        )) = plugins.get_mut(plugin)
        else {
            continue;
        };
        instance.see(granted, &agents, &papers);
        instance.see_world(granted, &survey);
        instance.reach(reach::of(manifest, &sharing, &limits));
        for dock in dirty.take() {
            let Some(local) = local_id(&info.id, &dock) else {
                continue;
            };
            if !frame.open(&limits) || behind(tree.as_deref(), &dock)
            {
                dirty.mark(&dock);
                continue;
            }
            let drawn = instance.panel_view(&limits, local);
            frame.charge(instance.spent.took);
            instance.spent.served = frame.number();
            match drawn {
                Ok(drawing) => {
                    drawing.logs.iter().for_each(|line| {
                        run::say(&mut commands, &info.id, line)
                    });
                    run::want(&mut commands, plugin, &drawing.wants);
                    let panel =
                        panels.iter().find(|(_, p, _)| p.id == dock);
                    match drawing.shown {
                        Shown::Tree(root) => {
                            let drawn = Drawn::Tree(root);
                            if !trees.shows(&dock, &drawn) {
                                trees.set(&dock, drawn);
                            }
                            // The panel is no longer drawn from a
                            // program.
                            if let Some((entity, _, Some(_))) = panel
                            {
                                commands
                                    .entity(entity)
                                    .remove::<Program>();
                            }
                        }
                        Shown::Lang(text) => {
                            let text = Source(text);
                            if let Some((entity, _, was)) = panel
                                && was != Some(&text)
                            {
                                commands.entity(entity).insert(text);
                            }
                        }
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
