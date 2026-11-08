//! Text and icons cut by the edge of what clips them fade out toward
//! that edge, letter by letter, instead of ending in a hard edge: a
//! line too long for its row, a field scrolled past its start, a row
//! at the foot of a scroll area. The letters and icons themselves
//! fade, so the fade is right over any fill, glow or hover under
//! them.

use bevy::app::{App, Plugin};
use bevy::ecs::entity::ContainsEntity;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::math::{Rect, Vec2};
use bevy::render::{Extract, ExtractSchedule, RenderApp};
use bevy::text::TextLayoutInfo;
use bevy::ui::CalculatedClip;
use bevy::ui_render::{
    ExtractedUiItem, ExtractedUiNodes, RenderUiSystems,
};

use super::icons::Glyph;

/// How far in from a cutting edge the letters and icons fade, in the
/// heights of their glyphs.
const FADE: f32 = 1.6;

/// Fades the letters of cut text and cut icons where they are drawn.
pub struct CutPlugin;

impl Plugin for CutPlugin {
    fn build(&self, _: &mut App) {}

    // Once every plugin is built, so the render world is there; only
    // a window that draws has one.
    fn finish(&self, app: &mut App) {
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                ExtractSchedule,
                fade_cut.after(RenderUiSystems::ExtractText),
            );
        }
    }
}

/// The edges of `clip` that a text's layout runs past.
#[derive(Clone, Copy, Default)]
struct Cut {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl Cut {
    /// The edges of `clip` that `layout`, at `place` in the clip's
    /// own space, runs past. The layout, not the letters drawn, so a
    /// letter whose ink hangs past where the text starts, as a `j`'s
    /// tail does, cuts nothing.
    fn of(
        layout: Rect,
        clip: Rect,
        place: impl Fn(Vec2) -> Vec2,
    ) -> Self {
        let corners = [
            layout.min,
            layout.max,
            Vec2::new(layout.min.x, layout.max.y),
            Vec2::new(layout.max.x, layout.min.y),
        ]
        .map(place);
        let mut cut = Self::default();
        for at in corners {
            cut.left |= at.x < clip.min.x - 0.5;
            cut.right |= at.x > clip.max.x + 0.5;
            cut.top |= at.y < clip.min.y - 0.5;
            cut.bottom |= at.y > clip.max.y + 0.5;
        }
        cut
    }

    fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }

    /// How much of a glyph drawn over `glyph` is shown, `fade` from
    /// each edge that cuts. Measured from the glyph's far side, so a
    /// glyph an edge runs through is not drawn at all, rather than
    /// drawn faint and cut.
    fn shown(self, clip: Rect, glyph: Rect, fade: f32) -> f32 {
        let within = |gap: f32| (gap / fade).clamp(0.0, 1.0);
        let mut shown = 1.0;
        if self.left {
            shown *= within(glyph.min.x - clip.min.x);
        }
        if self.right {
            shown *= within(clip.max.x - glyph.max.x);
        }
        if self.top {
            shown *= within(glyph.min.y - clip.min.y);
        }
        if self.bottom {
            shown *= within(clip.max.y - glyph.max.y);
        }
        shown
    }
}

/// Lowers the alpha of each letter of a text, and of each icon, that
/// runs past an edge of what clips it, the nearer that edge the
/// lower. Only what is extracted anew this frame is faded, as the
/// rest keeps its fade.
fn fade_cut(
    mut extracted: ResMut<ExtractedUiNodes>,
    texts: Extract<Query<&TextLayoutInfo>>,
) {
    let ExtractedUiNodes { uinodes, changed } = &mut *extracted;
    for main in changed.iter() {
        let Some((_, items)) = uinodes.get_mut(main) else {
            continue;
        };
        // Where the glyphs are placed from, in physical pixels.
        let text = texts
            .get(main.entity())
            .ok()
            .map(|text| Rect::from_corners(Vec2::ZERO, text.size));
        for node in items.values_mut() {
            let Some(CalculatedClip::Rects(clips)) = &node.clip
            else {
                continue;
            };
            let transform = node.transform;
            for clip in clips {
                let place = |at: Vec2| {
                    clip.world_to_clip_local.transform_point2(
                        transform.transform_point2(at),
                    )
                };
                match &mut node.item {
                    ExtractedUiItem::Glyphs { glyphs } => {
                        let Some(layout) = text else {
                            continue;
                        };
                        let tall = glyphs
                            .iter()
                            .map(|glyph| glyph.rect.height())
                            .fold(0.0, f32::max);
                        let cut = Cut::of(layout, clip.rect, place);
                        if tall <= 0.0 || !cut.any() {
                            continue;
                        }
                        for glyph in glyphs.iter_mut() {
                            let half = glyph.rect.half_size();
                            let drawn = Rect::from_corners(
                                place(glyph.translation - half),
                                place(glyph.translation + half),
                            );
                            glyph.color.alpha *= cut.shown(
                                clip.rect,
                                drawn,
                                tall * FADE,
                            );
                        }
                    }
                    // An icon is placed from its middle.
                    ExtractedUiItem::Node { color, rect, .. }
                        if Glyph::is_drawing(node.image) =>
                    {
                        let size = rect.size();
                        let bounds =
                            Rect::from_center_size(Vec2::ZERO, size);
                        let cut = Cut::of(bounds, clip.rect, place);
                        if cut.any() {
                            let drawn = Rect::from_corners(
                                place(bounds.min),
                                place(bounds.max),
                            );
                            color.alpha *= cut.shown(
                                clip.rect,
                                drawn,
                                size.y * FADE,
                            );
                        }
                    }
                    ExtractedUiItem::Node { .. } => {}
                }
            }
        }
    }
}
