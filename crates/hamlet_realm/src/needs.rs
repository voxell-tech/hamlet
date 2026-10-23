//! Paper tags at the houses of agents that wait for the chief: what
//! waits, and a check and a cross that answer it.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use bevy::ui::UiSystems;
use hamlet_assets::tint;
use hamlet_client::realm::Mayor;
use hamlet_client::waits::{Choice, ChoiceExt, RaisedBy, Waiting};

use super::label::{self, Lens};
use super::pick::{Footprint, Named, Pressable, Thing};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<TagSettings>()
        .add_systems(Startup, draw_glyphs)
        .add_systems(
            PostUpdate,
            (hang, place).chain().before(UiSystems::Layout),
        );
}

/// The paper tag at a house.
#[derive(
    Resource, Clone, Copy, PartialEq, Debug, serde::Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct TagSettings {
    #[serde(with = "tint")]
    pub paper: Color,
    #[serde(with = "tint")]
    pub ink: Color,
    #[serde(with = "tint")]
    pub needs_you: Color,
    /// Least room between the style test's tag and the view's
    /// edge, in pixels.
    pub margin: f32,
    /// Side of the picture a check or a cross is drawn into, in
    /// pixels.
    pub glyph: usize,
    /// Most characters a tag says of what waits.
    pub line: usize,
    /// Widest a tag gets, in pixels.
    pub width: f32,
    /// Height a tag hangs above the roof, in pixels.
    pub lift: f32,
}

/// The chief answered a tag with its check or its cross.
#[derive(Event, Clone, Copy, Debug)]
pub struct TagAnswered {
    pub item: Entity,
    pub agree: bool,
}

/// The chief clicked a tag's paper, to read it in full.
#[derive(Event, Clone, Copy, Debug)]
pub struct TagOpened {
    pub item: Entity,
}

/// The tag of the oldest thing that waits at one agent.
#[derive(Component, Clone, PartialEq)]
pub struct NeedTag {
    /// The thing that waits.
    item: Entity,
    /// The agent that raised it.
    agent: Entity,
    /// The line the tag carries.
    text: String,
    /// The tag's offers: a check, and a cross.
    check: bool,
    cross: bool,
    /// Number of others that wait at the same agent.
    more: usize,
}

/// The check or the cross of a tag.
#[derive(Component)]
struct Mark {
    item: Entity,
    agree: bool,
}

#[derive(Resource)]
struct Glyphs {
    check: Handle<Image>,
    cross: Handle<Image>,
}

fn draw_glyphs(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    settings: Res<TagSettings>,
) {
    commands.insert_resource(Glyphs {
        check: images.add(glyph(true, settings.glyph)),
        cross: images.add(glyph(false, settings.glyph)),
    });
}

