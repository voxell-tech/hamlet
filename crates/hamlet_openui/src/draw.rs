//! Drawing the panels that are due.

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{Granted, Wanted};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::papers::Papers;
use hamlet_sdk::plugin::{ContributedBy, PluginInfo};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};

use crate::evaluate::evaluate;
use crate::limits::PanelLimits;
use crate::source::Compiled;
use crate::stale::Stale;
use crate::state::{Raised, State};
use crate::wire::Wires;

/// Evaluates each panel that is due and keeps its tree. Every problem
/// is logged with the id of the plugin that owns the panel.
pub(crate) fn draw(
    limits: Res<PanelLimits>,
    agents: Res<AgentList>,
    papers: Res<Papers>,
    mut panels: Query<
        (
            Entity,
            &Panel,
            &Compiled,
            &State,
            &Raised,
            &mut Wires,
            Option<&ContributedBy>,
        ),
        With<Stale>,
    >,
    plugins: Query<(&PluginInfo, &Granted)>,
    mut trees: ResMut<PluginTrees>,
    mut commands: Commands,
) {
    let none = Granted::default();
    for (entity, panel, compiled, state, raised, mut wires, by) in
        &mut panels
    {
        let owner = by.and_then(|by| plugins.get(by.0).ok());
        let granted = owner.map_or(&none, |(_, granted)| granted);
        let drawn = evaluate(
            &compiled.0,
            &state.0,
            &agents,
            &papers,
            granted,
            &limits,
            &raised.0,
        );
        let id =
            owner.map_or(panel.id.as_str(), |(info, _)| &info.id);
        for problem in &drawn.problems {
            warn!("plugin {id}: panel {}: {problem}", panel.id);
        }
        for capability in &drawn.wants {
            if let Some(by) = by {
                commands.trigger(Wanted {
                    plugin: by.0,
                    capability: *capability,
                    reason: String::new(),
                });
            }
        }
        let tree = Drawn::Tree(drawn.root);
        if !trees.shows(&panel.id, &tree) {
            trees.set(&panel.id, tree);
        }
        if wires.0 != drawn.wires {
            wires.0 = drawn.wires;
        }
        commands.entity(entity).remove::<Stale>();
    }
}
