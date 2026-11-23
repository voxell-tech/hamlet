//! The colour each agent and chief wears in the world, told to the
//! panels as [`Paints`].

use bevy::prelude::*;
use hamlet_core::client::realm::{HamletAgent, Identity, Kind, Lead};
use hamlet_core::paints::Paints;
use hamlet_core::panels::PanelSystems;
use hamlet_core::protocol::client::AgentKind;

use crate::chief::{Chief, Look};
use crate::hamlet::Housed;
use crate::keeper::Who;
use crate::palette::Palette;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Paints>()
        .add_systems(PreUpdate, paint.in_set(PanelSystems::Follow));
}

fn shown(colour: Vec3) -> Color {
    Color::linear_rgb(colour.x, colour.y, colour.z)
}

fn paint(
    agents: Query<
        (
            Entity,
            Option<&Identity>,
            Has<Lead>,
            Option<&Housed>,
            Option<&Kind>,
        ),
        With<HamletAgent>,
    >,
    chiefs: Query<(&Chief, &Look)>,
    palette: Option<Res<Palette>>,
    mut paints: ResMut<Paints>,
) {
    let fallback = Palette::default();
    let palette = palette.as_deref().unwrap_or(&fallback);
    let agents = agents
        .iter()
        .map(|(agent, id, lead, housed, kind)| {
            let hand =
                kind.is_some_and(|kind| kind.0 == AgentKind::Hand);
            // An agent of a hamlet not yet housed is drawn by its id.
            let unhoused = id.filter(|_| {
                kind.is_some_and(|kind| kind.0 == AgentKind::Hamlet)
            });
            let who = match (lead || hand, housed, unhoused) {
                (true, ..) => Who::lead(),
                (_, Some(housed), _) => {
                    Who::agent(housed.0.0 * 7 + 3)
                }
                (_, None, Some(id)) => {
                    Who::agent(id.0.0 as u32 * 7 + 3)
                }
                (_, None, None) => Who::agent(1),
            };
            (agent, shown(palette.paints[who.paint as usize]))
        })
        .collect();
    let chiefs = chiefs
        .iter()
        .map(|(chief, look)| {
            (chief.name.clone(), shown(look.cloak.colour(palette)))
        })
        .collect();
    let now = Paints { agents, chiefs };
    if *paints != now {
        *paints = now;
    }
}
