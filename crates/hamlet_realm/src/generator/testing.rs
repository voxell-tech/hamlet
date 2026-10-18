//! The shared parts of the generator's tests.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use super::land::Land;
use super::society::AgentId;
use super::sync::Drawn;
use super::traffic::Traffic;
use super::work::{
    Engine, Lot, Rehearsal, Robot, Voting, pass, work_robots,
};
use super::{DetailSettings, GenSettings, Kit, flora, house, layout};
use crate::keeper::work::Work;
use crate::keeper::{self, KState, Motion};
use crate::palette::Palette;
use crate::{Clock, Stock};

/// The land as designed, from `seed`.
pub(crate) fn land(seed: u64) -> Land {
    Land::new(seed, GenSettings::default(), Palette::default())
}

/// Gives a world with nothing to draw on the shared materials and a
/// kit of empty meshes.
pub(super) fn kit(world: &mut World) {
    crate::stock::stock(world);
    let stock = world.resource::<Stock>().clone();
    world.insert_resource(Kit {
        matte: stock.matte,
        windows: stock.windows,
        lamps: [(); 5].map(|_| Handle::default()),
        lamp_glass: Handle::default(),
        wheel: Handle::default(),
        ring: Handle::default(),
        desk: Handle::default(),
        engine: Handle::default(),
        cart: Handle::default(),
        water: Handle::default(),
        pond: Handle::default(),
        trees: vec![Handle::default(); flora::KINDS],
        groove: Handle::default(),
    });
}

/// A world in which robots move with nothing drawn.
pub(crate) fn rehearse(app: &mut App) {
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<hamlet_vfx::mark::MarkMaterial>>()
        .init_resource::<hamlet_vfx::mark::Marks>()
        .add_message::<bevy::window::RequestRedraw>()
        .init_resource::<Clock>()
        .init_resource::<crate::Stage>()
        .init_resource::<Drawn>()
        .init_resource::<Traffic>()
        .init_resource::<Rehearsal>()
        .init_resource::<Voting>()
        .init_resource::<crate::Sky>()
        .init_resource::<DetailSettings>()
        .init_resource::<Motion>()
        .insert_resource(land(7))
        .add_systems(
            Update,
            (pass, work_robots, keeper::animate)
                .chain()
                .in_set(crate::WorldSystems),
        );
    crate::stock::stock(app.world_mut());
}

/// Stands the robot of `agent` at its porch on plot `number` of the
/// first town.
pub(crate) fn stand_in(
    world: &mut World,
    agent: AgentId,
    number: u32,
) -> Entity {
    let spawn = move |mut commands: Commands,
                      kit: Res<keeper::Kit>,
                      mut materials: ResMut<
        Assets<StandardMaterial>,
    >,
                      land: Res<Land>| {
        let spot = layout::plot(&land, 0, number);
        let yard = house::yard_of(&land, agent, false, spot.yaw, 0);
        let lot = Lot::new(&land, &spot, yard);
        let robot = keeper::spawn_agent(
            &mut commands,
            &kit,
            &mut materials,
            keeper::Who::citizen(20 + number),
            Transform::from_translation(lot.porch),
            None,
        );
        let mut part = || {
            commands
                .spawn((Transform::default(), Visibility::Hidden))
                .id()
        };
        let engine = Engine {
            wheel: part(),
            rings: [part(), part()],
        };
        commands.entity(robot).insert(Robot::new(
            agent,
            KState::Rest,
            Work::default(),
            lot,
            engine,
        ));
        robot
    };
    world.run_system_once(spawn).unwrap()
}