/// A check or a cross drawn into a small image, since the default
/// font has neither.
pub fn glyph(check: bool, n: usize) -> Image {
    let strokes: &[(Vec2, Vec2)] = if check {
        &[
            (Vec2::new(0.2, 0.55), Vec2::new(0.42, 0.76)),
            (Vec2::new(0.42, 0.76), Vec2::new(0.8, 0.26)),
        ]
    } else {
        &[
            (Vec2::new(0.24, 0.24), Vec2::new(0.76, 0.76)),
            (Vec2::new(0.76, 0.24), Vec2::new(0.24, 0.76)),
        ]
    };
    let mut data = vec![255u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let p =
                Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / n as f32;
            let d = strokes
                .iter()
                .map(|(a, b)| {
                    let ab = *b - *a;
                    let t = ((p - *a).dot(ab) / ab.length_squared())
                        .clamp(0.0, 1.0);
                    (*a + ab * t - p).length()
                })
                .fold(f32::MAX, f32::min);
            let alpha = 1.0 - ((d - 0.055) / 0.03).clamp(0.0, 1.0);
            data[(y * n + x) * 4 + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn text(need: &NeedTag) -> String {
    match need.more {
        0 => need.text.clone(),
        n => format!("{}\n(and {n} more)", need.text),
    }
}

/// Keeps one tag for each agent with something waiting.
fn hang(
    mut commands: Commands,
    waiting: Query<(Entity, &Waiting, &RaisedBy)>,
    settings: Res<TagSettings>,
    glyphs: Res<Glyphs>,
    tags: Query<(Entity, &NeedTag)>,
) {
    // Checked every frame: a tag goes with the panel it hangs in
    // when the dock is rearranged, and must come back.
    let mut oldest = waiting.iter().collect::<Vec<_>>();
    oldest.sort_by_key(|(_, waiting, _)| waiting.since);
    let mut wanted = Vec::<NeedTag>::new();
    for (item, waiting, by) in oldest {
        match wanted.iter_mut().find(|w| w.agent == by.0) {
            Some(tag) => tag.more += 1,
            None => wanted.push(NeedTag {
                item,
                agent: by.0,
                text: waiting.line(settings.line),
                check: Choice::quick(&waiting.answers, true)
                    .is_some(),
                cross: Choice::quick(&waiting.answers, false)
                    .is_some(),
                more: 0,
            }),
        }
    }
    for (entity, tag) in &tags {
        if !wanted.contains(tag) {
            commands.entity(entity).try_despawn();
        }
    }
    let TagSettings {
        paper,
        ink,
        needs_you,
        ..
    } = *settings;
    for need in wanted {
        if tags.iter().any(|(_, tag)| *tag == need) {
            continue;
        }
        let item = need.item;
        let mark = |image: &Handle<Image>, fill, colour, agree| {
            (
                Mark { item, agree },
                Node {
                    width: px(28),
                    height: px(28),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(fill),
                BorderColor::all(ink),
                children![(
                    ImageNode {
                        image: image.clone(),
                        color: colour,
                        ..default()
                    },
                    Node {
                        width: px(18),
                        height: px(18),
                        ..default()
                    },
                    Pickable::IGNORE,
                )],
            )
        };
        let mut tag = commands.spawn((
            need.clone(),
            Pressable,
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::axes(px(8), px(5)),
                column_gap: px(8),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                align_items: AlignItems::Center,
                max_width: px(settings.width),
                ..default()
            },
            BackgroundColor(paper),
            BorderColor::all(needs_you),
            BoxShadow::new(
                Color::BLACK.with_alpha(0.55),
                px(0),
                px(6),
                px(0),
                px(18),
            ),
            Visibility::Hidden,
        ));
        tag.observe(
            move |mut click: On<Pointer<Click>>,
                  mut commands: Commands| {
                click.propagate(false);
                if click.button == PointerButton::Primary {
                    commands.trigger(TagOpened { item });
                }
            },
        );
        tag.with_children(|tag| {
            tag.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(1),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (
                        Text::new("NEEDS YOU"),
                        TextFont::from_font_size(11.0),
                        TextColor(needs_you),
                        Pickable::IGNORE,
                    ),
                    (
                        Text::new(text(&need)),
                        TextFont::from_font_size(13.0),
                        TextColor(ink),
                        Pickable::IGNORE,
                    ),
                ],
            ));
            let answer =
                |mut click: On<Pointer<Click>>,
                 marks: Query<&Mark>,
                 mut commands: Commands| {
                    click.propagate(false);
                    if click.button == PointerButton::Primary
                        && let Ok(mark) =
                            marks.get(click.event_target())
                    {
                        commands.trigger(TagAnswered {
                            item: mark.item,
                            agree: mark.agree,
                        });
                    }
                };
            if need.check {
                tag.spawn(mark(&glyphs.check, ink, paper, true))
                    .observe(answer);
            }
            if need.cross {
                tag.spawn(mark(&glyphs.cross, paper, ink, false))
                    .observe(answer);
            }
        });
    }
}

