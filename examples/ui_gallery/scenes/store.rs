//! The Hamlet Store's scenes: its stall selected beside the bazaar's
//! panel at the HAMLETS tab, with the dioramas the world draws for
//! them, and one hamlet opened on its own page.

use bevy::app::{App, PostStartup, Update};
use bevy::camera::Camera;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button};
use bevy_fynix::dock::DockTree;
use hamlet_model::market::hamlets::Listing;
use hamlet_model::market::lot::Kind;
use hamlet_model::market::samples;
use hamlet_ui::bazaar::StallShown;
use hamlet_world::WorldCamera;
use hamlet_world::bazaar::Stall;
use hamlet_world::diorama::{Dioramas, Model};
use hamlet_world::generator::GenSettings;
use hamlet_world::generator::land::Land;
use hamlet_world::palette::Palette;
use hamlet_world::pick::{Selected, Thing};

use super::Scene;
use super::dock::seed_dock;

/// The hamlet opened on its own page, by its listing's id.
const OPENED: &str = "l_lamplight";

/// The update its row is pressed on, once the panels are built.
const PRESS: u32 = 8;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        hamlet_world::base,
        hamlet_world::diorama::plugin,
    ));
    let pal = app.world().resource::<Palette>().clone();
    app.insert_resource(Land::new(7, GenSettings::default(), pal))
        .insert_resource(super::stalls::stocked())
        .insert_resource(StallShown(Stall::Hamlets))
        .add_systems(
            PostStartup,
            (at_the_stall, unseen).after(seed_dock),
        )
        .add_systems(
            Update,
            open.run_if(|scene: Res<Scene>| {
                *scene == Scene::StoreHamlet
            }),
        );
}

/// The bazaar's panel over the world, and the stall selected so that
/// the Inspector shows its counter.
fn at_the_stall(world: &mut World) {
    world.insert_resource(Selected(Some(Thing::Stall(
        Stall::Hamlets,
    ))));
    let mut tree = world.resource_mut::<DockTree>();
    if let Some(centre) = tree.find_leaf_with_window("world") {
        tree.add_tab(centre, "bazaar");
    }
}

/// The world's own camera draws nothing here: a picture of the world
/// stands in its place.
fn unseen(mut cameras: Query<&mut Camera, With<WorldCamera>>) {
    for mut camera in &mut cameras {
        camera.is_active = false;
    }
}

/// Presses the row of the hamlet opened, and turns its diorama.
fn open(
    mut commands: Commands,
    texts: Query<(Entity, &Text)>,
    parents: Query<&ChildOf>,
    buttons: Query<(), With<Button>>,
    mut update: Local<u32>,
) {
    *update += 1;
    if *update != PRESS {
        return;
    }
    let Some(listing) = samples::lots(Kind::Hamlet)
        .iter()
        .find(|lot| lot.id == OPENED)
        .map(Listing::of)
    else {
        return;
    };
    let row = texts
        .iter()
        .filter(|(_, text)| text.0 == listing.title)
        .find_map(|(text, _)| {
            parents
                .iter_ancestors(text)
                .find(|ancestor| buttons.contains(*ancestor))
        });
    if let Some(row) = row {
        commands.trigger(Activate { entity: row });
    }
    let model = Model {
        seed: listing.seed,
        agents: listing.agents.len() as u32 + 1,
    };
    commands.queue(move |world: &mut World| {
        world.resource_mut::<Dioramas>().turning = Some(model);
    });
}
