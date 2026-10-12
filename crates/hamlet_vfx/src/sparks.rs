//! Particles on the GPU: embers that burst from the ground, and
//! swarms of fireflies. A world whose [`Density`] is 0 has none.

use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy_hanabi::Gradient;
use bevy_hanabi::prelude::*;

use crate::{Density, Now, Tints, VfxSystems};

/// The world's particles.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SparkSettings {
    /// Embers of a falling house at full quality, the seconds each
    /// lasts at most, how fast they rise and how big they begin.
    pub embers: u32,
    pub ember_life: f32,
    pub ember_speed: f32,
    pub ember_size: f32,
    /// Brightness of an ember and of a firefly, over white.
    pub ember_light: f32,
    pub firefly_light: f32,
    /// Fireflies born each second in a town at full quality, the
    /// seconds each lives, how far from the common they roam and how
    /// high, how fast they drift and how big they are.
    pub fireflies: f32,
    pub firefly_life: f32,
    pub firefly_reach: f32,
    pub firefly_height: f32,
    pub firefly_speed: f32,
    pub firefly_size: f32,
}

/// The effects, made once for the density the world is set to.
#[derive(Resource)]
pub struct Sparks {
    made_for: f32,
    embers: Handle<EffectAsset>,
    fireflies: Handle<EffectAsset>,
}

impl Sparks {
    /// True when the world has particles at all.
    pub fn any(&self) -> bool {
        self.made_for > 0.0
    }

    /// A swarm of fireflies, to be put where it should fly. None
    /// where the world has no particles.
    pub fn swarm(&self) -> Option<impl Bundle + use<>> {
        self.any().then(|| {
            (Swarm, ParticleEffect::new(self.fireflies.clone()))
        })
    }
}

/// A burst that is over at a clock time, to be cleared away then.
#[derive(Component)]
struct Spent(f32);

/// A swarm of fireflies.
#[derive(Component)]
struct Swarm;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SparkSettings>()
        .init_resource::<Density>();
    // A world with nothing to draw on has no particles.
    if !app.is_plugin_added::<RenderPlugin>() {
        return;
    }
    if !app.is_plugin_added::<HanabiPlugin>() {
        app.add_plugins(HanabiPlugin);
    }
    app.add_systems(Update, (make, clear).chain().in_set(VfxSystems));
}

/// A colour that is `light` times white at its brightest and fades
/// to nothing, as `(age, colour)` keys.
fn fading(
    colour: Vec3,
    light: f32,
    keys: &[(f32, f32)],
) -> Gradient<Vec4> {
    let mut gradient = Gradient::<Vec4>::new();
    for &(at, shown) in keys {
        gradient.add_key(at, (colour * light).extend(shown));
    }
    gradient
}

fn embers(
    tints: &Tints,
    set: &SparkSettings,
    count: u32,
) -> EffectAsset {
    let w = ExprWriter::new();
    let place = SetPositionCircleModifier {
        center: w.lit(Vec3::ZERO).expr(),
        axis: w.lit(Vec3::Y).expr(),
        radius: w.lit(2.4).expr(),
        dimension: ShapeDimension::Volume,
    };
    // Up, each a little off to one side.
    let side = (w.rand(VectorType::VEC3F) - w.lit(0.5))
        * w.lit(Vec3::new(1.6, 0.0, 1.6));
    let up =
        w.lit(set.ember_speed * 0.5).uniform(w.lit(set.ember_speed));
    let speed = SetAttributeModifier::new(
        Attribute::VELOCITY,
        (side + w.lit(Vec3::Y) * up).expr(),
    );
    let age =
        SetAttributeModifier::new(Attribute::AGE, w.lit(0.0).expr());
    let life = SetAttributeModifier::new(
        Attribute::LIFETIME,
        w.lit(set.ember_life * 0.45)
            .uniform(w.lit(set.ember_life))
            .expr(),
    );
    let drag = LinearDragModifier::new(w.lit(1.1).expr());
    let lift = AccelModifier::new(w.lit(Vec3::Y * 0.9).expr());
    let mut size = Gradient::<Vec3>::new();
    size.add_key(0.0, Vec3::splat(set.ember_size));
    size.add_key(1.0, Vec3::ZERO);
    EffectAsset::new(
        count.max(1),
        SpawnerSettings::once((count as f32).into()),
        w.finish(),
    )
    .with_name("embers")
    .init(place)
    .init(speed)
    .init(age)
    .init(life)
    .update(drag)
    .update(lift)
    .render(ColorOverLifetimeModifier {
        gradient: fading(
            tints.ember,
            set.ember_light,
            &[(0.0, 1.0), (0.6, 0.8), (1.0, 0.0)],
        ),
        blend: ColorBlendMode::Overwrite,
        mask: ColorBlendMask::RGBA,
    })
    .render(SizeOverLifetimeModifier {
        gradient: size,
        screen_space_size: false,
    })
    .render(OrientModifier::new(OrientMode::FaceCameraPosition))
}

