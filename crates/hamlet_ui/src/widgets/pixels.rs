//! Letters drawn on whole pixels. The text's layout places each
//! letter between pixels as often as not, and its picture in the
//! font's atlas is then sampled across two of them, which blurs every
//! letter. Each is moved to the nearest whole pixel instead, at most
//! half a pixel off where the layout put it.

use bevy::app::{App, PostUpdate};
use bevy::ecs::query::Changed;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Query;
use bevy::text::TextLayoutInfo;
use bevy::ui::UiSystems;

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, snap.after(UiSystems::PostLayout));
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
