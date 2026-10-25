//! The Agent Bazaar in the world: a striped pavilion and a ring of
//! five stalls on ground of their own beside the first town, in no
//! hamlet.

mod build;
mod bustle;
mod keepers;
mod lights;
mod merchant;
mod pitch;
mod shoppers;
mod stalls;

use bevy::prelude::*;

pub use self::keepers::Stallholder;
pub use self::merchant::Merchant;
pub use self::shoppers::Shopper;
pub use self::stalls::{Keeper, Stall, Traded};
use crate::chief::mascot;
use crate::generator::DetailSettings;
use crate::generator::society::{Ground, Society};
use crate::{RealmSystems, WorldCamera, pick};

pub fn plugin(app: &mut App) {
    app.init_resource::<Pitched>()
        .init_resource::<BazaarSettings>()
        .add_observer(merchant::sold)
        .add_observer(bustle::celebrate)
        .add_systems(
            Update,
            (
                (
                    repitch.run_if(resource_changed::<Pitched>),
                    pitch::pitch
                        .run_if(not(any_with_component::<Bazaar>)),
                )
                    .chain()
                    .in_set(Pitching),
                detail.run_if(looked),
                (merchant::tend, shoppers::stroll, keepers::tend)
                    .run_if(near)
                    .before(mascot::stand),
                lights::shine.run_if(lights::lit),
                lights::twinkle.run_if(near.and_then(lights::astir)),
                (bustle::spin, bustle::stir).run_if(near),
            )
                .in_set(RealmSystems::Act),
        );
}

/// The systems that stand the bazaar on its ground. They run once
/// the library has its own and before any town is founded.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Pitching;

/// The radius of the ground the bazaar keeps level and clear.
pub(crate) const GROUNDS: f32 = 15.0;

/// Reach of the bazaar from its middle and its height, for the ring
/// about it and the name over it.
const SIZE: (f32, f32) = (11.6, 8.6);

/// The spot of one who comes to the bazaar, from its middle: in the
/// plaza, a little before the merchant and to its side.
const VISITOR: Vec3 = Vec3::new(-1.1, 0.0, 4.5);

/// The spot of one who comes to a stall, from its middle: before its
/// counter, to the side its keeper is not on.
const CUSTOMER: Vec3 = Vec3::new(-0.35, 0.0, 2.3);

/// The reach of a stall from its middle and its height, for the box
/// it is picked by, and how far its box stands out before it.
const STALL: (f32, f32, f32) = (2.3, 3.6, 0.25);

/// The bazaar's lights, its people and what moves about it.
#[derive(Resource, Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BazaarSettings {
    /// The light of one of its lanterns after dark, and how far it
    /// reaches.
    pub lamp: f32,
    pub lamp_reach: f32,
    /// Brightness of the lanterns' glass by day and its gain after
    /// dark, and how many times brighter it burns after a sale.
    pub glass: (f32, f32),
    pub flash: f32,
    /// Brightness of what glows among the wares by day, and its gain
    /// after dark.
    pub gleam: (f32, f32),
    /// A fairy bulb's glow by day, its gain after dark and how many
    /// times brighter it burns after a sale.
    pub bulb: (f32, f32, f32),
    /// The rounds a second the fairy lights' chase runs along a
    /// string, and how many times faster it runs after a sale.
    pub chase: (f32, f32),
    /// The light of a glow over the avenue after dark, how far it
    /// reaches and the seconds it keeps each colour.
    pub glow: f32,
    pub glow_reach: f32,
    pub glow_holds: f32,
    /// The light under the pavilion that flares after a sale, at its
    /// brightest, and how far it reaches.
    pub flare: f32,
    pub flare_reach: f32,
    /// Seconds the merchant cheers after a sale.
    pub cheers: f32,
    /// Seconds a keeper waves once its stall is chosen, and takes to
    /// ease from one motion into the next.
    pub waves: f32,
    pub blend: f32,
    /// How fast a keeper turns, in radians a second.
    pub turn: f32,
    /// Farthest from where a visitor stands that a chief's mascot is
    /// taken for the visitor.
    pub near: f32,
    /// A shopper's walk in units a second, the seconds it takes to
    /// turn, and its size as a share of a chief's mascot.
    pub pace: f32,
    pub shopper_turn: f32,
    pub grown: f32,
    /// The motes of the sparks off the grindstone, the shimmer over
    /// the scrolls and the steam off the tea: how many, the seconds
    /// each lasts, its size and its light.
    pub sparks: (usize, f32, f32, f32),
    pub shimmer: (usize, f32, f32, f32),
    pub steam: (usize, f32, f32, f32),
    /// Embers that rise from the counter after a sale.
    pub embers: usize,
}

