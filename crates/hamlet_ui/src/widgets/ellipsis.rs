//! A label kept to one line that is cut with an ellipsis where it
//! does not fit, between letters, so the column after it stays clear.

use bevy::app::{App, PostUpdate};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::{Changed, Or};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::Query;
use bevy::text::{
    ComputedTextBlock, PositionedGlyph, TextBrush, TextFont,
    TextLayoutInfo,
};
use bevy::ui::widget::Text;
use bevy::ui::{ComputedNode, Node, UiSystems, Val};
use bevy_fynix::Bevy;
use bevy_fynix::views::Label;
use parley::Layout;

use crate::Cx;
use crate::theme::HamletTheme;

/// Three full stops, which every face has, where an ellipsis
/// character is missing from some.
const DOTS: &str = "...";

/// The share of the room the start of a text cut in its middle takes
/// when the words before its last do not fit in [`MOST_HEAD`].
const HEAD: f32 = 0.4;

/// The most of the room the words before a text's last may take, for
/// the cut to fall in its last word, as at the start of a path.
const MOST_HEAD: f32 = 0.6;

pub(super) fn plugin(app: &mut App) {
    // After the text is laid out and before it is drawn.
    app.add_systems(PostUpdate, fit.after(UiSystems::PostLayout));
}

/// `label` on one line, ending in an ellipsis where its node is
/// narrower than its text. The node shrinks to what it is given.
pub fn one_line(label: Label) -> OneLine {
    OneLine(label, Keep::Start)
}

/// The part of a text a cut keeps.
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub enum Keep {
    #[default]
    Start,
    /// Its start and its end, with the ellipsis between them, for a
    /// text whose end tells it apart, as a path does. The cut falls
    /// in its last word where the words before it fit.
    Ends,
}

/// `text` on one line, its first `most` letters, ending in an
/// ellipsis where it was cut.
pub fn cut(text: &str, most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(most) {
        Some((at, _)) => format!("{}{DOTS}", flat[..at].trim_end()),
        None => flat,
    }
}

/// A [`Label`] that is cut with an ellipsis where it does not fit.
pub struct OneLine(Label, Keep);

impl OneLine {
    /// Keeps `keep` of the text where it is cut.
    pub fn keep(self, keep: Keep) -> Self {
        Self(self.0, keep)
    }
}

impl bevy_fynix::View<Bevy, HamletTheme> for OneLine {
    fn build(self, cx: &mut Cx<'_>) -> Entity {
        let node = self.0.wrap(false).build(cx);
        let mut entity = cx.world.entity_mut(node);
        if let Some(mut ui) = entity.get_mut::<Node>() {
            ui.min_width = Val::Px(0.0);
            ui.max_width = Val::Percent(100.0);
        }
        entity.insert(Ellipsis {
            keep: self.1,
            ..Ellipsis::default()
        });
        node
    }
}

/// The whole text of a [`OneLine`] and where it can be cut.
#[derive(Component, Default)]
struct Ellipsis {
    keep: Keep,
    full: String,
    /// The text last written, `full` or a cut of it.
    shown: String,
    /// Each place `full` can be cut: its byte index and the width of
    /// the text before it, in physical pixels.
    stops: Vec<(usize, f32)>,
    /// The width of the ellipsis, once laid out.
    dots: Option<f32>,
    scale: f32,
    /// It wrote a cut whose layout is still to be made.
    laying: bool,
}

impl Ellipsis {
    fn reset(&mut self, full: String) {
        self.shown.clone_from(&full);
        self.full = full;
        self.stops.clear();
        self.dots = None;
        self.laying = true;
    }

