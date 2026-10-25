//! The feel of a control: its warmth under the pointer, its give
//! under a press, the flame in its glass, the light it spills and
//! the sparks of work done.

use std::collections::HashMap;

use bevy::color::{Alpha, Color, Mix};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Changed, Has, With, Without};
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::math::{Rect, Vec2};
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::text::EditableText;
use bevy::time::Time;
use bevy::ui::{
    BackgroundColor, BackgroundGradient, BorderRadius, BoxShadow,
    CalculatedClip, ComputedNode, CornerRadius, GlobalZIndex, Node,
    PositionType, Pressed, RadialGradient, RadialGradientShape,
    UiGlobalTransform, UiPosition, UiTransform, Val2, percent, px,
};
use bevy::ui_widgets::{Activate, Button};
use bevy_fynix::Focused;
use hamlet_realm::sound::{Cue, Sound};

use super::glow::{GlowLayer, GlowPanel, layer_of};
use super::{FeelSettings, MagicColors, Mood, ease, pool, waver};
use crate::sounds;
use crate::theme::Themed;

/// A button's state of feel, each 0 to 1.
#[derive(Component, Default)]
pub(super) struct Feel {
    heat: f32,
    press: f32,
    pub(super) pulse: f32,
    shake: f32,
    /// Its light, the reach of its spill in pixels, and the side its
    /// flame leans to, -1 to 1.
    glow: f32,
    reach: f32,
    lean: f32,
}

/// A spark rising from where the good action was done.
#[derive(Component)]
pub(super) struct Ember {
    from: Vec2,
    /// Pixels it drifts sideways over its life, and the place in its
    /// sway it starts at.
    drift: f32,
    sway: f32,
    /// Seconds since it lit. Below zero it has yet to.
    age: f32,
    life: f32,
}

pub(super) fn arm(
    mut commands: Commands,
    buttons: Query<Entity, (With<Button>, Without<Feel>)>,
    fields: Query<&ChildOf, Added<EditableText>>,
) {
    for field in &fields {
        commands
            .entity(field.parent())
            .try_insert((Feel::default(), Mood::Field));
    }
    for button in &buttons {
        commands
            .entity(button)
            .try_insert((Feel::default(), UiTransform::IDENTITY));
    }
}

type Entered = (Changed<Hovered>, With<Feel>);

pub(super) fn hover_tick(
    buttons: Query<(&Hovered, Option<&Mood>), Entered>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    let heard = |(hovered, mood): (&Hovered, Option<&Mood>)| {
        hovered.0 && mood.is_some_and(|mood| mood.has_tick())
    };
    if buttons.iter().any(heard) {
        sounds::write(&mut cues, &mut turn, Sound::Tick);
    }
}

pub(super) fn press_tap(
    pressed: Query<(), (Added<Pressed>, With<Feel>)>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    if !pressed.is_empty() {
        sounds::write(&mut cues, &mut turn, Sound::Tap);
    }
}

/// A number from 0 to 1 that is the same for the same `seed`.
fn chance(seed: u32) -> f32 {
    let state =
        seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28) + 4)) ^ state)
        .wrapping_mul(277_803_737);
    ((word >> 22) ^ word) as f32 / u32::MAX as f32
}

/// Lights the sparks of a good action done on a control that stands
/// at `centre` and is `size` across, in logical pixels.
fn kindle(
    commands: &mut Commands,
    set: &FeelSettings,
    centre: Vec2,
    size: Vec2,
    seed: u32,
) {
    for spark in 0..set.embers {
        let roll = |salt: u32| {
            chance(
                seed.wrapping_mul(31).wrapping_add(spark * 7 + salt),
            )
        };
        let from = centre
            + Vec2::new(
                (roll(0) - 0.5) * size.x * 0.8,
                -size.y * 0.5 + roll(1) * 4.0,
            );
        let side = set.ember_size * (0.7 + 0.5 * roll(2));
        commands.spawn((
            Ember {
                from,
                drift: (roll(3) - 0.5) * 20.0,
                sway: roll(4) * std::f32::consts::TAU,
                age: -0.14 * roll(5),
                life: set.ember_life * (0.6 + 0.4 * roll(6)),
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(from.x),
                top: px(from.y),
                width: px(side),
                height: px(side),
                border_radius: BorderRadius::MAX,
                ..Node::default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
            GlobalZIndex(i32::MAX - 1),
        ));
    }
}

type Answering<'a> = (
    &'a mut Feel,
    Option<&'a Mood>,
    Option<&'a ComputedNode>,
    Option<&'a UiGlobalTransform>,
);

