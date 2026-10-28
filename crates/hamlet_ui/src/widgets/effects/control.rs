//! The effects of a control: its glow under the pointer, its give
//! under a press, the gradient inside it, the outer glow it throws
//! and the particles of work done.

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
use super::{
    EffectColors, EffectSettings, Variant, ease, flicker, pool,
};
use crate::fade::ScrollFade;
use crate::sounds;
use crate::theme::Themed;

/// A button's animated state, each 0 to 1.
#[derive(Component, Default)]
pub(super) struct ControlEffect {
    heat: f32,
    press: f32,
    pub(super) pulse: f32,
    shake: f32,
    /// Its glow, the radius of its outer glow in pixels, and the
    /// side its glow leans to, -1 to 1.
    glow: f32,
    reach: f32,
    lean: f32,
}

/// A particle rising from where the primary action was done.
#[derive(Component)]
pub(super) struct Particle {
    from: Vec2,
    /// Pixels it drifts sideways over its life, and the place in its
    /// sway it starts at.
    drift: f32,
    sway: f32,
    /// Seconds since it appeared. Below zero it has yet to.
    age: f32,
    life: f32,
}

pub(super) fn arm(
    mut commands: Commands,
    buttons: Query<Entity, (With<Button>, Without<ControlEffect>)>,
    fields: Query<&ChildOf, Added<EditableText>>,
) {
    for field in &fields {
        commands
            .entity(field.parent())
            .try_insert((ControlEffect::default(), Variant::Input));
    }
    for button in &buttons {
        commands.entity(button).try_insert((
            ControlEffect::default(),
            UiTransform::IDENTITY,
        ));
    }
}

type Entered = (Changed<Hovered>, With<ControlEffect>);

pub(super) fn hover_tick(
    buttons: Query<(&Hovered, Option<&Variant>), Entered>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    let heard = |(hovered, variant): (&Hovered, Option<&Variant>)| {
        hovered.0 && variant.is_some_and(|variant| variant.has_tick())
    };
    if buttons.iter().any(heard) {
        sounds::write(&mut cues, &mut turn, Sound::Tick);
    }
}