/// The spot a chief has set the bazaar down on. Without one it
/// stands where the land puts it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct Pitched(pub Option<Vec2>);

impl Pitched {
    /// The file for the bazaar's place under the window's folder.
    pub fn file(view: &std::path::Path) -> std::path::PathBuf {
        view.join("bazaar.place")
    }
}

/// The bazaar, and the middle of the ground it holds.
#[derive(Component, Clone, Copy, Debug)]
pub struct Bazaar {
    pub held: Vec2,
}

/// One stall of the bazaar, standing where it does in it.
#[derive(Component, Clone, Copy, Debug)]
pub struct Booth {
    pub stall: Stall,
}

/// The bazaar's meshes near and far, and whether the camera is close
/// enough for its small things to be drawn and to move.
#[derive(Component, Clone, Debug)]
struct Lod {
    near: Handle<Mesh>,
    far: Handle<Mesh>,
    close: bool,
}

/// What holds the small things of the bazaar or of a booth, all
/// hidden at once when the camera is not close.
#[derive(Component)]
struct Detail;

/// Takes the bazaar down when a chief has set it down elsewhere, so
/// that it is pitched anew where it now stands.
fn repitch(
    mut commands: Commands,
    pitched: Res<Pitched>,
    bazaars: Query<(Entity, &Bazaar)>,
    mut society: ResMut<Society>,
) {
    let Some(to) = pitched.0 else {
        return;
    };
    for (entity, bazaar) in &bazaars {
        if bazaar.held != to {
            society
                .claims
                .retain(|claim| claim.place != Ground::Bazaar);
            commands.entity(entity).try_despawn();
        }
    }
}

/// Brings the whole bazaar into view.
pub fn go_to(world: &mut World) {
    let held = world
        .query::<&Bazaar>()
        .iter(world)
        .next()
        .map(|bazaar| bazaar.held);
    if let Some(at) = held {
        pick::frame_round(world, at, SIZE.0);
    }
}

/// True when the camera moved, the bazaar is new or the distances
/// things are drawn at changed.
fn looked(
    set: Res<DetailSettings>,
    camera: Query<Ref<Transform>, With<WorldCamera>>,
    new: Query<(), Added<Lod>>,
) -> bool {
    set.is_changed()
        || !new.is_empty()
        || camera.iter().any(|at| at.is_changed())
}

/// True when a bazaar is in view and close enough for its small
/// things to be drawn.
fn near(
    bazaars: Query<(&ViewVisibility, &Lod), With<Bazaar>>,
) -> bool {
    bazaars.iter().any(|(seen, lod)| seen.get() && lod.close)
}

/// Draws the bazaar plain from far away and hides its small things
/// past the distance robots are hidden at, as the towns are.
fn detail(
    set: Res<DetailSettings>,
    camera: Single<&Transform, With<WorldCamera>>,
    mut bazaars: Query<
        (&Transform, &mut Lod, &mut Mesh3d),
        Without<WorldCamera>,
    >,
    mut small: Query<&mut Visibility, With<Detail>>,
) {
    for (at, mut lod, mut mesh) in &mut bazaars {
        let away = camera.translation.distance(at.translation);
        let shape = match away < set.town_far {
            true => &lod.near,
            false => &lod.far,
        };
        if mesh.0 != *shape {
            mesh.0 = shape.clone();
        }
        let close = away < set.robots_far;
        if lod.close != close {
            lod.close = close;
        }
        let want = match close {
            true => Visibility::Inherited,
            false => Visibility::Hidden,
        };
        for mut seen in &mut small {
            seen.set_if_neq(want);
        }
    }
}

#[cfg(test)]
mod tests;