/// A button that has done its work pulses, with sparks if it was the
/// good action, or shakes if what it did was to refuse.
pub(super) fn answer(
    activated: On<Activate>,
    mut commands: Commands,
    set: Res<FeelSettings>,
    mut buttons: Query<Answering>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    let Ok((mut feel, mood, node, at)) =
        buttons.get_mut(activated.entity)
    else {
        return;
    };
    match mood.copied().unwrap_or_default() {
        Mood::Field => {}
        Mood::Quiet | Mood::Plain | Mood::Ink => feel.pulse = 1.0,
        Mood::Good => {
            feel.pulse = 1.0;
            sounds::write(&mut cues, &mut turn, Sound::Confirm);
            if let Some((node, at)) = node.zip(at) {
                let scale = node.inverse_scale_factor;
                kindle(
                    &mut commands,
                    &set,
                    at.translation * scale,
                    node.size * scale,
                    activated.entity.index_u32() ^ *turn,
                );
            }
        }
        Mood::Bad => {
            feel.shake = 1.0;
            sounds::write(&mut cues, &mut turn, Sound::Deny);
        }
    }
}

type Felt<'a> = (
    Entity,
    &'a mut Feel,
    Option<&'a mut UiTransform>,
    Has<Focused>,
    Option<&'a Hovered>,
    Has<Pressed>,
    Option<&'a Mood>,
);

pub(super) fn feel(
    time: Res<Time>,
    set: Res<FeelSettings>,
    mut buttons: Query<Felt>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (id, mut feel, transform, focused, hovered, held, mood) in
        &mut buttons
    {
        let mood = mood.copied().unwrap_or_default();
        let over = if mood == Mood::Field {
            focused
        } else {
            hovered.is_some_and(|h| h.0)
        };
        let heat = ease(feel.heat, f32::from(over), set.warm, dt);
        let press = ease(feel.press, f32::from(held), set.give, dt);
        let pulse = ease(feel.pulse, 0.0, set.fade, dt);
        let shake = ease(feel.shake, 0.0, set.fade, dt);
        let still = heat == feel.heat
            && press == feel.press
            && pulse == feel.pulse
            && shake == feel.shake;
        // Its light at rest and under the pointer or the caret, and
        // the light its work adds.
        let (rest, near, done) = match mood {
            Mood::Quiet => (0.0, 0.0, 0.0),
            Mood::Field => (0.0, set.lit_field, 0.0),
            Mood::Plain | Mood::Ink => {
                (0.0, set.lit_plain, set.kindle * pulse)
            }
            Mood::Good => (set.banked, set.lit_good, pulse),
            Mood::Bad => (0.0, set.lit_bad, set.kindle * shake),
        };
        let steady = rest + (near - rest) * heat;
        if still && steady == 0.0 && feel.glow == 0.0 {
            continue;
        }
        // A pulse swells and falls back as it dies away.
        let swell = 4.0 * pulse * (1.0 - pulse);
        let scale = 1.0 - set.squash * press + set.pop * swell;
        let slide = shake * set.shake * (now * set.rattle).sin();
        let want = UiTransform {
            translation: Val2::new(px(slide), px(set.sink * press)),
            scale: Vec2::splat(scale),
            ..UiTransform::IDENTITY
        };
        if let Some(mut transform) = transform
            && *transform != want
        {
            *transform = want;
        }
        // The hand covers the lamp, and the work done turns it up.
        let covered = steady * (1.0 - set.cover * press);
        let lit = covered + (1.0 - covered) * done;
        let seed = id.index_u32() as f32 * 1.618;
        let flame = 1.0 + set.waver * waver(now, seed);
        *feel = Feel {
            heat,
            press,
            pulse,
            shake,
            glow: (lit * flame).clamp(0.0, 1.0),
            reach: set.reach + set.burst * pulse.max(shake),
            lean: (now * 1.1 + seed).sin(),
        };
    }
}

/// The colour of a control's light and of the heart of its flame.
fn hues(mood: Mood, colors: &MagicColors) -> (Color, Color) {
    match mood {
        Mood::Bad => (colors.coal, colors.ember),
        _ => (colors.lamp, colors.flame),
    }
}

