//! Which panels are due to be drawn again.

use bevy::prelude::*;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::Granted;
use hamlet_sdk::papers::Papers;

use crate::source::Compiled;

/// On a panel's entity: it is to be drawn again.
#[derive(Component)]
pub struct Stale;

/// Whether any panel is due.
pub(crate) fn due(panels: Query<(), With<Stale>>) -> bool {
    !panels.is_empty()
}

/// Whether what a query answers from has changed.
pub(crate) fn reads_moved(
    agents: Res<AgentList>,
    papers: Res<Papers>,
    granted: Query<(), Changed<Granted>>,
) -> bool {
    agents.is_changed() || papers.is_changed() || !granted.is_empty()
}

/// Has every panel drawn again.
pub(crate) fn all(
    panels: Query<Entity, With<Compiled>>,
    mut commands: Commands,
) {
    for panel in &panels {
        commands.entity(panel).insert(Stale);
    }
}
