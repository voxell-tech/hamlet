//! A scroll area that keeps to its end as what it holds grows, until
//! the person scrolls up, and again once they are back at its end. At
//! its top it says so, and what is then put above is kept out of
//! view, so the rows read stay where they were.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition};
use bevy_fynix::scroll::ScrollGoal;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, stick);
}

/// On a scroll area that keeps to its end.
#[derive(Component, Clone, Copy, Debug)]
pub struct StickToEnd {
    held: bool,
    /// Where it was left, in pixels from its top.
    was: f32,
    /// How tall what it holds was.
    tall: f32,
    /// The height it last said it was at its top for.
    told: Option<f32>,
    /// What grows next was put above.
    paging: bool,
}

impl Default for StickToEnd {
    fn default() -> Self {
        Self {
            held: true,
            was: 0.0,
            tall: 0.0,
            told: None,
            paging: false,
        }
    }
}

/// Triggered on a [`StickToEnd`] area scrolled to its top, once for
/// each height of what it holds.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct ReachedTop {
    pub entity: Entity,
}

/// The furthest `node` can be scrolled, and how tall what it holds
/// is, in logical pixels.
fn reach(node: &ComputedNode) -> (f32, f32) {
    let scale = node.inverse_scale_factor();
    let seen = node.size().y - node.scrollbar_size.y;
    let tall = node.content_size().y * scale;
    (((node.content_size().y - seen) * scale).max(0.0), tall)
}

pub(crate) fn stick(
    mut areas: Query<(
        Entity,
        &ComputedNode,
        &mut ScrollPosition,
        &mut ScrollGoal,
        &mut StickToEnd,
    )>,
    mut commands: Commands,
) {
    for (area, node, mut at, mut goal, mut stick) in &mut areas {
        let (most, tall) = reach(node);
        let grew = tall - stick.tall;
        if stick.paging && !stick.held && grew > 0.5 {
            at.y += grew;
            // Where the wheel was headed is above what came.
            *goal = ScrollGoal::default();
            stick.paging = false;
        }
        if at.y < stick.was - 0.5 {
            stick.held = false;
        } else if at.y >= most - 1.0 {
            stick.held = true;
        }
        if stick.held {
            stick.paging = false;
            if (at.y - most).abs() > 0.5 {
                at.y = most;
            }
        }
        stick.was = at.y;
        stick.tall = tall;
        if tall > 0.0 && at.y <= 0.5 && stick.told != Some(tall) {
            stick.told = Some(tall);
            stick.paging = true;
            commands.trigger(ReachedTop { entity: area });
        }
    }
}