type Lamped<'a> =
    (Entity, &'a Feel, &'a Mood, Option<&'a BackgroundGradient>);

/// Keeps the flame in the glass of every control that has one: a
/// pool of light from low in the glass, leaning to one side.
pub(super) fn light(
    mut commands: Commands,
    theme: Themed,
    controls: Query<Lamped>,
) {
    let colors = MagicColors::of(&theme);
    for (control, feel, mood, now) in &controls {
        // Lettering has no glass for a flame to burn in.
        if matches!(*mood, Mood::Quiet | Mood::Ink) {
            continue;
        }
        // Only the good action shows its glass while it is out.
        let glass = *mood == Mood::Good;
        if feel.glow <= 0.0 && !glass {
            if now.is_some() {
                commands
                    .entity(control)
                    .try_remove::<BackgroundGradient>();
            }
            continue;
        }
        let (hue, heart) = hues(*mood, &colors);
        let burn = feel.glow;
        let stops = pool(8.0, |near| {
            if glass {
                let fill = colors.glass_low.mix(
                    &colors.glass_high,
                    near * (0.55 + 0.45 * burn),
                );
                fill.mix(&heart, near * burn * 0.4).with_alpha(0.97)
            } else {
                hue.mix(&heart, near * 0.5)
                    .with_alpha(burn * near * 0.8)
            }
        });
        // Wide and tall enough to fade out inside the glass, with no
        // rim. The flame sits a little under the foot, to one side.
        let want = BackgroundGradient::from(RadialGradient::new(
            UiPosition::bottom(
                percent(9.0 * feel.lean),
                percent(2.5),
            ),
            RadialGradientShape::Ellipse(
                percent(150.0),
                percent(160.0),
            ),
            stops,
        ));
        if now != Some(&want) {
            commands.entity(control).try_insert(want);
        }
    }
}

type Glowing<'a> =
    (&'a Feel, Option<&'a Mood>, Option<&'a Children>, Entity);

type Shape<'a> = (
    &'a ComputedNode,
    &'a UiGlobalTransform,
    Option<&'a CalculatedClip>,
);

/// The node that holds the light a control spills on its panel's
/// [`GlowLayer`], and the place of that light there.
#[derive(Component)]
pub(super) struct SpillOf(Entity);

/// A spill to keep: its layer, its place there and its light.
pub(super) struct Spilt {
    layer: Entity,
    stands: Node,
    shadow: BoxShadow,
}

/// The place, in the layer's logical pixels, of a node as large as
/// `control` is drawn, squashed or swollen, with the corners of
/// `control` or round at its ends for lettering. Nothing when the
/// clip of `control` hides it.
fn stands(
    (node, at, clip): Shape,
    (layer, layer_at): (&ComputedNode, &UiGlobalTransform),
    lettering: bool,
) -> Option<Node> {
    let (drawn, _, centre) = at.to_scale_angle_translation();
    let size = node.size * drawn;
    let seen = |clip: &CalculatedClip| {
        clip.rects().is_some_and(|rects| {
            rects.iter().all(|cut| {
                let local =
                    cut.world_to_clip_local.transform_point2(centre);
                !Rect::from_center_half_size(local, size / 2.0)
                    .intersect(cut.rect)
                    .is_empty()
            })
        })
    };
    if clip.is_some_and(|clip| !seen(clip)) {
        return None;
    }
    let scale = node.inverse_scale_factor;
    let corner = centre
        - size / 2.0
        - (layer_at.translation - layer.size / 2.0);
    let round = |radius: Vec2| {
        let k = scale * drawn.x;
        if lettering {
            CornerRadius::circular(px(size.y * 0.5 * scale))
        } else {
            CornerRadius::new(px(radius.x * k), px(radius.y * k))
        }
    };
    let corners = node.border_radius;
    Some(Node {
        position_type: PositionType::Absolute,
        left: px(corner.x * scale),
        top: px(corner.y * scale),
        width: px(size.x * scale),
        height: px(size.y * scale),
        border_radius: BorderRadius {
            top_left: round(corners.top_left),
            top_right: round(corners.top_right),
            bottom_right: round(corners.bottom_right),
            bottom_left: round(corners.bottom_left),
        },
        ..Node::default()
    })
}

/// The share of a control's reach the light about lettering has.
const INK_REACH: f32 = 0.55;

/// The share of its reach a spilled light falls under the control,
/// as on a table under a lamp.
const DROP: f32 = 0.5;

/// Keeps the light that spills from every lit control as a shadow
/// of its colour, and takes it away when the control goes dark. A
/// control in a panel throws it on the panel's layer, which is cut at
/// the panel's border, and any other on itself.
pub(super) fn spill(
    mut commands: Commands,
    set: Res<FeelSettings>,
    theme: Themed,
    buttons: Query<Glowing>,
    shapes: Query<Shape>,
    shadows: Query<&BoxShadow, Without<SpillOf>>,
    ancestors: Query<&ChildOf>,
    panels: Query<&Children, With<GlowPanel>>,
    layers: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<GlowLayer>,
    >,
    mut spills: Query<(
        Entity,
        &SpillOf,
        &ChildOf,
        &mut Node,
        &mut BoxShadow,
    )>,
    mut spilt: Local<HashMap<Entity, Spilt>>,
) {
    spilt.clear();
    let colors = MagicColors::of(&theme);
    for (feel, mood, within, control) in &buttons {
        let mood = mood.copied().unwrap_or_default();
        // The light about lettering is about the words and the mark,
        // which are the button's one child.
        let (target, reach) = match mood {
            Mood::Ink => {
                let Some(first) = within.and_then(|w| w.first())
                else {
                    continue;
                };
                (*first, feel.reach * INK_REACH)
            }
            _ => (control, feel.reach),
        };
        let thrown = match mood {
            Mood::Bad | Mood::Field => set.spill * set.smoulder,
            _ => set.spill,
        };
        let strength = feel.glow * thrown;
        let now = shadows.get(target).ok();
        let shape = shapes
            .get(target)
            .ok()
            .filter(|(node, ..)| node.size.min_element() > 0.0)
            .filter(|_| strength > 0.0);
        let Some(shape) = shape else {
            if now.is_some() {
                commands.entity(target).try_remove::<BoxShadow>();
            }
            continue;
        };
        let (hue, _) = hues(mood, &colors);
        let want = BoxShadow::new(
            hue.with_alpha(strength),
            px(0.0),
            px(reach * DROP),
            px(0.0),
            px(reach),
        );
        let Some(layer) =
            layer_of(control, &ancestors, &panels, &layers)
        else {
            if now != Some(&want) {
                commands.entity(target).try_insert(want);
            }
            continue;
        };
        if now.is_some() {
            commands.entity(target).try_remove::<BoxShadow>();
        }
        let place = layers
            .get(layer)
            .ok()
            .and_then(|at| stands(shape, at, mood == Mood::Ink));
        if let Some(stands) = place {
            spilt.insert(
                control,
                Spilt {
                    layer,
                    stands,
                    shadow: want,
                },
            );
        }
    }
    for (spill, of, parent, mut node, mut shadow) in &mut spills {
        let Some(keep) = spilt.remove(&of.0) else {
            commands.entity(spill).try_despawn();
            continue;
        };
        if parent.parent() != keep.layer {
            commands.entity(spill).try_insert(ChildOf(keep.layer));
        }
        if *node != keep.stands {
            *node = keep.stands;
        }
        if *shadow != keep.shadow {
            *shadow = keep.shadow;
        }
    }
    for (control, keep) in spilt.drain() {
        commands.spawn((
            SpillOf(control),
            keep.stands,
            keep.shadow,
            Pickable::IGNORE,
            ChildOf(keep.layer),
        ));
    }
}

/// Lifts each spark, sways it and lets it die.
pub(super) fn embers(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<FeelSettings>,
    theme: Themed,
    mut sparks: Query<(
        Entity,
        &mut Ember,
        &mut Node,
        &mut BackgroundColor,
    )>,
) {
    let colors = MagicColors::of(&theme);
    for (id, mut ember, mut node, mut fill) in &mut sparks {
        ember.age += time.delta_secs();
        if ember.age < 0.0 {
            continue;
        }
        let t = ember.age / ember.life;
        if t >= 1.0 {
            commands.entity(id).try_despawn();
            continue;
        }
        // Fast off the wick, then slower as it cools.
        let up = set.rise * (1.0 - (1.0 - t) * (1.0 - t));
        let aside = ember.drift * t
            + 2.5 * (ember.sway + ember.age * 7.0).sin();
        node.left = px(ember.from.x + aside);
        node.top = px(ember.from.y - up);
        let bright = (t * 8.0).min(1.0) * (1.0 - t).powf(1.5);
        fill.0 = colors.ember.with_alpha(bright);
    }
}
