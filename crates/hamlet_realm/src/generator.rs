//! A world generated from the society's state
//! (docs/endless-world.md): land from a seed, a town per mayor, a
//! house and a robot per agent, a road per pair.

mod building;
mod common;
mod debug;
mod fireflies;
pub(crate) mod flora;
pub mod ground;
pub(crate) mod homing;
pub mod house;
mod kit;
pub mod land;
pub mod layout;
mod lights;
mod loading;
mod magic;
pub mod noise;
pub mod road;
mod shift;
pub mod society;
mod sounds;
mod sync;
#[cfg(test)]
pub(crate) mod testing;
mod traffic;
mod view;
mod wind;
mod work;
pub mod yard;

use bevy::prelude::*;
use hamlet_vfx::magic::MagicSettings;
pub(crate) use hamlet_vfx::magic::glow;
use hamlet_vfx::{Density, Now, Tints, VfxPlugin, VfxSystems};

pub use self::ground::DetailSettings;
use self::house::HouseSettings;
use self::kit::{Kit, setup, state_index};
use self::land::{Land, LandSettings};
use self::layout::TownSettings;
use self::lights::{light_lanterns, light_roads, light_spills};
pub use self::loading::{Ground, Loading};
use self::loading::{Lanes, clear_lanes, finish_loading, stream};
pub use self::magic::Thought;
use self::road::RoadSettings;
pub use self::shift::HouseMotion;
use self::shift::shift_houses;
use self::society::{AgentId, Pair, RoadLevel, Society, TownId};
use self::sync::{Drawn, reshaped, sync, tell_states};
use self::traffic::run_traffic;
pub use self::traffic::{Trip, Trips};
pub use self::view::{Ahead, GeneratorView};
use self::view::{aim, robot_detail, town_detail};
pub use self::work::{
    Floor, Objecting, Passage, Rehearsal, Robot, Voting,
};
use self::work::{pass, work_robots};
use self::yard::YardSettings;
use super::debug::shown;
use super::palette::Palette;
use super::{Clock, Sky, Step};
use crate::sky::{Quality, SkySettings};

/// What the land is grown from. Insert it before [`plugin`] to choose
/// the seed; without it the land grows from seed 7 and the settings
/// in the file.
#[derive(Resource, Clone)]
pub struct Origin {
    pub seed: u64,
    pub settings: GeneratorSettings,
}

impl Default for Origin {
    fn default() -> Self {
        Self {
            seed: 7,
            settings: GeneratorSettings::default(),
        }
    }
}

/// Draws whatever [`Society`] holds, and draws again only what a
/// change touches. Add it after [`crate::scene::plugin`]; the app
/// needs Bevy's default plugins.
pub fn plugin(app: &mut App) {
    app.init_resource::<Origin>()
        .init_resource::<DetailSettings>()
        .init_resource::<HouseMotion>()
        .init_resource::<MagicSettings>();
    let pal = app.world().resource::<Palette>().clone();
    let origin = app.world().resource::<Origin>().clone();
    app.insert_resource(tints(&pal));
    app.add_plugins((
        // It makes its materials at startup, from the tints above.
        VfxPlugin,
        lights::plugin,
        sounds::plugin,
    ))
    // An effect a house is given plays the frame it is given.
    .configure_sets(
        Update,
        VfxSystems
            .in_set(Step::Scene)
            .after(magic::charm)
            .before(run_traffic),
    )
    .add_systems(
        Update,
        (
            (
                tell_effects,
                thin_effects,
                tint_effects.run_if(resource_changed::<Palette>),
                wind::gusts,
            )
                .in_set(Step::Scene)
                .before(VfxSystems),
            fireflies::keep.after(VfxSystems),
        ),
    )
    .init_resource::<Society>()
    .init_resource::<Trips>()
    .init_resource::<Drawn>()
    .init_resource::<GeneratorView>()
    .init_resource::<Ahead>()
    .init_resource::<GeneratorOptions>()
    .init_resource::<Rehearsal>()
    .init_resource::<Voting>()
    .init_state::<Phase>()
    .init_resource::<Objecting>()
    .init_resource::<Loading>()
    .init_resource::<Lanes>()
    .insert_resource(Land::new(origin.seed, origin.settings, pal))
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            finish_loading,
            sync.run_if(
                reshaped
                    .or_eager(resource_changed::<Land>)
                    .or_eager(resource_changed::<GeneratorOptions>),
            ),
            tell_states.run_if(resource_changed::<Society>),
            shift_houses,
            magic::charm,
            run_traffic,
            (
                light_roads,
                light_lanterns,
                light_spills.run_if(
                    resource_changed::<Sky>
                        .or_else(resource_changed::<SkySettings>),
                ),
                town_detail,
                robot_detail,
                aim,
            ),
            (clear_lanes.run_if(resource_changed::<Drawn>), stream)
                .chain(),
            pass,
            work_robots,
            traffic::carts,
            magic::muse,
        )
            .chain()
            .in_set(Step::Scene),
    )
    .add_systems(
        PostUpdate,
        debug::draw.run_if(shown).after(TransformSystems::Propagate),
    );
}

/// Everything the generated world is shaped by. The land keeps the
/// copy it was built from, so a change means a new land.
#[derive(Clone, PartialEq, Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorSettings {
    pub land: LandSettings,
    pub town: TownSettings,
    pub house: HouseSettings,
    pub yard: YardSettings,
    pub road: RoadSettings,
}

/// Tells the effects the scene's time.
fn tell_effects(clock: Res<Clock>, mut now: ResMut<Now>) {
    now.0 = clock.t;
}

/// The colours of the palette the effects glow in.
fn tints(pal: &Palette) -> Tints {
    Tints {
        ember: pal.ember,
        firefly: pal.firefly,
        rune: pal.rune,
        wind: pal.wind,
    }
}

