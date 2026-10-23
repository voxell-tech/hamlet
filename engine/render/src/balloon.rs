//! Balloons of speech and thought over the entities of a scene: a
//! paper balloon with its words, or dots that swell in turn, with a
//! point under it for speech and small rounds for a thought. It pops
//! in as it comes and shrinks away as it goes.

use bevy::color::{Alpha, Color};
use bevy::ecs::change_detection::DetectChangesMut;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::Without;
use bevy::ecs::system::{Commands, Query, Res, SystemParam};
use bevy::ecs::world::World;
use bevy::math::{Rot2, Vec2};
use bevy::picking::Pickable;
use bevy::text::{TextColor, TextFont};
use bevy::time::Time;
use bevy::ui::widget::Text;
use bevy::ui::{
    AlignItems, BackgroundColor, BorderColor, BorderRadius,
    BoxShadow, Display, FlexDirection, Node, UiRect, UiTransform, px,
};
use hamlet_engine_shell::EngineTheme;

use crate::label::MarkOf;
use crate::scene::Scene;

/// On an entity of a scene: the balloon over it.
#[derive(Component, Clone, PartialEq, Debug)]
pub struct Balloon {
    /// Its words, or dots while it has none.
    pub text: String,
    pub thought: bool,
    /// Pixels between the entity and the balloon's point.
    pub gap: f32,
    /// The farthest the camera is from the entity for it to be
    /// shown, or any distance at 0.
    pub far: f32,
}

/// On an entity with a [`Balloon`]: seconds since it came, and since
/// it began to go.
#[derive(Component, Clone, Copy, Default, Debug)]
pub(crate) struct Popping {
    age: f32,
    leaving: Option<f32>,
}

/// On the node of a balloon: its parts.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct BalloonParts {
    balloon: Entity,
    words: Entity,
    dots: Entity,
    point: Entity,
    trail: Entity,
}

/// One of the dots of a balloon with no words, by its place.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Dot(f32);

/// The parts of the balloons shown, to dress as their entities ask.
#[derive(SystemParam)]
pub(crate) struct Parts<'w, 's> {
    parts: Query<'w, 's, &'static BalloonParts>,
    nodes: Query<'w, 's, &'static mut Node, Without<MarkOf>>,
    fills: Query<
        'w,
        's,
        (&'static mut BackgroundColor, &'static mut BorderColor),
        Without<Dot>,
    >,
    inks: Query<'w, 's, (&'static mut Text, &'static mut TextColor)>,
    poses: Query<'w, 's, &'static mut UiTransform>,
}

impl Scene {
    /// Gives the entity `id` `balloon`, or has its balloon go.
    pub(crate) fn balloon(
        &self,
        world: &mut World,
        id: u32,
        balloon: Option<Balloon>,
    ) {
        let Some(entity) = self.entity(id) else {
            return;
        };
        let Ok(mut entity) = world.get_entity_mut(entity) else {
            return;
        };
        match balloon {
            Some(balloon) => {
                let popping = entity
                    .get::<Popping>()
                    .copied()
                    .unwrap_or_default();
                // Words that come while it goes keep it.
                entity.insert((
                    balloon,
                    Popping {
                        leaving: None,
                        ..popping
                    },
                ));
            }
            None => {
                if let Some(mut popping) = entity.get_mut::<Popping>()
                    && popping.leaving.is_none()
                {
                    popping.leaving = Some(0.0);
                }
            }
        }
    }
}

/// The size of a balloon `t` of the way into popping in: past its
/// size a little, then back.
fn pop_size(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + 2.2 * t * t * t + 1.2 * t * t
}

impl Popping {
    /// Its size now, as it comes and goes.
    fn size(&self, theme: &EngineTheme) -> f32 {
        let world = &theme.world;
        let leaving = self.leaving.map_or(0.0, |left| {
            (left / world.balloon_leave).min(1.0)
        });
        (pop_size(self.age / world.balloon_arrive) * (1.0 - leaving))
            .max(0.0)
    }
}

/// Moves each balloon on as it comes and goes, and takes away those
/// gone.
pub(crate) fn pop(
    mut balloons: Query<(Entity, &mut Popping)>,
    theme: Option<Res<EngineTheme>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let leave = theme.map_or(0.0, |theme| theme.world.balloon_leave);
    let dt = time.delta_secs();
    for (entity, mut popping) in &mut balloons {
        popping.age += dt;
        if let Some(left) = &mut popping.leaving {
            *left += dt;
            if *left >= leave {
                commands
                    .entity(entity)
                    .try_remove::<(Balloon, Popping)>();
            }
        }
    }
}

/// The dots of a balloon with no words swell one after another.
pub(crate) fn dots(
    time: Res<Time>,
    theme: Option<Res<EngineTheme>>,
    mut dots: Query<(&Dot, &mut BackgroundColor)>,
) {
    let Some(theme) = theme else {
        return;
    };
    let t = time.elapsed_secs() * 4.5;
    for (dot, mut fill) in &mut dots {
        let lit = (0.5 + 0.5 * (t - dot.0 * 0.9).sin()).powi(2);
        let want =
            theme.colors.text_muted.with_alpha(0.25 + 0.75 * lit);
        if fill.0 != want {
            fill.0 = want;
        }
    }
}