    /// The longest cut of `full`, with its ellipsis, no wider than
    /// `room`, or `full` when it fits.
    fn fitted(&self, room: f32, tall: f32) -> String {
        let whole = self.stops.last().map_or(0.0, |&(_, x)| x);
        if whole <= room + 0.5 {
            return self.full.clone();
        }
        // Before the ellipsis is laid out, a guess wider than it.
        let room = room - self.dots.unwrap_or(tall);
        let within = |most: f32| {
            self.stops.iter().take_while(move |&&(_, x)| x <= most)
        };
        let (head, head_x) = match self.keep {
            Keep::Start => within(room).last(),
            // The words before the last whole where they leave it
            // room, else up to a space, else up to a letter.
            Keep::Ends => {
                let last_word = self.full.rfind(' ').map(|at| at + 1);
                within(room * MOST_HEAD)
                    .find(|&&(end, _)| Some(end) == last_word)
                    .or_else(|| {
                        within(room * HEAD)
                            .filter(|&&(end, _)| {
                                self.full[..end].ends_with(' ')
                            })
                            .last()
                    })
                    .or_else(|| within(room * HEAD).last())
            }
        }
        .copied()
        .unwrap_or_default();
        match self.keep {
            Keep::Start => {
                format!("{}{DOTS}", self.full[..head].trim_end())
            }
            Keep::Ends => {
                let from = whole - (room - head_x);
                let tail = self
                    .stops
                    .iter()
                    .find(|&&(_, x)| x >= from)
                    .map_or(self.full.len(), |&(at, _)| at);
                format!(
                    "{}{DOTS}{}",
                    &self.full[..head],
                    self.full[tail..].trim_start()
                )
            }
        }
    }
}

/// The places `layout`'s first line can be cut, after each cluster.
fn stops(layout: &Layout<TextBrush>) -> Vec<(usize, f32)> {
    let mut stops = Vec::new();
    let mut x = 0.0;
    if let Some(line) = layout.lines().next() {
        for run in line.runs() {
            for cluster in run.clusters() {
                x += cluster.advance();
                stops.push((cluster.text_range().end, x));
            }
        }
    }
    stops
}

/// The width of the ellipsis in `shown`, when `layout` is of `shown`
/// and it has one.
fn dots_width(
    layout: &Layout<TextBrush>,
    shown: &str,
) -> Option<f32> {
    let body = shown.find(DOTS)?;
    let laid = stops(layout);
    if laid.last()?.0 != shown.len() {
        return None;
    }
    let x = |at: usize| match at {
        0 => Some(0.0),
        _ => laid.iter().find(|&&(end, _)| end == at).map(|s| s.1),
    };
    Some(x(body + DOTS.len())? - x(body)?)
}

fn fit(
    mut texts: Query<
        (
            &mut Text,
            &mut Ellipsis,
            Ref<TextFont>,
            &mut Node,
            &ComputedNode,
            &ComputedTextBlock,
            &mut TextLayoutInfo,
        ),
        Or<(
            Changed<Text>,
            Changed<TextFont>,
            Changed<ComputedNode>,
            Changed<TextLayoutInfo>,
        )>,
    >,
) {
    for (
        mut text,
        mut ellipsis,
        font,
        mut ui,
        node,
        block,
        mut info,
    ) in &mut texts
    {
        // Its stops hold for one text in one font at one scale.
        let written = text.0 != ellipsis.shown;
        let mut rewritten = false;
        if written
            || font.is_changed()
            || info.scale_factor != ellipsis.scale
        {
            // Another font or scale measures the whole text again,
            // never the cut on show, which would then stay cut.
            let full = match written {
                true => text.0.clone(),
                false => ellipsis.full.clone(),
            };
            if text.0 != full {
                text.0.clone_from(&full);
                rewritten = true;
            }
            ellipsis.reset(full);
            ellipsis.scale = info.scale_factor;
        }
        // A layout made since is of the text last written, which is
        // laid out in the update it is written in or the next. One
        // made this update is of the cut just put back.
        if info.is_changed() && !rewritten {
            ellipsis.laying = false;
        }
        let layout = block.buffer();
        if !ellipsis.laying {
            // Only a layout of the whole text tells where it can be
            // cut.
            if ellipsis.stops.is_empty()
                && ellipsis.shown == ellipsis.full
            {
                ellipsis.stops = stops(layout);
            }
            if ellipsis.dots.is_none() {
                ellipsis.dots = dots_width(layout, &ellipsis.shown);
            }
        }
        // The node asks for the whole text's width, not its cut's, so
        // it grows again when its parent gives it more room.
        let width = match ellipsis.stops.last() {
            Some(&(_, whole)) => {
                let chrome =
                    node.size().x - node.content_box().width();
                Val::Px(
                    (whole + chrome + 1.0).ceil()
                        * node.inverse_scale_factor(),
                )
            }
            None => Val::Auto,
        };
        if ui.width != width {
            ui.width = width;
        }
        let room = node.content_box().width();
        if !ellipsis.stops.is_empty() && room > 0.0 {
            let fitted = ellipsis.fitted(room, info.size.y);
            if fitted != ellipsis.shown {
                text.0.clone_from(&fitted);
                ellipsis.shown = fitted;
                ellipsis.laying = true;
            }
        }
        // A frame draws the layout of the text before this cut, which
        // can be wider than the room: it is cut at the room's edge
        // until the cut is laid out, so the label never shows blank
        // or past its room.
        if room > 0.0 && info.size.x > room + 0.5 {
            clip(&mut info.glyphs, room);
        }
    }
}