/// Gives the effects the palette's colours when it changes.
fn tint_effects(pal: Res<Palette>, mut effects: ResMut<Tints>) {
    effects.set_if_neq(tints(&pal));
}

/// Gives the effects the share of particles the quality allows.
fn thin_effects(quality: Res<Quality>, mut density: ResMut<Density>) {
    if density.0 != quality.density() {
        density.0 = quality.density();
    }
}

/// The world's phase: the ground and the trees loading, or in. Houses
/// and roads come and go in either.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum Phase {
    #[default]
    Loading,
    Ready,
}

/// A house, and whose it is.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct House {
    pub agent: AgentId,
    pub town: TownId,
    /// A ruin: the agent was removed or merged away.
    pub archived: bool,
}

/// A road, and the two agents it joins.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Road {
    pub pair: Pair,
    pub level: RoadLevel,
}

/// A town's common.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Common {
    pub town: TownId,
}

/// The parts left out, to see their cost.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct GeneratorOptions {
    pub robots: bool,
}

impl Default for GeneratorOptions {
    fn default() -> Self {
        Self { robots: true }
    }
}

/// Counts of what is drawn.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct Tally {
    pub towns: usize,
    pub houses: usize,
    pub robots: usize,
    pub roads: usize,
    pub road_length: f32,
    pub road_tris: usize,
    pub house_tris: usize,
    /// Bytes of mesh data the houses and the roads hold.
    pub house_bytes: usize,
    pub road_bytes: usize,
    /// Trees that stand as themselves.
    pub trees: usize,
    /// Tiles of ground that are drawn.
    pub tiles: usize,
}

impl Tally {
    fn count(&mut self, society: &Society, drawn: &Drawn) {
        self.towns = society.towns.len();
        self.houses = drawn.houses.len();
        self.robots = drawn.robots.len();
        self.roads = drawn.roads.len();
        self.road_length =
            drawn.roads.values().map(|r| r.route.total()).sum();
        self.road_tris =
            drawn.roads.values().map(|r| r.cost.tris).sum();
        self.road_bytes =
            drawn.roads.values().map(|r| r.cost.bytes).sum();
        self.house_tris =
            drawn.houses.values().map(|h| h.2.tris).sum();
        self.house_bytes =
            drawn.houses.values().map(|h| h.2.bytes).sum();
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Affine3A;

    use super::building::mesh_bytes;
    use super::house::TownStyle;
    use super::*;
    use crate::Stock;
    use crate::keeper::Kit;
    use crate::meshes::Meshing;

    /// Prints the mesh data each kind of thing holds. Run with
    /// `cargo test --release generator::tests::cost -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    #[expect(
        clippy::print_stdout,
        reason = "it prints its measurements, read with --nocapture"
    )]
    fn cost() {
        let land = Land::new(
            7,
            GeneratorSettings::default(),
            Palette::default(),
        );
        let detail = DetailSettings::default();
        for far in [72.0, 300.0, 1150.0] {
            let sight = ground::Sight {
                eye: Vec3::new(0.0, far * 0.64, far * 0.77),
                centre: Vec2::ZERO,
                reach: (far * 2.6 + 150.0) * detail.ground_reach,
            };
            let began = std::time::Instant::now();
            let tiles =
                ground::wanted(&land, &detail, &sight, true, |_| {
                    false
                });
            let drawn = tiles.iter().copied().collect();
            let ground = tiles
                .iter()
                .map(|tile| {
                    let recipe =
                        ground::Recipe::of(&land, &detail, *tile);
                    let sod = ground::sod(&land, recipe, &drawn);
                    mesh_bytes(&sod.mesh)
                })
                .sum::<usize>();
            let woods =
                ground::wanted(&land, &detail, &sight, false, |_| {
                    false
                });
            let (mut canopy, mut trees) = (0, 0);
            for tile in &woods {
                let stand = ground::Stand::of(&detail, *tile);
                let grove = ground::grove(&land, *tile, stand);
                canopy += grove.canopy.as_ref().map_or(0, mesh_bytes);
                trees += grove.trees.len();
            }
            println!(
                "from {far}: {} tiles of ground, {ground} bytes; {} \
                 of trees, {canopy} bytes and {trees} standing; \
                 {:?} on one thread",
                tiles.len(),
                woods.len(),
                began.elapsed()
            );
        }
        let trees = (0..flora::KINDS)
            .map(|k| mesh_bytes(&flora::mesh(&land.pal, k)))
            .sum::<usize>();
        println!("trees, {} meshes: {trees} bytes", flora::KINDS);
        let style = TownStyle::of(&land, 0);
        let common = common::mesh(&land, 0, &style);
        println!("a common: {} bytes", common.bytes());
        let yard = house::yard_of(&land, AgentId(0), true, 0.0, 0);
        let hall = house::house(
            &land,
            AgentId(0),
            &style,
            true,
            house::Banner::Flown,
            0,
            &yard,
        );
        println!(
            "a hall: {} bytes",
            hall.body.bytes() + hall.panes.bytes()
        );
        let (mut far, mut panes) = (Meshing::new(), Meshing::new());
        house::far(
            &mut far,
            &mut panes,
            Affine3A::IDENTITY,
            &land,
            AgentId(1),
            &style,
            false,
            None,
        );
        println!(
            "a house from far: {} bytes",
            far.bytes() + panes.bytes()
        );

        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let stock = Stock::new(&mut materials, &land.pal);
        Kit::new(&mut meshes, &mut materials, &stock, &land.pal);
        let kit =
            meshes.iter().map(|(_, m)| mesh_bytes(m)).sum::<usize>();
        println!(
            "the robots' kit, {} meshes: {kit} bytes",
            meshes.len()
        );
    }
}
