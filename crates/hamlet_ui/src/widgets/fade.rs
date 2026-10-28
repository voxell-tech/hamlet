//! Soft fades over the edges of a scroll area, each shown while more
//! of what it holds lies past that edge.

use bevy::app::{App, PostUpdate, Update};
use bevy::color::{Alpha, Color, Mix, Oklaba};
use bevy::ecs::change_detection::{DetectChanges, DetectChangesMut};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::lifecycle::Add;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, With, Without};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Query, Res};
use bevy::math::{BVec2, Vec2};
use bevy::picking::Pickable;
use bevy::time::Time;
use bevy::ui::{
    BackgroundGradient, ColorStop, ComputedNode, IgnoreScroll,
    LinearGradient, Node, Overflow, OverflowAxis, PositionType,
    ScrollPosition, UiGlobalTransform, UiSystems, ZIndex, percent,
    px,
};
use bevy_fynix::Theme;
use bevy_fynix::scroll::ScrollGoal;

use crate::platform::Glow;
use crate::theme::HamletTheme;

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(add_fades)
        .add_systems(Update, (default_fade, fade).chain())
        .add_systems(
            PostUpdate,
            keep_fades.before(UiSystems::Prepare),
        );
}

/// On a scroll area: a fade over each edge into `surface` while more
/// of what it holds lies past that edge.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ScrollFade {
    pub surface: FadeSurface,
}

/// The surface under a scroll area.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FadeSurface {
    /// The panel's black, warmed toward the hearth as the [`Glow`]
    /// surface it is on is.
    #[default]
    Panel,
    /// A card.
    Card,
    /// A well, as a field is.
    Well,
}

/// An edge of a scroll area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The fade over `edge` of the scroll area `area`, `level` 0 to 1,
/// and the colour last drawn at its edge.
#[derive(Component, Clone, Copy, Debug)]
struct EdgeFade {
    area: Entity,
    edge: Edge,
    level: f32,
    drawn: Color,
}

impl Edge {
    const ALL: [Self; 4] =
        [Self::Top, Self::Bottom, Self::Left, Self::Right];

    /// A node over this edge, `depth` deep, out of the layout.
    fn node(self, depth: f32) -> Node {
        let (zero, depth) = (px(0.0), px(depth));
        let mut node = Node {
            position_type: PositionType::Absolute,
            ..Node::default()
        };
        match self {
            Self::Top | Self::Bottom => {
                (node.left, node.right, node.height) =
                    (zero, zero, depth);
            }
            Self::Left | Self::Right => {
                (node.top, node.bottom, node.width) =
                    (zero, zero, depth);
            }
        }
        match self {
            Self::Top => node.top = zero,
            Self::Bottom => node.bottom = zero,
            Self::Left => node.left = zero,
            Self::Right => node.right = zero,
        }
        node
    }

    /// Whether more lies past this edge, scrolled `at` of `most`.
    fn more(self, overflow: Overflow, at: Vec2, most: Vec2) -> bool {
        let (axis, at, most) = match self {
            Self::Top | Self::Bottom => (overflow.y, at.y, most.y),
            Self::Left | Self::Right => (overflow.x, at.x, most.x),
        };
        axis == OverflowAxis::Scroll
            && match self {
                Self::Top | Self::Left => at > 0.5,
                Self::Bottom | Self::Right => at < most - 0.5,
            }
    }

    /// `color` at this edge, fading to nothing inward.
    fn gradient(self, color: Color) -> BackgroundGradient {
        let stops = vec![
            ColorStop::new(color, percent(0.0)),
            ColorStop::new(color.with_alpha(0.0), percent(100.0)),
        ];
        BackgroundGradient::from(match self {
            Self::Top => LinearGradient::to_bottom(stops),
            Self::Bottom => LinearGradient::to_top(stops),
            Self::Left => LinearGradient::to_right(stops),
            Self::Right => LinearGradient::to_left(stops),
        })
    }

    /// How far down the window this edge of a node at `at`, `size`
    /// big, lies, in physical pixels.
    fn y(self, at: &UiGlobalTransform, size: Vec2) -> f32 {
        match self {
            Self::Top => at.translation.y - size.y / 2.0,
            Self::Bottom => at.translation.y + size.y / 2.0,
            Self::Left | Self::Right => at.translation.y,
        }
    }
}

/// Moves `value` toward `to` at `rate` per second.
pub(crate) fn ease(value: f32, to: f32, rate: f32, dt: f32) -> f32 {
    let next = to + (value - to) * (-rate * dt).exp();
    if (next - to).abs() < 0.002 { to } else { next }
}

/// Gives each new scroll area a [`ScrollFade`] on the panel unless it
/// was given one on a surface of its own, so that every scroll area
/// fades at its edges by default.
fn default_fade(
    areas: Query<Entity, (Added<ScrollGoal>, Without<ScrollFade>)>,
    mut commands: Commands,
) {
    for area in &areas {
        commands.entity(area).try_insert(ScrollFade::default());
    }
}

