//! The feel of a control: its warmth under the pointer, its give
//! under a press, the flame in its glass, the light it spills and
//! the sparks of work done.

use bevy::asset::{Asset, Assets};
use bevy::color::{Alpha, Color};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::{Added, Changed, Has, With, Without};
use bevy::ecs::system::{Commands, Local, Query, Res, ResMut};
use bevy::math::{Vec2, Vec4};
use bevy::picking::Pickable;
use bevy::picking::hover::Hovered;
use bevy::reflect::TypePath;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use bevy::text::EditableText;
use bevy::time::Time;
use bevy::ui::{
    BackgroundColor, BorderRadius, ComputedNode, GlobalZIndex, Node,
    PositionType, Pressed, UiGlobalTransform, UiTransform, Val2, px,
};
use bevy::ui_render::prelude::{MaterialNode, UiMaterial};
use bevy::ui_widgets::{Activate, Button};
use bevy_fynix::Focused;
use hamlet_realm::sound::{Cue, Sound};

use super::{FeelSettings, MagicColors, Mood, ease, linear, waver};
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

/// The light of a control: on the control itself, the flame low in
/// its glass; on a node around it, the light that spills out.
#[derive(Asset, TypePath, AsBindGroup, Clone, PartialEq)]
pub(super) struct Lamp {
    /// rgb: the light. a: its brightness.
    #[uniform(0)]
    hue: Vec4,
    /// rgb: the heart of the flame. a: the share of the glass that
    /// shows when the lamp is out.
    #[uniform(1)]
    heart: Vec4,
    /// The glass away from the flame and near it.
    #[uniform(2)]
    low: Vec4,
    #[uniform(3)]
    high: Vec4,
    /// x: the node's reach past the control, in physical pixels. y:
    /// the control's corner radius. z: the side the flame leans to.
    #[uniform(4)]
    shape: Vec4,
}

impl UiMaterial for Lamp {
    fn fragment_shader() -> ShaderRef {
        "embedded://hamlet_ui/widgets/feel/lamp.wgsl".into()
    }
}

/// The node that draws the light a control spills. It stands outside
/// every panel, so that no panel or list cuts the light at its edge.
#[derive(Component)]
pub(super) struct SpillOf(Entity);

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
        hovered.0 && mood.is_some_and(|mood| mood.ticks())
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

type Lamped<'a> = (
    Entity,
    &'a Feel,
    &'a Mood,
    &'a ComputedNode,
    Option<&'a MaterialNode<Lamp>>,
);

/// Keeps the flame in the glass of every control that has one.
pub(super) fn light(
    mut commands: Commands,
    theme: Themed,
    mut lamps: ResMut<Assets<Lamp>>,
    controls: Query<Lamped>,
) {
    let colors = MagicColors::of(&theme);
    for (control, feel, mood, node, lamp) in &controls {
        // Lettering has no glass for a flame to burn in.
        if matches!(*mood, Mood::Quiet | Mood::Ink) {
            continue;
        }
        let (hue, heart) = hues(*mood, &colors);
        // Only the good action shows its glass while it is out.
        let glass = f32::from(*mood == Mood::Good);
        let want = Lamp {
            hue: linear(hue).extend(feel.glow),
            heart: linear(heart).extend(glass),
            low: linear(colors.glass_low).extend(1.0),
            high: linear(colors.glass_high).extend(1.0),
            shape: Vec4::new(
                0.0,
                node.border_radius.top_left,
                feel.lean,
                0.0,
            ),
        };
        let Some(lamp) = lamp else {
            commands
                .entity(control)
                .try_insert(MaterialNode(lamps.add(want)));
            continue;
        };
        let stale =
            lamps.get(&lamp.0).is_some_and(|now| *now != want);
        if stale && let Some(mut now) = lamps.get_mut(&lamp.0) {
            *now = want;
        }
    }
}

type Glowing<'a> = (
    Entity,
    &'a Feel,
    Option<&'a Mood>,
    &'a ComputedNode,
    &'a UiGlobalTransform,
    Option<&'a Children>,
);

/// The share of a control's reach the light about lettering has.
const INK_REACH: f32 = 0.55;

