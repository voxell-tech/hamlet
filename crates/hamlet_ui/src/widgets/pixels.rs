//! Letters and lines drawn on whole pixels.
//!
//! The text's layout places each letter between pixels as often as
//! not, and its picture in the font's atlas is then sampled across
//! two of them, which blurs every letter. Each is moved to the
//! nearest whole pixel instead, at most half a pixel off where the
//! layout put it.
//!
//! The layout rounds every edge to a whole pixel, so a line or a
//! border thinner than a pixel, as a one point line is when the
//! panels are drawn small, comes out a pixel wide in one place and
//! gone in the next. Each one rounded away is kept a pixel wide.

use bevy::app::{App, PostUpdate};
use bevy::ecs::query::Changed;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Query;
use bevy::math::Vec2;
use bevy::text::TextLayoutInfo;
use bevy::ui::{ComputedNode, Node, UiSystems, Val};

pub fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        (
            snap.after(UiSystems::PostLayout),
            keep_lines.after(UiSystems::Layout),
        ),
    );
}

/// Puts the corner of each letter laid out anew on a whole pixel. A
/// letter is placed by its middle, half its picture in from that
/// corner.
fn snap(
    mut texts: Query<&mut TextLayoutInfo, Changed<TextLayoutInfo>>,
) {
    for mut text in &mut texts {
        for glyph in &mut text.glyphs {
            let half = glyph.atlas_info.rect.size() / 2.0;
            glyph.position = (glyph.position - half).round() + half;
        }
    }
}

/// Keeps each line and border the layout rounded away a pixel wide.
/// Only one asked for less than a pixel: a longer length laid out as
/// nothing was shut on purpose.
fn keep_lines(
    mut nodes: Query<
        (&Node, &mut ComputedNode),
        Changed<ComputedNode>,
    >,
) {
    for (node, mut laid) in &mut nodes {
        let pixel = laid.inverse_scale_factor;
        let kept = |asked: Val, laid: f32| match asked {
            Val::Px(asked)
                if asked > 0.0 && asked < pixel && laid == 0.0 =>
            {
                1.0
            }
            _ => laid,
        };
        let size = Vec2::new(
            kept(node.width, laid.size.x),
            kept(node.height, laid.size.y),
        );
        let border = &node.border;
        let (near, far) = (
            Vec2::new(
                kept(border.left, laid.border.min_inset.x),
                kept(border.top, laid.border.min_inset.y),
            ),
            Vec2::new(
                kept(border.right, laid.border.max_inset.x),
                kept(border.bottom, laid.border.max_inset.y),
            ),
        );
        if size != laid.size {
            laid.size = size;
        }
        if near != laid.border.min_inset
            || far != laid.border.max_inset
        {
            laid.border.min_inset = near;
            laid.border.max_inset = far;
        }
    }
}
