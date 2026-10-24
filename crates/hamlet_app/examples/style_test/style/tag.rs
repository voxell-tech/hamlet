//! The paper tag: a UI node tied by a string to a point in the world.

use bevy::prelude::*;
use hamlet_realm::WorldCamera;
use hamlet_realm::needs::{TagSettings, glyph};

use super::demo::Demo;

/// From the knot to the hole in the tag, in pixels.
const STRING: Vec2 = Vec2::new(30.0, -34.0);

/// The point in the world a tag's string is tied to.
#[derive(Resource)]
pub struct Anchor(pub Vec3);

#[derive(Component)]
pub struct Tag;

#[derive(Component)]
pub struct TagString;

/// The line of the tag that carries the request.
#[derive(Component)]
pub struct TagText;

fn choice(
    image: Handle<Image>,
    fill: Color,
    mark: Color,
    ink: Color,
) -> impl Bundle {
    (
        Node {
            width: px(28),
            height: px(28),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(fill),
        BorderColor::all(ink),
        children![(
            ImageNode {
                image,
                color: mark,
                ..default()
            },
            Node {
                width: px(18),
                height: px(18),
                ..default()
            }
        )],
    )
}

pub fn spawn(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    settings: Res<TagSettings>,
) {
    let TagSettings {
        paper,
        ink,
        needs_you,
        glyph: size,
        ..
    } = *settings;
    let string = STRING;
    let (check, cross) = (
        images.add(glyph(true, size)),
        images.add(glyph(false, size)),
    );
    commands.spawn((
        TagString,
        Node {
            position_type: PositionType::Absolute,
            width: px(string.length()),
            height: px(1.5),
            ..default()
        },
        BackgroundColor(ink),
        UiTransform::from_rotation(Rot2::radians(
            string.y.atan2(string.x),
        )),
        Visibility::Hidden,
    ));
    commands.spawn((
        Tag,
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(px(8), px(5)),
            column_gap: px(8),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(3)),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(paper),
        BorderColor::all(ink),
        BoxShadow::new(
            ink.with_alpha(0.35),
            px(1),
            px(2),
            px(0),
            px(3),
        ),
        Visibility::Hidden,
        children![
            // The punched hole the string goes through.
            (
                Node {
                    width: px(8),
                    height: px(8),
                    border: UiRect::all(px(1.5)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BorderColor::all(ink),
                BackgroundColor(ink.with_alpha(0.25)),
            ),
            (
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(1),
                    ..default()
                },
                children![
                    (
                        Text::new("NEEDS YOU"),
                        TextFont::from_font_size(12.0),
                        TextColor(needs_you)
                    ),
                    (
                        TagText,
                        Text::new(""),
                        TextFont::from_font_size(14.0),
                        TextColor(ink)
                    ),
                ],
            ),
            choice(check, ink, paper, ink),
            choice(cross, paper, ink, ink),
        ],
    ));
}

/// Keeps the tag at its anchor's place on screen, and inside the
/// viewport: a tag that would run off an edge slides back in and its
/// string follows.
pub fn place(
    demo: Res<Demo>,
    settings: Res<TagSettings>,
    anchor: Res<Anchor>,
    camera: Single<(Entity, &Camera), With<WorldCamera>>,
    now: TransformHelper,
    mut text: Single<&mut Text, With<TagText>>,
    mut tag: Single<
        (&mut Node, &mut Visibility, &ComputedNode),
        (With<Tag>, Without<TagString>),
    >,
    mut string: Single<
        (&mut Node, &mut Visibility, &mut UiTransform),
        (With<TagString>, Without<Tag>),
    >,
) {
    let margin = settings.margin;
    let (entity, camera) = *camera;
    // The layout runs ahead of the frame's propagation.
    let Ok(at) = now.compute_global_transform(entity) else {
        return;
    };
    let shown = demo.tag.is_some();
    if let Some(request) = &demo.tag
        && text.0 != *request
    {
        text.0.clone_from(request);
    }
    let Ok(knot) = camera.world_to_viewport(&at, anchor.0) else {
        return;
    };
    let visible = if shown {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    let size = tag.2.size() * tag.2.inverse_scale_factor();
    let view = camera
        .logical_viewport_size()
        .unwrap_or(Vec2::splat(f32::MAX));
    // Whole pixels, or the text and the 1 px border blur.
    let wanted = (knot + STRING).round() - Vec2::new(13.0, 19.0);
    let limit = (view - size - margin).max(Vec2::splat(margin));
    let mut corner = wanted.clamp(Vec2::splat(margin), limit);
    // Pushed sideways, it would sit on top of the post: lift it clear
    // of the lamp.
    if (corner.x - wanted.x).abs() > 1.0 {
        corner.y = (knot.y - size.y - 34.0).clamp(margin, limit.y);
    }
    tag.1.set_if_neq(visible);
    put(&mut tag.0, corner);

    let hole = corner + Vec2::new(13.0, 19.0);
    let run = hole - knot;
    let mid = knot + run * 0.5;
    string.1.set_if_neq(visible);
    let width = px(run.length());
    if string.0.width != width {
        string.0.width = width;
    }
    put(
        &mut string.0,
        Vec2::new(mid.x - run.length() * 0.5, mid.y - 0.75),
    );
    let turned =
        UiTransform::from_rotation(Rot2::radians(run.y.atan2(run.x)));
    string.2.set_if_neq(turned);
}

fn put(node: &mut Mut<Node>, corner: Vec2) {
    let (left, top) = (px(corner.x), px(corner.y));
    if (node.left, node.top) != (left, top) {
        node.left = left;
        node.top = top;
    }
}