/// Keeps the light that spills from every lit control, and takes it
/// away when the control goes dark or goes away.
pub(super) fn spill(
    mut commands: Commands,
    set: Res<FeelSettings>,
    theme: Themed,
    mut lamps: ResMut<Assets<Lamp>>,
    buttons: Query<Glowing>,
    inked: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut pools: Query<(
        Entity,
        &SpillOf,
        &mut Node,
        &MaterialNode<Lamp>,
    )>,
    mut pooled: Local<Vec<Entity>>,
) {
    let colors = MagicColors::of(&theme);
    // The place of the light's node and the lamp it shows.
    let place = |(_, feel, mood, node, at, within): Glowing| {
        let mood = mood.copied().unwrap_or_default();
        // The light about lettering is about the words and the mark,
        // which are the button's one child.
        let written = within
            .filter(|_| mood == Mood::Ink)
            .and_then(|within| within.first())
            .and_then(|first| inked.get(*first).ok());
        let (node, at, reach) = match written {
            Some((node, at)) => (node, at, feel.reach * INK_REACH),
            None => (node, at, feel.reach),
        };
        let scale = node.inverse_scale_factor;
        // As large as the control is drawn, squashed or swollen.
        let (drawn, _, centre) = at.to_scale_angle_translation();
        let size = node.size * scale * drawn;
        let corner = centre * scale - size / 2.0;
        let stands = Node {
            position_type: PositionType::Absolute,
            left: px(corner.x - reach),
            top: px(corner.y - reach),
            width: px(size.x + 2.0 * reach),
            height: px(size.y + 2.0 * reach),
            ..Node::default()
        };
        // Round at its ends, as light about a line of words is.
        let round = match written {
            Some(_) => node.size.y * 0.5,
            None => node.border_radius.top_left,
        };
        let (hue, heart) = hues(mood, &colors);
        let thrown = match mood {
            Mood::Bad | Mood::Field => set.spill * set.smoulder,
            _ => set.spill,
        };
        let shows = Lamp {
            hue: linear(hue).extend(feel.glow * thrown),
            heart: linear(heart).extend(0.0),
            low: Vec4::ZERO,
            high: Vec4::ZERO,
            shape: Vec4::new(
                reach / scale,
                round * drawn.x,
                feel.lean,
                f32::from(written.is_some()),
            ),
        };
        (stands, shows)
    };
    let lit = |(_, feel, _, node, _, _): &Glowing| {
        feel.glow > 0.0 && node.size.min_element() > 0.0
    };
    pooled.clear();
    for (pool, of, mut node, material) in &mut pools {
        let Some(button) = buttons.get(of.0).ok().filter(lit) else {
            commands.entity(pool).try_despawn();
            continue;
        };
        pooled.push(of.0);
        let (stands, shows) = place(button);
        if *node != stands {
            *node = stands;
        }
        if let Some(mut lamp) = lamps.get_mut(&material.0)
            && *lamp != shows
        {
            *lamp = shows;
        }
    }
    for button in buttons.iter().filter(lit) {
        if pooled.contains(&button.0) {
            continue;
        }
        let (stands, shows) = place(button);
        commands.spawn((
            SpillOf(button.0),
            stands,
            MaterialNode(lamps.add(shows)),
            Pickable::IGNORE,
            GlobalZIndex(i32::MAX - 2),
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

#[cfg(test)]
mod tests {
    use bevy::app::{App, Update};
    use bevy::ecs::message::Messages;
    use bevy::ecs::schedule::IntoScheduleConfigs;
    use bevy::ui::Val;
    use bevy_fynix::Theme;
    use hamlet_assets::Tuning;

    use super::*;
    use crate::testing::step;
    use crate::theme::HamletTheme;

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<FeelSettings>()
            .insert_resource(Theme(HamletTheme::designed()))
            .init_resource::<Time>()
            .init_resource::<Assets<Lamp>>()
            .add_message::<Cue>()
            .add_observer(answer)
            .add_systems(
                Update,
                (
                    arm, hover_tick, press_tap, feel, light, spill,
                    embers,
                )
                    .chain(),
            );
        app
    }

    /// A button a hundred pixels wide, as layout would leave it.
    fn button(app: &mut App, mood: Mood) -> Entity {
        let node = ComputedNode {
            size: Vec2::new(100.0, 28.0),
            inverse_scale_factor: 1.0,
            ..ComputedNode::default()
        };
        app.world_mut()
            .spawn((Button, mood, node, UiGlobalTransform::default()))
            .id()
    }

    fn heard(app: &App) -> Vec<Sound> {
        let cues = app.world().resource::<Messages<Cue>>();
        cues.iter_current_update_messages()
            .map(|cue| cue.sound)
            .collect()
    }

    /// The colour and brightness of the light each control spills.
    fn glows(app: &mut App) -> Vec<Vec4> {
        let world = app.world_mut();
        let pools = world
            .query_filtered::<&MaterialNode<Lamp>, With<SpillOf>>()
            .iter(world)
            .map(|pool| pool.0.clone())
            .collect::<Vec<_>>();
        let lamps = world.resource::<Assets<Lamp>>();
        pools
            .iter()
            .filter_map(|pool| lamps.get(pool).map(|lamp| lamp.hue))
            .collect()
    }

    /// The light of the flame in a control's own glass.
    fn flame(app: &App, control: Entity) -> f32 {
        let lamp =
            app.world().get::<MaterialNode<Lamp>>(control).unwrap();
        let lamps = app.world().resource::<Assets<Lamp>>();
        lamps.get(&lamp.0).unwrap().hue.w
    }

    fn sparks(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query::<&Ember>().iter(world).count()
    }

    fn at_rest(app: &App, button: Entity) -> bool {
        *app.world().get::<UiTransform>(button).unwrap()
            == UiTransform::IDENTITY
    }

    #[test]
    fn a_held_button_sinks_dims_and_a_released_one_comes_back() {
        let mut app = app();
        let button = button(&mut app, Mood::Plain);
        app.world_mut().entity_mut(button).insert(Hovered(true));
        for _ in 0..60 {
            step(&mut app);
        }
        let open = flame(&app, button);
        app.world_mut().entity_mut(button).insert(Pressed);
        for _ in 0..20 {
            step(&mut app);
        }
        let held = *app.world().get::<UiTransform>(button).unwrap();
        assert!(held.scale.x < 0.98, "{:?}", held.scale);
        let Val::Px(sunk) = held.translation.y else {
            panic!("sunk in pixels");
        };
        assert!(sunk > 1.0, "{sunk}");
        let covered = flame(&app, button);
        assert!(covered < open * 0.6, "{covered} of {open}");

        app.world_mut()
            .entity_mut(button)
            .remove::<Pressed>()
            .insert(Hovered(false));
        for _ in 0..80 {
            step(&mut app);
        }
        assert!(at_rest(&app, button));
        assert!(glows(&mut app).is_empty());
    }

    #[test]
    fn a_good_button_pulses_confirms_and_throws_sparks() {
        let mut app = app();
        let button = button(&mut app, Mood::Good);
        step(&mut app);
        app.world_mut().trigger(Activate { entity: button });
        assert_eq!(heard(&app), [Sound::Confirm]);
        for _ in 0..6 {
            step(&mut app);
        }
        let scale =
            app.world().get::<UiTransform>(button).unwrap().scale.x;
        assert!(scale > 1.0, "{scale}");
        assert!(flame(&app, button) > 0.6);
        let glows = glows(&mut app);
        assert_eq!(glows.len(), 1);
        assert_eq!(
            glows[0].truncate(),
            linear(HamletTheme::designed().palette.lamp)
        );
        let set = FeelSettings::default();
        assert_eq!(sparks(&mut app), set.embers as usize);

        for _ in 0..200 {
            step(&mut app);
        }
        assert_eq!(sparks(&mut app), 0, "every spark dies");
        assert!(at_rest(&app, button));
    }

    #[test]
    fn a_bad_button_shakes_like_a_coal_and_settles() {
        let mut app = app();
        let button = button(&mut app, Mood::Bad);
        step(&mut app);
        app.world_mut().trigger(Activate { entity: button });
        assert_eq!(heard(&app), [Sound::Deny]);
        let set = FeelSettings::default();
        let mut furthest = 0.0f32;
        for _ in 0..12 {
            step(&mut app);
            let moved = app
                .world()
                .get::<UiTransform>(button)
                .unwrap()
                .translation
                .x;
            if let Val::Px(x) = moved {
                furthest = furthest.max(x.abs());
            }
        }
        assert!(furthest > 1.0, "{furthest}");
        assert!(furthest <= set.shake, "a shake of the head");
        assert_eq!(
            glows(&mut app)[0].truncate(),
            linear(HamletTheme::designed().palette.coal)
        );
        assert_eq!(sparks(&mut app), 0);

        for _ in 0..200 {
            step(&mut app);
        }
        assert!(at_rest(&app, button));
        assert!(glows(&mut app).is_empty());
    }

    #[test]
    fn a_glow_goes_with_its_button() {
        let mut app = app();
        let button = button(&mut app, Mood::Good);
        for _ in 0..3 {
            step(&mut app);
        }
        assert_eq!(glows(&mut app).len(), 1);
        app.world_mut().entity_mut(button).despawn();
        step(&mut app);
        assert!(glows(&mut app).is_empty());
    }
}