/// Leaves out the glyphs that reach past `room`.
fn clip(glyphs: &mut Vec<PositionedGlyph>, room: f32) {
    let right = |glyph: &PositionedGlyph| {
        glyph.position.x + glyph.atlas_info.rect.width() / 2.0
    };
    if glyphs.iter().any(|glyph| right(glyph) > room) {
        glyphs.retain(|glyph| right(glyph) <= room);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cut_in_the_middle_keeps_both_ends_within_the_room() {
        let full = "grep -n row-0 notes/orchard/1.md";
        let ellipsis = Ellipsis {
            keep: Keep::Ends,
            full: full.to_string(),
            stops: (1..=full.len())
                .map(|at| (at, at as f32 * 10.0))
                .collect(),
            dots: Some(30.0),
            ..Ellipsis::default()
        };
        assert_eq!(
            ellipsis.fitted(200.0, 0.0),
            "grep ...orchard/1.md"
        );
        assert_eq!(
            ellipsis.fitted(280.0, 0.0),
            "grep -n row-0 ...rchard/1.md"
        );
        assert_eq!(ellipsis.fitted(400.0, 0.0), full);
    }

    #[test]
    fn a_label_shows_within_its_room_from_its_first_frame_and_refits()
    {
        use bevy::asset::{AssetApp, AssetPlugin};
        use bevy::camera::{
            Camera, Camera2d, ComputedCameraValues, RenderTargetInfo,
        };
        use bevy::ecs::hierarchy::ChildOf;
        use bevy::image::{Image, TextureAtlasLayout};
        use bevy::math::UVec2;
        use bevy::text::{TextLayout, TextPlugin};
        use bevy::ui::{UiPlugin, px};

        let mut app = App::new();
        app.add_plugins((
            AssetPlugin::default(),
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::input::InputPlugin,
            bevy::picking::PickingPlugin,
            bevy::picking::InteractionPlugin,
            TextPlugin,
            UiPlugin,
            plugin,
        ))
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>();
        crate::widgets::platform::install_fonts(app.world_mut());
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::splat(400),
                        scale_factor: 2.0,
                    }),
                    ..Default::default()
                },
                ..Default::default()
            },
        ));
        let room = app
            .world_mut()
            .spawn(Node {
                width: px(60.0),
                ..Node::default()
            })
            .id();
        let name = app
            .world_mut()
            .spawn((
                Text::new("A name much wider than its room"),
                TextLayout::default().with_no_wrap(),
                Node {
                    min_width: px(0.0),
                    max_width: Val::Percent(100.0),
                    ..Node::default()
                },
                Ellipsis::default(),
                ChildOf(room),
            ))
            .id();

        let drawn = |app: &mut App| {
            app.update();
            let world = app.world();
            let width =
                world.get::<ComputedNode>(name).unwrap().size().x;
            let glyphs =
                &world.get::<TextLayoutInfo>(name).unwrap().glyphs;
            assert!(!glyphs.is_empty());
            assert!(glyphs.iter().all(|glyph| glyph.position.x
                + glyph.atlas_info.rect.width() / 2.0
                <= width));
            world.get::<Text>(name).unwrap().0.clone()
        };
        drawn(&mut app);
        assert!(drawn(&mut app).ends_with(DOTS));

        // It refits when its room widens, and is cut again when it
        // narrows.
        let resize = |app: &mut App, width: f32| {
            app.world_mut().get_mut::<Node>(room).unwrap().width =
                px(width);
            (0..4).map(|_| drawn(app)).last().unwrap()
        };
        assert_eq!(
            resize(&mut app, 1000.0),
            "A name much wider than its room"
        );
        assert!(resize(&mut app, 60.0).ends_with(DOTS));

        // Made bold while it is cut, it still shows its whole text
        // once that fits.
        app.world_mut().get_mut::<TextFont>(name).unwrap().weight =
            bevy::text::FontWeight::BOLD;
        drawn(&mut app);
        assert_eq!(
            resize(&mut app, 1000.0),
            "A name much wider than its room"
        );
    }
}
