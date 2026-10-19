//! What plugins draw over the world: each mark of `Overlays` a node
//! hung off the world's panel and kept where its anchor is seen, and
//! a tag in the panel's corner that names the plugins that draw.

use bevy::math::Rot2;
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy::ui::UiSystems;
use hamlet_realm::label::Lens;
use hamlet_realm::survey::Whereabouts;
use hamlet_realm_api::overlay::{
    Anchor, Form, Mark, Overlays, Paint,
};
use hamlet_sdk::theme::{Palette, Themed};
use hamlet_sdk::tree::canvas::Swatch;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Overlays>()
        .add_systems(
            Update,
            show.run_if(resource_changed::<Overlays>),
        )
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

/// Draws the marks again when a plugin changed what it draws.
fn show(
    mut commands: Commands,
    overlays: Res<Overlays>,
    theme: Themed,
    drawn: Query<Entity, Or<(With<Pinned>, With<Tag>)>>,
) {
    for entity in &drawn {
        commands.entity(entity).try_despawn();
    }
    spawn(&mut commands, &overlays, &theme.palette);
}

/// Spawns a node for each mark of `overlays`, hidden until it is
/// placed, and the tag that names the plugins that drew them.
pub fn spawn(
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
    let by = overlays
        .0
        .iter()
        .map(|overlay| overlay.by.as_str())
        .collect::<Vec<_>>();
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
/// whose anchor is not in sight.
fn place(
    lens: Lens,
    whereabouts: Whereabouts,
    mut marks: Query<(
        &Pinned,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
    )>,
) {
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
    for (pinned, mut node, mut turn, mut visibility) in &mut marks {
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
        visibility.set_if_neq(Visibility::Inherited);
    }
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