/// Holds each tag over the house of its agent.
fn place(
    settings: Res<TagSettings>,
    lens: Lens,
    houses: Query<
        (Entity, &Thing, &Footprint, &GlobalTransform),
        With<Named>,
    >,
    mayors: Query<(), With<Mayor>>,
    mut tags: Query<(
        &NeedTag,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
    )>,
) {
    let (camera, view) = lens.camera();
    for (tag, mut node, mut visibility, computed) in &mut tags {
        // Only the first hamlet's mayor lives in the cottage.
        let own = Thing::ResidentHouse(tag.agent);
        let hall = mayors.contains(tag.agent);
        let point = houses
            .iter()
            .find(|(_, thing, ..)| **thing == own)
            .or_else(|| {
                houses.iter().find(|(_, thing, ..)| {
                    hall && **thing == Thing::Cottage
                })
            })
            .and_then(|(house, _, room, was)| {
                let top = lens.stands(house, was).translation()
                    + Vec3::Y * (room.height - room.lift);
                camera.world_to_viewport(&view, top).ok()
            });
        let Some(point) = point else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        // The house's place, even past an edge: the panel cuts the
        // tag there.
        label::put(
            &mut node,
            label::above(computed, point, settings.lift),
        );
        visibility.set_if_neq(Visibility::Visible);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::camera::NormalizedRenderTarget;
    use bevy::picking::backend::HitData;
    use bevy::picking::pointer::{Location, PointerId};
    use bevy::prelude::*;
    use hamlet_client::ClientPlugin;
    use hamlet_client::waits::Waiting;
    use hamlet_testing::realm;

    use super::{TagAnswered, TagSettings, plugin};
    use crate::WorldCamera;
    use crate::testing::receive_state;

    #[derive(Resource, Default)]
    struct Answers(Vec<(Entity, bool)>);

    #[test]
    fn a_received_approval_has_a_tag_with_both_quick_answers() {
        let mut app = App::new();
        app.add_plugins((ClientPlugin { server: None }, plugin))
            .init_resource::<Assets<Image>>()
            .init_resource::<Answers>()
            .add_observer(
                |answer: On<TagAnswered>,
                 mut answers: ResMut<Answers>| {
                    answers.0.push((answer.item, answer.agree));
                },
            );
        app.world_mut().spawn((
            WorldCamera,
            Camera::default(),
            Transform::default(),
            GlobalTransform::default(),
        ));
        let mut state = realm::state();
        state.waits.push(realm::wait("run-orchard-check"));
        receive_state(&mut app, state);
        app.update();
        let world = app.world_mut();
        let words = world
            .query::<&Text>()
            .iter(world)
            .map(|text| text.0.as_str())
            .collect::<Vec<_>>();
        assert!(words.contains(&"NEEDS YOU"));
        assert!(
            words
                .iter()
                .any(|text| text.contains("founder wants to run"))
        );
        let wait = world
            .query_filtered::<Entity, With<Waiting>>()
            .single(world)
            .unwrap();
        let set = *world.resource::<TagSettings>();
        let buttons = world
            .query::<(Entity, &BackgroundColor, &Children)>()
            .iter(world)
            .filter(|(_, _, children)| {
                children.iter().any(|child| {
                    world.get::<ImageNode>(child).is_some()
                })
            })
            .map(|(entity, fill, _)| (entity, fill.0 == set.ink))
            .collect::<Vec<_>>();
        assert_eq!(buttons.len(), 2);
        for (button, _) in &buttons {
            world.trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::None {
                        width: 1,
                        height: 1,
                    },
                    position: Vec2::ZERO,
                },
                Click {
                    button: PointerButton::Primary,
                    hit: HitData::new(*button, 0.0, None, None),
                    duration: Duration::ZERO,
                    count: 1,
                },
                *button,
            ));
            world.flush();
        }
        let answers = &world.resource::<Answers>().0;
        assert_eq!(answers.len(), 2);
        assert!(answers.contains(&(wait, true)));
        assert!(answers.contains(&(wait, false)));
    }
}