pub(super) fn press_tap(
    pressed: Query<(), (Added<Pressed>, With<ControlEffect>)>,
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

/// Spawns the particles of a primary action done on a control that
/// stands at `centre` and is `size` across, in logical pixels.
fn spawn_particles(
    commands: &mut Commands,
    set: &EffectSettings,
    centre: Vec2,
    size: Vec2,
    seed: u32,
) {
    for index in 0..set.particle_count {
        let roll = |salt: u32| {
            chance(
                seed.wrapping_mul(31).wrapping_add(index * 7 + salt),
            )
        };
        let from = centre
            + Vec2::new(
                (roll(0) - 0.5) * size.x * 0.8,
                -size.y * 0.5 + roll(1) * 4.0,
            );
        let side = set.particle_size * (0.7 + 0.5 * roll(2));
        commands.spawn((
            Particle {
                from,
                drift: (roll(3) - 0.5) * 20.0,
                sway: roll(4) * std::f32::consts::TAU,
                age: -0.14 * roll(5),
                life: set.particle_lifetime * (0.6 + 0.4 * roll(6)),
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
    &'a mut ControlEffect,
    Option<&'a Variant>,
    Option<&'a ComputedNode>,
    Option<&'a UiGlobalTransform>,
);

/// A button that has done its work pulses, with particles if it was
/// the primary action, or shakes if what it did was to refuse.
pub(super) fn answer(
    activated: On<Activate>,
    mut commands: Commands,
    set: Res<EffectSettings>,
    mut buttons: Query<Answering>,
    mut cues: MessageWriter<Cue>,
    mut turn: Local<u32>,
) {
    let Ok((mut effect, variant, node, at)) =
        buttons.get_mut(activated.entity)
    else {
        return;
    };
    match variant.copied().unwrap_or_default() {
        Variant::Input => {}
        Variant::Subtle | Variant::Default | Variant::Text => {
            effect.pulse = 1.0;
        }
        Variant::Primary => {
            effect.pulse = 1.0;
            sounds::write(&mut cues, &mut turn, Sound::Confirm);
            if let Some((node, at)) = node.zip(at) {
                let scale = node.inverse_scale_factor;
                spawn_particles(
                    &mut commands,
                    &set,
                    at.translation * scale,
                    node.size * scale,
                    activated.entity.index_u32() ^ *turn,
                );
            }
        }
        Variant::Danger => {
            effect.shake = 1.0;
            sounds::write(&mut cues, &mut turn, Sound::Deny);
        }
    }
}

type Animated<'a> = (
    Entity,
    &'a mut ControlEffect,
    Option<&'a mut UiTransform>,
    Has<Focused>,
    Option<&'a Hovered>,
    Has<Pressed>,
    Option<&'a Variant>,
);

pub(super) fn animate(
    time: Res<Time>,
    set: Res<EffectSettings>,
    mut buttons: Query<Animated>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (
        id,
        mut effect,
        transform,
        focused,
        hovered,
        held,
        variant,
    ) in &mut buttons
    {
        let variant = variant.copied().unwrap_or_default();
        let over = if variant == Variant::Input {
            focused
        } else {
            hovered.is_some_and(|h| h.0)
        };
        let heat =
            ease(effect.heat, f32::from(over), set.glow_rate, dt);
        let press =
            ease(effect.press, f32::from(held), set.press_rate, dt);
        let pulse = ease(effect.pulse, 0.0, set.pulse_decay, dt);
        let shake = ease(effect.shake, 0.0, set.pulse_decay, dt);
        let still = heat == effect.heat
            && press == effect.press
            && pulse == effect.pulse
            && shake == effect.shake;
        // Its glow at rest and under the pointer or the caret, and
        // the glow its work adds.
        let (rest, near, done) = match variant {
            Variant::Subtle => (0.0, 0.0, 0.0),
            Variant::Input => (0.0, set.glow_input, 0.0),
            Variant::Default | Variant::Text => {
                (0.0, set.glow_default, set.done_boost * pulse)
            }
            Variant::Primary => {
                (set.glow_primary_idle, set.glow_primary, pulse)
            }
            Variant::Danger => {
                (0.0, set.glow_danger, set.done_boost * shake)
            }
        };
        let steady = rest + (near - rest) * heat;
        if still && steady == 0.0 && effect.glow == 0.0 {
            continue;
        }
        // A pulse swells and falls back as it dies away.
        let swell = 4.0 * pulse * (1.0 - pulse);
        let scale =
            1.0 - set.press_scale * press + set.pulse_scale * swell;
        let slide =
            shake * set.shake_distance * (now * set.shake_rate).sin();
        let want = UiTransform {
            translation: Val2::new(
                px(slide),
                px(set.press_offset * press),
            ),
            scale: Vec2::splat(scale),
            ..UiTransform::IDENTITY
        };
        if let Some(mut transform) = transform
            && *transform != want
        {
            *transform = want;
        }
        // A press dims the glow, and the work done raises it.
        let covered = steady * (1.0 - set.press_dim * press);
        let lit = covered + (1.0 - covered) * done;
        let seed = id.index_u32() as f32 * 1.618;
        let brightness = 1.0 + set.flicker * flicker(now, seed);
        *effect = ControlEffect {
            heat,
            press,
            pulse,
            shake,
            glow: (lit * brightness).clamp(0.0, 1.0),
            reach: set.outer_glow_radius
                + set.outer_glow_pulse_radius * pulse.max(shake),
            lean: (now * 1.1 + seed).sin(),
        };
    }
}

/// The colour of a control's glow and of the core of that glow.
fn hues(variant: Variant, colors: &EffectColors) -> (Color, Color) {
    match variant {
        Variant::Danger => (colors.glow_danger, colors.particle),
        _ => (colors.glow, colors.glow_core),
    }
}

type Glowing<'a> = (
    Entity,
    &'a ControlEffect,
    &'a Variant,
    Option<&'a BackgroundGradient>,
);

/// Keeps the glow gradient inside every control that has one: a pool
/// of glow from low in the control, leaning to one side.
pub(super) fn update_gradients(
    mut commands: Commands,
    theme: Themed,
    controls: Query<Glowing>,
) {
    let colors = EffectColors::of(&theme);
    for (control, effect, variant, now) in &controls {
        // Text has no fill for a glow to show in.
        if matches!(*variant, Variant::Subtle | Variant::Text) {
            continue;
        }
        // Only the primary action shows its fill while it is out.
        let filled = *variant == Variant::Primary;
        if effect.glow <= 0.0 && !filled {
            if now.is_some() {
                commands
                    .entity(control)
                    .try_remove::<BackgroundGradient>();
            }
            continue;
        }
        let (hue, core) = hues(*variant, &colors);
        let burn = effect.glow;
        let stops = pool(8.0, |near| {
            if filled {
                let fill = colors.primary_low.mix(
                    &colors.primary_high,
                    near * (0.55 + 0.45 * burn),
                );
                fill.mix(&core, near * burn * 0.4).with_alpha(0.97)
            } else {
                hue.mix(&core, near * 0.5)
                    .with_alpha(burn * near * 0.8)
            }
        });
        // Wide and tall enough to fade out inside the control, with
        // no rim. The glow sits a little under the bottom edge, to
        // one side.
        let want = BackgroundGradient::from(RadialGradient::new(
            UiPosition::bottom(
                percent(9.0 * effect.lean),
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

type Throwing<'a> = (
    &'a ControlEffect,
    Option<&'a Variant>,
    Option<&'a Children>,
    Entity,
);

type Shape<'a> = (
    &'a ComputedNode,
    &'a UiGlobalTransform,
    Option<&'a CalculatedClip>,
);

/// The node that holds the outer glow a control throws on its
/// panel's [`GlowLayer`], and the place of that glow there.
#[derive(Component)]
pub(super) struct OuterGlowOf(Entity);

/// An outer glow to keep: its layer, its place there and its shadow.
pub(super) struct OuterGlow {
    layer: Entity,
    stands: Node,
    shadow: BoxShadow,
}

/// The place, in the layer's logical pixels, of a node as large as
/// `control` is drawn, squashed or swollen, with the corners of
/// `control` or round at its ends for text. Nothing when the clip of
/// `control` hides it.
fn stands(
    (node, at, clip): Shape,
    (layer, layer_at): (&ComputedNode, &UiGlobalTransform),
    text: bool,
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
        if text {
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

/// The share of its glow a control keeps while it lies in a scroll
/// area: all of it once both its edges are `room` logical pixels
/// inside the area, and none as one reaches the area's edge.
fn in_view(
    control: Entity,
    (node, at, _): Shape,
    ancestors: &Query<&ChildOf>,
    scrolls: &Query<
        (&ComputedNode, &UiGlobalTransform),
        With<ScrollFade>,
    >,
    room: f32,
) -> f32 {
    let Some((area, area_at)) = ancestors
        .iter_ancestors(control)
        .find_map(|above| scrolls.get(above).ok())
    else {
        return 1.0;
    };
    let (drawn, _, centre) = at.to_scale_angle_translation();
    let half = node.size.y * drawn.y / 2.0;
    let (top, bottom) = (centre.y - half, centre.y + half);
    // As large as the area is drawn, as the control is.
    let (area_drawn, _, area_centre) =
        area_at.to_scale_angle_translation();
    let tall = area.size.y * area_drawn.y / 2.0;
    let (from, to) = (area_centre.y - tall, area_centre.y + tall);
    let inside = (top - from).min(to - bottom);
    let share =
        (inside * node.inverse_scale_factor / room).clamp(0.0, 1.0);
    share * share * (3.0 - 2.0 * share)
}

/// The share of its radius the outer glow about text has.
const TEXT_REACH: f32 = 0.55;

/// The share of its radius an outer glow falls below the control.
const DROP: f32 = 0.5;

/// Keeps the outer glow of every glowing control as a shadow of its
/// colour, and takes it away when the control goes dark. A control in
/// a panel throws it on the panel's layer, which is cut at the
/// panel's border, and any other on itself.
pub(super) fn outer_glow(
    mut commands: Commands,
    set: Res<EffectSettings>,
    theme: Themed,
    buttons: Query<Throwing>,
    shapes: Query<Shape>,
    shadows: Query<&BoxShadow, Without<OuterGlowOf>>,
    ancestors: Query<&ChildOf>,
    scrolls: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<ScrollFade>,
    >,
    panels: Query<&Children, With<GlowPanel>>,
    layers: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<GlowLayer>,
    >,
    mut glows: Query<(
        Entity,
        &OuterGlowOf,
        &ChildOf,
        &mut Node,
        &mut BoxShadow,
    )>,
    mut kept: Local<HashMap<Entity, OuterGlow>>,
) {
    kept.clear();
    let colors = EffectColors::of(&theme);
    for (effect, variant, within, control) in &buttons {
        let variant = variant.copied().unwrap_or_default();
        // The glow about text is about the words and the icon, which
        // are the button's one child.
        let (target, reach) = match variant {
            Variant::Text => {
                let Some(first) = within.and_then(|w| w.first())
                else {
                    continue;
                };
                (*first, effect.reach * TEXT_REACH)
            }
            _ => (control, effect.reach),
        };
        let thrown = match variant {
            Variant::Danger | Variant::Input => {
                set.outer_glow * set.outer_glow_danger
            }
            _ => set.outer_glow,
        };
        // The glow is not cut where a scroll area ends, so it dims
        // with the fade as its control nears the edge.
        let view = shapes.get(target).ok().map_or(0.0, |shape| {
            in_view(
                control,
                shape,
                &ancestors,
                &scrolls,
                theme.space.scroll_fade + reach / 2.0,
            )
        });
        let strength = effect.glow * thrown * view;
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
        let (hue, _) = hues(variant, &colors);
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
        let place = layers.get(layer).ok().and_then(|at| {
            stands(shape, at, variant == Variant::Text)
        });
        if let Some(stands) = place {
            kept.insert(
                control,
                OuterGlow {
                    layer,
                    stands,
                    shadow: want,
                },
            );
        }
    }
    for (glow, of, parent, mut node, mut shadow) in &mut glows {
        let Some(keep) = kept.remove(&of.0) else {
            commands.entity(glow).try_despawn();
            continue;
        };
        if parent.parent() != keep.layer {
            commands.entity(glow).try_insert(ChildOf(keep.layer));
        }
        if *node != keep.stands {
            *node = keep.stands;
        }
        if *shadow != keep.shadow {
            *shadow = keep.shadow;
        }
    }
    for (control, keep) in kept.drain() {
        commands.spawn((
            OuterGlowOf(control),
            keep.stands,
            keep.shadow,
            Pickable::IGNORE,
            ChildOf(keep.layer),
        ));
    }
}

/// Lifts each particle, sways it and lets it die.
pub(super) fn particles(
    mut commands: Commands,
    time: Res<Time>,
    set: Res<EffectSettings>,
    theme: Themed,
    mut sparks: Query<(
        Entity,
        &mut Particle,
        &mut Node,
        &mut BackgroundColor,
    )>,
) {
    let colors = EffectColors::of(&theme);
    for (id, mut particle, mut node, mut fill) in &mut sparks {
        particle.age += time.delta_secs();
        if particle.age < 0.0 {
            continue;
        }
        let t = particle.age / particle.life;
        if t >= 1.0 {
            commands.entity(id).try_despawn();
            continue;
        }
        // Fast at first, then slower as it fades.
        let up = set.particle_rise * (1.0 - (1.0 - t) * (1.0 - t));
        let aside = particle.drift * t
            + 2.5 * (particle.sway + particle.age * 7.0).sin();
        node.left = px(particle.from.x + aside);
        node.top = px(particle.from.y - up);
        let bright = (t * 8.0).min(1.0) * (1.0 - t).powf(1.5);
        fill.0 = colors.particle.with_alpha(bright);
    }
}
