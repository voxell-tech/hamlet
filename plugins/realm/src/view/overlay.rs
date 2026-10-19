//! What plugins draw over the world: each mark of `Overlays` a node
//! hung off the world's panel and kept where its anchor is seen, and
//! a tag in the panel's corner that names the plugins that draw over
//! it or build in it.

use bevy::math::Rot2;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::UiSystems;
use hamlet_realm::label::Lens;
use hamlet_realm::needs::NeedTag;
use hamlet_realm::survey::Whereabouts;
use hamlet_realm_api::overlay::{
    Anchor, Form, Mark, Overlays, Paint,
};
use hamlet_realm_api::scene::PluginScene;
use hamlet_sdk::theme::{Palette, Themed};
use hamlet_sdk::tree::canvas::Swatch;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Overlays>()
        .add_systems(Update, show)
        .add_systems(
            PostUpdate,
            place
                .before(UiSystems::Layout)
                .run_if(any_with_component::<Pinned>),
        );
}

/// A mark a plugin draws over the world, kept where its anchor is
/// seen.
#[derive(Component, Clone, Debug)]
pub struct Pinned(pub Mark);

/// The tag in the corner of the world's panel that names the plugins
/// that draw over it.
#[derive(Component)]
pub struct Tag;

/// Draws the marks and the tag again when a plugin changed what it
/// draws, or which plugins build in the world.
fn show(
    mut commands: Commands,
    overlays: Res<Overlays>,
    scenes: Query<&PluginScene>,
    theme: Themed,
    drawn: Query<Entity, Or<(With<Pinned>, With<Tag>)>>,
    mut named: Local<Vec<String>>,
) {
    let mut by = overlays
        .0
        .iter()
        .map(|overlay| overlay.by.clone())
        .collect::<Vec<_>>();
    for scene in &scenes {
        if !by.contains(&scene.by) {
            by.push(scene.by.clone());
        }
    }
    if !overlays.is_changed() && by == *named {
        return;
    }
    for entity in &drawn {
        commands.entity(entity).try_despawn();
    }
    marks(&mut commands, &overlays, &theme.palette);
    tag(&mut commands, &by, &theme.palette);
    *named = by;
}

/// Spawns a node for each mark of `overlays`, hidden until it is
/// placed, and the tag that names the plugins that drew them.
pub fn spawn(
    commands: &mut Commands,
    overlays: &Overlays,
    palette: &Palette,
) {
    marks(commands, overlays, palette);
    let by = overlays
        .0
        .iter()
        .map(|overlay| overlay.by.clone())
        .collect::<Vec<_>>();
    tag(commands, &by, palette);
}

/// Spawns a node for each mark of `overlays`, hidden until it is
/// placed.
fn marks(
    commands: &mut Commands,
    overlays: &Overlays,
    palette: &Palette,
) {
    for mark in overlays.0.iter().flat_map(|o| &o.marks) {
        let colour = colour(&mark.paint, palette);
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..Node::default()
        };
        if matches!(
            mark.form,
            Form::Circle { .. } | Form::Line { .. }
        ) {
            node.border_radius = BorderRadius::MAX;
        }
        let mut drawn = commands.spawn((
            Pinned(mark.clone()),
            node,
            UiTransform::default(),
            // Under the host's own tags and names.
            ZIndex(-1),
            Pickable::IGNORE,
            Visibility::Hidden,
        ));
        match &mark.form {
            Form::Text { text, size } => drawn.insert((
                Text::new(text.clone()),
                TextFont {
                    font_size: FontSize::Px(*size),
                    ..TextFont::default()
                },
                TextColor(colour),
            )),
            _ => drawn.insert(BackgroundColor(colour)),
        };
    }
}

/// Spawns the tag that names the plugins `by` in the corner of the
/// world's panel, if there are any.
fn tag(commands: &mut Commands, by: &[String], palette: &Palette) {
    if by.is_empty() {
        return;
    }
    commands.spawn((
        Tag,
        Node {
            position_type: PositionType::Absolute,
            left: px(8),
            top: px(8),
            padding: UiRect::axes(px(6), px(2)),
            border: UiRect::all(px(1)),
            ..Node::default()
        },
        BackgroundColor(palette.black.with_alpha(0.85)),
        BorderColor::all(palette.seam),
        // Over the marks, spawned before it, and under the host's
        // tags that ask for the chief.
        ZIndex(-1),
        Pickable::IGNORE,
        children![(
            Text::new(format!("Drawn by {}", by.join(", "))),
            TextFont::from_font_size(11.0),
            TextColor(palette.grey_1),
            Pickable::IGNORE,
        )],
    ));
}

/// The colour `paint` stands for in `palette`.
fn colour(paint: &Paint, palette: &Palette) -> Color {
    match paint {
        Paint::Theme(name) => {
            Swatch::named(name).unwrap_or(Swatch::Ink).colour(palette)
        }
        Paint::Rgba([r, g, b, a]) => Color::srgba_u8(*r, *g, *b, *a),
    }
}