/// A round of `side` pixels.
fn round(
    side: f32,
    fill: Color,
) -> (Node, BackgroundColor, Pickable) {
    (
        Node {
            width: px(side),
            height: px(side),
            border_radius: BorderRadius::MAX,
            ..Node::default()
        },
        BackgroundColor(fill),
        Pickable::IGNORE,
    )
}

/// A balloon with nothing in it yet: its node.
pub(crate) fn spawn(
    world: &mut World,
    theme: &EngineTheme,
) -> Entity {
    let (colors, looks) = (&theme.colors, &theme.world);
    let (paper, ink, edge) =
        (*colors.surface, *colors.text, *colors.border_strong);
    let words = world
        .spawn((
            Text::new(""),
            TextFont::from_font_size(looks.balloon_size),
            TextColor(ink),
            Pickable::IGNORE,
        ))
        .id();
    let dots = world
        .spawn((
            Node {
                column_gap: px(5.0),
                padding: UiRect::axes(px(2.0), px(4.0)),
                ..Node::default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|dots| {
            for at in 0..3 {
                dots.spawn((
                    Dot(at as f32),
                    round(6.0, *colors.text_muted),
                ));
            }
        })
        .id();
    let balloon = world
        .spawn((
            Node {
                max_width: px(looks.balloon_width),
                padding: UiRect::axes(
                    px(looks.balloon_padding_x),
                    px(looks.balloon_padding_y),
                ),
                border: UiRect::all(px(theme.space.border)),
                border_radius: BorderRadius::all(px(theme
                    .space
                    .menu_radius)),
                flex_direction: FlexDirection::Column,
                ..Node::default()
            },
            BackgroundColor(paper),
            BorderColor::all(edge),
            BoxShadow::new(
                *colors.shadow,
                px(0.0),
                px(3.0),
                px(0.0),
                px(10.0),
            ),
            Pickable::IGNORE,
        ))
        .add_children(&[words, dots])
        .id();
    // A square on its corner, half under the balloon, its two sides
    // that show under it framed.
    let point = world
        .spawn((
            Node {
                width: px(12.0),
                height: px(12.0),
                margin: UiRect::top(px(-7.0)),
                border: UiRect {
                    right: px(1.0),
                    bottom: px(1.0),
                    ..UiRect::default()
                },
                ..Node::default()
            },
            BackgroundColor(paper),
            BorderColor::all(edge),
            UiTransform::from_rotation(Rot2::degrees(45.0)),
            Pickable::IGNORE,
        ))
        .id();
    let trail = world
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(3.0),
                margin: UiRect::top(px(3.0)),
                ..Node::default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|trail| {
            trail.spawn(round(8.0, *looks.thought));
            trail.spawn(round(5.0, *looks.thought));
        })
        .id();
    world
        .spawn((
            BalloonParts {
                balloon,
                words,
                dots,
                point,
                trail,
            },
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..Node::default()
            },
            UiTransform::from_scale(Vec2::ZERO),
            Pickable::IGNORE,
        ))
        .add_children(&[balloon, point, trail])
        .id()
}

impl Parts<'_, '_> {
    /// Gives the text `text` the words `words`.
    pub(crate) fn write(&mut self, text: Entity, words: &str) {
        if let Ok((mut text, _)) = self.inks.get_mut(text)
            && text.0 != words
        {
            text.0 = words.to_owned();
        }
    }

    /// Gives the balloon of `node` the words, the look and the size
    /// `balloon` and `popping` ask for.
    pub(crate) fn dress(
        &mut self,
        node: Entity,
        balloon: &Balloon,
        popping: &Popping,
        theme: &EngineTheme,
    ) {
        let Ok(&parts) = self.parts.get(node) else {
            return;
        };
        let (colors, looks) = (&theme.colors, &theme.world);
        let (paper, ink, edge) = if balloon.thought {
            (*looks.thought, *colors.text_muted, *colors.separator)
        } else {
            (*colors.surface, *colors.text, *colors.border_strong)
        };
        if let Ok((mut text, mut colour)) =
            self.inks.get_mut(parts.words)
        {
            if text.0 != balloon.text {
                text.0.clone_from(&balloon.text);
            }
            colour.set_if_neq(TextColor(ink));
        }
        for part in [parts.balloon, parts.point] {
            if let Ok((mut fill, mut frame)) =
                self.fills.get_mut(part)
            {
                fill.set_if_neq(BackgroundColor(paper));
                frame.set_if_neq(BorderColor::all(edge));
            }
        }
        let wordless = balloon.text.is_empty();
        for (part, on) in [
            (parts.words, !wordless),
            (parts.dots, wordless),
            (parts.point, !balloon.thought),
            (parts.trail, balloon.thought),
        ] {
            if let Ok(mut part) = self.nodes.get_mut(part) {
                let display =
                    if on { Display::Flex } else { Display::None };
                if part.display != display {
                    part.display = display;
                }
            }
        }
        if let Ok(mut pose) = self.poses.get_mut(node) {
            let size = Vec2::splat(popping.size(theme));
            if pose.scale != size {
                pose.scale = size;
            }
        }
    }
}