/// Puts the fades over the edges of each new [`ScrollFade`] area.
fn add_fades(
    added: On<Add<ScrollFade>>,
    theme: Option<Res<Theme<HamletTheme>>>,
    mut commands: Commands,
) {
    let area = added.entity;
    // The panels are made before the theme has loaded: their fades
    // wait with no depth, and `fade` sizes them once it has.
    let depth = theme.map_or(0.0, |theme| theme.0.space.scroll_fade);
    let fades = Edge::ALL.map(|edge| {
        commands
            .spawn((
                EdgeFade {
                    area,
                    edge,
                    level: 0.0,
                    drawn: Color::NONE,
                },
                edge.node(depth),
                IgnoreScroll(BVec2::TRUE),
                Pickable::IGNORE,
                ZIndex(1),
            ))
            .id()
    });
    // Ahead of the scroll bars, which share their z-index, so the
    // bars are drawn over the fades.
    commands.entity(area).insert_children(0, &fades);
}

/// Eases each fade toward shown while more lies past its edge, in the
/// colour of the surface under its area.
fn fade(
    time: Res<Time>,
    theme: Option<Res<Theme<HamletTheme>>>,
    areas: Query<(
        &Node,
        &ComputedNode,
        &ScrollPosition,
        &UiGlobalTransform,
        &ScrollFade,
    )>,
    shaded: Query<(&ComputedNode, &UiGlobalTransform), With<Glow>>,
    parents: Query<&ChildOf>,
    mut fades: Query<
        (Entity, &mut EdgeFade, &mut Node),
        Without<ScrollFade>,
    >,
    mut commands: Commands,
) {
    let Some(theme) = theme else {
        return;
    };
    let changed = theme.is_changed();
    let theme = &theme.0;
    let rate = theme.motion.interact.recip();
    let dt = time.delta_secs();
    for (entity, mut fade, mut node) in &mut fades {
        let Ok((area, computed, at, place, scroll)) =
            areas.get(fade.area)
        else {
            continue;
        };
        if changed {
            node.set_if_neq(fade.edge.node(theme.space.scroll_fade));
        }
        let most = ((computed.content_size() - computed.size()
            + computed.scrollbar_size)
            * computed.inverse_scale_factor())
        .max(Vec2::ZERO);
        let want =
            f32::from(fade.edge.more(area.overflow, **at, most));
        fade.level = ease(fade.level, want, rate, dt);
        if fade.level <= 0.0 {
            if fade.drawn != Color::NONE {
                fade.drawn = Color::NONE;
                commands
                    .entity(entity)
                    .try_remove::<BackgroundGradient>();
            }
            continue;
        }
        let ground = match scroll.surface {
            FadeSurface::Card => theme.palette.card,
            FadeSurface::Well => theme.palette.input,
            FadeSurface::Panel => {
                let y = fade.edge.y(place, computed.size());
                parents
                    .iter_ancestors(fade.area)
                    .find_map(|panel| shaded.get(panel).ok())
                    .map_or(theme.palette.surface, |(panel, at)| {
                        shade(theme, panel, at, y)
                    })
            }
        };
        let color = ground.with_alpha(ground.alpha() * fade.level);
        if color != fade.drawn {
            fade.drawn = color;
            commands
                .entity(entity)
                .try_insert(fade.edge.gradient(color));
        }
    }
}

/// The colour of a [`Glow`] surface, `computed` big at `at`, `y`
/// physical pixels down the window.
fn shade(
    theme: &HamletTheme,
    computed: &ComputedNode,
    at: &UiGlobalTransform,
    y: f32,
) -> Color {
    let (palette, start) = (&theme.palette, theme.space.bottom_start);
    let tall = computed.size().y;
    let down = (y - at.translation.y) / tall + 0.5;
    let share = ((down - start) / (1.0 - start)).clamp(0.0, 1.0);
    if !share.is_finite() {
        return palette.surface;
    }
    Oklaba::from(palette.surface)
        .mix(&Oklaba::from(palette.bottom_glow), share)
        .into()
}

/// Puts each fade back over its area once something else has taken
/// it from the area's children, or drops it with its area, so it is
/// never drawn at the root of the window.
fn keep_fades(
    fades: Query<(Entity, &EdgeFade, Option<&ChildOf>)>,
    areas: Query<(), With<ScrollFade>>,
    mut commands: Commands,
) {
    for (entity, fade, parent) in &fades {
        if parent.is_some_and(|parent| parent.parent() == fade.area) {
            continue;
        }
        if areas.contains(fade.area) {
            commands.entity(fade.area).insert_children(0, &[entity]);
        } else {
            commands.entity(entity).despawn();
        }
    }
}