fn firefly_swarm(
    tints: &Tints,
    set: &SparkSettings,
    rate: f32,
) -> EffectAsset {
    let w = ExprWriter::new();
    let place = SetPositionCircleModifier {
        center: w.lit(Vec3::ZERO).expr(),
        axis: w.lit(Vec3::Y).expr(),
        radius: w.lit(set.firefly_reach).expr(),
        dimension: ShapeDimension::Volume,
    };
    // Each is born at a height of its own and wanders from there.
    let wander = (w.rand(VectorType::VEC3F) - w.lit(0.5))
        * w.lit(Vec3::new(2.0, 0.6, 2.0) * set.firefly_speed);
    let speed =
        SetAttributeModifier::new(Attribute::VELOCITY, wander.expr());
    let rise = SetAttributeModifier::new(
        Attribute::POSITION,
        (w.attr(Attribute::POSITION)
            + w.lit(Vec3::Y)
                * w.lit(0.4).uniform(w.lit(set.firefly_height)))
        .expr(),
    );
    let age =
        SetAttributeModifier::new(Attribute::AGE, w.lit(0.0).expr());
    let life = SetAttributeModifier::new(
        Attribute::LIFETIME,
        w.lit(set.firefly_life * 0.6)
            .uniform(w.lit(set.firefly_life))
            .expr(),
    );
    let capacity = (rate * set.firefly_life).ceil() as u32 + 8;
    EffectAsset::new(
        capacity,
        SpawnerSettings::rate(rate.into()),
        w.finish(),
    )
    .with_name("fireflies")
    .init(place)
    .init(rise)
    .init(speed)
    .init(age)
    .init(life)
    .render(ColorOverLifetimeModifier {
        // They blink on, glow, dim and glow again before they go.
        gradient: fading(
            tints.firefly,
            set.firefly_light,
            &[
                (0.0, 0.0),
                (0.12, 1.0),
                (0.45, 0.35),
                (0.7, 1.0),
                (1.0, 0.0),
            ],
        ),
        blend: ColorBlendMode::Overwrite,
        mask: ColorBlendMask::RGBA,
    })
    .render(SizeOverLifetimeModifier {
        gradient: Gradient::constant(Vec3::splat(set.firefly_size)),
        screen_space_size: false,
    })
    .render(OrientModifier::new(OrientMode::FaceCameraPosition))
}

/// Makes the effects for the density that is set, and makes them
/// again when it or their tuning changes.
fn make(
    mut commands: Commands,
    density: Res<Density>,
    set: Res<SparkSettings>,
    tints: Res<Tints>,
    sparks: Option<Res<Sparks>>,
    mut effects: ResMut<Assets<EffectAsset>>,
    old: Query<Entity, With<Swarm>>,
) {
    let stale = sparks
        .as_ref()
        .is_none_or(|sparks| sparks.made_for != density.0)
        || set.is_changed();
    if !stale {
        return;
    }
    // The swarms are of the old kind.
    for swarm in &old {
        commands.entity(swarm).try_despawn();
    }
    let share = density.0;
    let count = (set.embers as f32 * share) as u32;
    commands.insert_resource(Sparks {
        made_for: share,
        embers: effects.add(embers(&tints, &set, count)),
        fireflies: effects.add(firefly_swarm(
            &tints,
            &set,
            set.fireflies * share,
        )),
    });
}

/// Sets off a burst of embers at `ground`. Returns false where the
/// world has no particles, for something plainer to stand in.
pub fn burst(
    commands: &mut Commands,
    sparks: Option<&Sparks>,
    set: &SparkSettings,
    now: f32,
    ground: Vec3,
) -> bool {
    let Some(sparks) = sparks.filter(|s| s.any()) else {
        return false;
    };
    commands.spawn((
        ParticleEffect::new(sparks.embers.clone()),
        Transform::from_translation(ground),
        Spent(now + set.ember_life + 0.5),
    ));
    true
}

fn clear(
    mut commands: Commands,
    now: Res<Now>,
    bursts: Query<(Entity, &Spent)>,
) {
    for (burst, spent) in &bursts {
        if now.0 >= spent.0 {
            commands.entity(burst).try_despawn();
        }
    }
}