/// Keeps each mark where its anchor is seen this frame, and hides one
/// whose anchor is not in sight or that lies over a tag that asks for
/// the chief.
#[allow(clippy::type_complexity)]
fn place(
    lens: Lens,
    whereabouts: Whereabouts,
    tags: Query<
        (&Node, &ComputedNode, &Visibility),
        (With<NeedTag>, Without<Pinned>),
    >,
    mut marks: Query<(
        &Pinned,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
        &ComputedNode,
    )>,
) {
    let asking = asking(tags.iter());
    let (camera, view) = lens.camera();
    let seen = |anchor: &Anchor| {
        let point = match anchor {
            Anchor::Agent(name) => {
                whereabouts.agent(name).map(|(robot, was)| {
                    lens.stands(robot, was).translation()
                })
            }
            Anchor::Hamlet(id) => {
                whereabouts.hamlet(id).map(|(common, was)| {
                    lens.stands(common, was).translation()
                })
            }
            Anchor::Point(point) => Some(*point),
        }?;
        camera.world_to_viewport(&view, point).ok()
    };
    for (pinned, mut node, mut turn, mut visibility, computed) in
        &mut marks
    {
        let Mark {
            anchor,
            offset,
            form,
            ..
        } = &pinned.0;
        let ends = seen(anchor).and_then(|at| {
            let end = match form {
                Form::Line {
                    to: Some(to), end, ..
                } => seen(to)? + *end,
                Form::Line { end, .. } => at + *end,
                _ => at,
            };
            Some((at + *offset, end))
        });
        let Some((start, end)) = ends else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        put(&mut node, &mut turn, form, start, end);
        let size = computed.size() * computed.inverse_scale_factor();
        let reach = match form {
            Form::Rect { w, h } => Some(Rect::from_corners(
                start,
                start + Vec2::new(*w, *h),
            )),
            Form::Circle { r } => Some(Rect::from_center_half_size(
                start,
                Vec2::splat(*r),
            )),
            Form::Line { width, .. } => Some(
                Rect::from_corners(start, end).inflate(width / 2.0),
            ),
            // Words not yet measured stay hidden until they are.
            Form::Text { .. } => (size != Vec2::ZERO)
                .then(|| Rect::from_corners(start, start + size)),
        };
        let shown =
            reach.is_some_and(|reach| !covers(&asking, reach));
        visibility.set_if_neq(match shown {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        });
    }
}

/// The boxes of the tags that ask for the chief and are shown, in the
/// pixels of the world's panel.
pub(super) fn asking<'a>(
    tags: impl Iterator<
        Item = (&'a Node, &'a ComputedNode, &'a Visibility),
    >,
) -> Vec<Rect> {
    tags.filter(|(.., shown)| **shown != Visibility::Hidden)
        .filter_map(|(node, computed, _)| {
            let (Val::Px(left), Val::Px(top)) = (node.left, node.top)
            else {
                return None;
            };
            let corner = Vec2::new(left, top);
            let size =
                computed.size() * computed.inverse_scale_factor();
            Some(Rect::from_corners(corner, corner + size))
        })
        .collect()
}

/// Whether `reach` lies over any of the boxes `asking`.
pub(super) fn covers(asking: &[Rect], reach: Rect) -> bool {
    asking.iter().any(|tag| !tag.intersect(reach).is_empty())
}

/// Places the node of `form` from `start`, with the far end of a line
/// at `end`, writing only what differs.
pub fn put(
    node: &mut Mut<Node>,
    turn: &mut Mut<UiTransform>,
    form: &Form,
    start: Vec2,
    end: Vec2,
) {
    let (corner, size, angle) = match form {
        Form::Rect { w, h } => (start, Some(Vec2::new(*w, *h)), 0.0),
        Form::Circle { r } => {
            (start - Vec2::splat(*r), Some(Vec2::splat(2.0 * r)), 0.0)
        }
        Form::Line { width, .. } => {
            let along = end - start;
            let length = along.length();
            let middle = (start + end) / 2.0;
            (
                middle - Vec2::new(length, *width) / 2.0,
                Some(Vec2::new(length, *width)),
                along.y.atan2(along.x),
            )
        }
        Form::Text { .. } => (start, None, 0.0),
    };
    let (left, top) = (px(corner.x), px(corner.y));
    if (node.left, node.top) != (left, top) {
        node.left = left;
        node.top = top;
    }
    if let Some(size) = size {
        let (width, height) = (px(size.x), px(size.y));
        if (node.width, node.height) != (width, height) {
            node.width = width;
            node.height = height;
        }
    }
    let rotation = Rot2::radians(angle);
    if turn.rotation != rotation {
        turn.rotation = rotation;
    }
}
