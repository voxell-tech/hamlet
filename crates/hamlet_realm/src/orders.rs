//! The chief's actions by hand, carried to the server as commands,
//! and the moves of what the hand set down.

use bevy::prelude::*;
use hamlet_core::client::ViewFolder;
use hamlet_core::client::realm::{Actions, HamletId, Profile};
use hamlet_core::orders::{
    Carry, CarryHouse, Decree, Introduce, channel,
};

use crate::chief::{Kept, Player, Stands};
use crate::generator::land::{IslandId, Land};
use crate::generator::society::Ground;
use crate::hamlet::{hamlet_of, town_of};
use crate::hand::{self, Wish};
use crate::library::Stood;
use crate::moving::{self, SetDown};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, grant)
        .add_observer(
            |asked: On<Introduce>, mut commands: Commands| {
                let Introduce(a, b) = *asked;
                commands.queue(move |world: &mut World| {
                    introduce(world, a, b);
                });
            },
        )
        .add_observer(|asked: On<Carry>, mut commands: Commands| {
            let hamlet = asked.0;
            commands
                .queue(move |world: &mut World| carry(world, hamlet));
        })
        .add_observer(
            |asked: On<CarryHouse>, mut commands: Commands| {
                let profile = asked.0;
                commands.queue(move |world: &mut World| {
                    carry_house(world, profile);
                });
            },
        );
}

/// Has two agents greet each other and opens a channel between them.
fn introduce(world: &mut World, a: Entity, b: Entity) {
    hand::greet(world, a, b);
    channel(world, a, b);
}

/// Takes the town of `hamlet` in hand, to be set down with a click
/// on the ground.
fn carry(world: &mut World, hamlet: Entity) {
    if !Actions::is_allowed(world, "move-hamlet") {
        return;
    }
    if let Some(town) = town_of(world, hamlet) {
        moving::pick_up(world, Ground::Town(town));
    }
}

/// Takes the house of `profile` in hand, to be set down with a click
/// on the ground.
fn carry_house(world: &mut World, profile: Entity) {
    if !Actions::is_allowed(world, "move-house") {
        return;
    }
    if let Some(profile) = world.get::<Profile>(profile) {
        let id = profile.0.id;
        moving::pick_up(world, Ground::House(id));
    }
}

/// Records the place a chief set something down. The world moves it
/// when the record says so.
fn set_down(world: &mut World, set: SetDown) {
    match set.place {
        Ground::Town(town) => {
            if !Actions::is_allowed(world, "move-hamlet") {
                return;
            }
            let Some(hamlet) = hamlet_of(world, town) else {
                return;
            };
            let Some(name) =
                world.get::<HamletId>(hamlet).map(|id| id.0.clone())
            else {
                return;
            };
            // The record keeps a town's place from its island's
            // middle, which moves.
            let to =
                set.to - world.resource::<Land>().hamlet_centre();
            let (x, z) = (to.x, to.y);
            world.trigger(Decree {
                text: format!("/move-hamlet {name} {x:.1} {z:.1}"),
                hamlet: Some(name),
            });
            world.flush();
        }
        Ground::Chief => {
            let player = world
                .query_filtered::<Entity, With<Player>>()
                .iter(world)
                .next();
            if let Some(chief) = player {
                world.entity_mut(chief).insert(Stands(set.to));
            }
        }
        Ground::Library => {
            if let Some(view) = world.get_resource::<ViewFolder>() {
                let file = Stood::file(&view.0);
                let _ = Stands(set.to).write_at(&file);
                world.resource_mut::<Kept>().library = Some(set.to);
            }
            world.resource_mut::<Stood>().0 = Some(set.to);
        }
        Ground::House(id) => {
            if !Actions::is_allowed(world, "move-house") {
                return;
            }
            let turn = world
                .query::<&Profile>()
                .iter(world)
                .find(|profile| profile.0.id == id)
                .and_then(|profile| {
                    Some(profile.0.house.as_ref()?.turn)
                });
            let Some(turn) = turn else {
                return;
            };
            let name =
                hamlet_core::client::realm::profile_address(id);
            let city = world
                .resource::<Land>()
                .island(IslandId::City)
                .centre;
            let to = set.to - city;
            let (x, z) = (to.x, to.y);
            let degrees = turn.to_degrees();
            world.trigger(Decree {
                hamlet: None,
                text: format!(
                    "/move-house {name} {x:.1} {z:.1} {degrees:.0}"
                ),
            });
            world.flush();
        }
        // Nothing carries it, so nothing sets it down.
        Ground::Tower => {}
    }
}

/// Carries out everything the hand asked for.
fn grant(
    mut set: MessageReader<SetDown>,
    mut wishes: MessageReader<Wish>,
    mut commands: Commands,
) {
    for set in set.read().copied() {
        commands.queue(move |world: &mut World| set_down(world, set));
    }
    for wish in wishes.read().cloned() {
        match wish {
            Wish::Channel { a, b } => commands
                .queue(move |world: &mut World| channel(world, a, b)),
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_core::client::ClientPlugin;
    use hamlet_core::testing::Scratch;

    use super::*;

    #[test]
    fn setting_down_the_library_keeps_its_place_in_the_view_folder() {
        let dir = Scratch::new("world-library-view");
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None })
            .add_message::<Wish>()
            .add_message::<SetDown>()
            .add_plugins(plugin)
            .insert_resource(ViewFolder(dir.to_path_buf()))
            .init_resource::<Kept>()
            .init_resource::<Stood>();
        app.world_mut().write_message(SetDown {
            place: Ground::Library,
            to: Vec2::new(12.5, -40.0),
        });
        app.update();
        let library = Some(Vec2::new(12.5, -40.0));
        assert_eq!(Kept::read(&dir).unwrap().library, library);
        assert_eq!(app.world().resource::<Kept>().library, library);
    }
}
