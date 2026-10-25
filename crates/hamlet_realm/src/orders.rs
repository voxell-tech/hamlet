//! The chief's deeds by hand, carried to the server as commands, and
//! the moves of what the hand set down.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use hamlet_client::realm::{self as agents, Deeds, HOME, HamletId};
use hamlet_client::{ViewFolder, gifts};

use crate::bazaar::Pitched;
use crate::chief::{Kept, Player, Stands};
use crate::generator::society::Ground;
use crate::hamlet::{hamlet_of, town_of};
use crate::hand::{self, Wish};
use crate::library::Stood;
use crate::moving::{self, SetDown};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, grant);
}

/// A chief's command for the hamlet `hamlet`, as the chief would
/// type it.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Decree {
    pub hamlet: String,
    pub text: String,
}

/// Words from the chief to `agent`, in its own chat.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Tell {
    pub agent: Entity,
    pub text: String,
}

fn name(world: &World, agent: Entity) -> Option<String> {
    world
        .get::<Name>(agent)
        .map(|name| name.as_str().to_string())
}

/// A chief's command sent to the mayor of `agent`'s hamlet.
pub fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet =
        agents::hamlet_of(world, agent).unwrap_or(HOME).to_string();
    world.trigger(Decree { hamlet, text });
    world.flush();
}

/// Builds a road between two agents at the chief's word.
fn road(world: &mut World, a: Entity, b: Entity) {
    if !Deeds::is_allowed(world, "road") {
        return;
    }
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/road {a_name} {b_name}"));
    }
}

/// Takes up the road between two agents at the chief's word.
pub fn part(world: &mut World, a: Entity, b: Entity) {
    if !Deeds::is_allowed(world, "unroad") {
        return;
    }
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/unroad {a_name} {b_name}"));
    }
}

/// Has two agents greet each other and builds a road between them.
pub fn introduce(world: &mut World, a: Entity, b: Entity) {
    hand::greet(world, a, b);
    road(world, a, b);
}

/// Takes the town of `hamlet` in hand, to be set down with a click
/// on the ground.
pub fn carry(world: &mut World, hamlet: Entity) {
    if !Deeds::is_allowed(world, "move-hamlet") {
        return;
    }
    if let Some(town) = town_of(world, hamlet) {
        moving::pick_up(world, Ground::Town(town));
    }
}

/// Records the place a chief set something down. The world moves it
/// when the record says so.
fn set_down(world: &mut World, set: SetDown) {
    match set.place {
        Ground::Town(town) => {
            if !Deeds::is_allowed(world, "move-hamlet") {
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
            let (x, z) = (set.to.x, set.to.y);
            world.trigger(Decree {
                text: format!("/move-hamlet {name} {x:.1} {z:.1}"),
                hamlet: name,
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
            if keep(world, Stood::file, set.to) {
                world.resource_mut::<Kept>().library = Some(set.to);
            }
            world.resource_mut::<Stood>().0 = Some(set.to);
        }
        Ground::Bazaar => {
            if keep(world, Pitched::file, set.to) {
                world.resource_mut::<Kept>().bazaar = Some(set.to);
            }
            world.resource_mut::<Pitched>().0 = Some(set.to);
        }
    }
}

/// Writes `to` to the file `file` names under the window's folder,
/// where the place is read back from at the next launch. False with
/// no folder.
fn keep(world: &World, file: fn(&Path) -> PathBuf, to: Vec2) -> bool {
    let Some(view) = world.get_resource::<ViewFolder>() else {
        return false;
    };
    let _ = Stands(to).write_at(&file(&view.0));
    true
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
    let mut letters = Vec::<(Entity, Vec<PathBuf>)>::new();
    for wish in wishes.read().cloned() {
        match wish {
            Wish::Road { a, b } => commands
                .queue(move |world: &mut World| road(world, a, b)),
            Wish::Letter { to, path } => {
                if let Some((_, paths)) =
                    letters.iter_mut().find(|(agent, _)| *agent == to)
                {
                    paths.push(path);
                } else {
                    letters.push((to, vec![path]));
                }
            }
        }
    }
    for (to, paths) in letters {
        commands.queue(move |world: &mut World| {
            gifts::letter(world, to, paths);
        });
    }
}

#[cfg(test)]
mod tests {
    use hamlet_client::ClientPlugin;
    use hamlet_testing::Scratch;

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

    #[test]
    fn setting_down_the_bazaar_keeps_its_own_place_in_the_view_folder()
     {
        let dir = Scratch::new("world-bazaar-view");
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None })
            .add_message::<Wish>()
            .add_message::<SetDown>()
            .add_plugins(plugin)
            .insert_resource(ViewFolder(dir.to_path_buf()))
            .init_resource::<Kept>()
            .init_resource::<Stood>()
            .init_resource::<Pitched>();
        app.world_mut().write_message(SetDown {
            place: Ground::Bazaar,
            to: Vec2::new(-82.5, 140.25),
        });
        app.update();
        let bazaar = Some(Vec2::new(-82.5, 140.25));
        assert_eq!(Kept::read(&dir).unwrap().bazaar, bazaar);
        assert_eq!(app.world().resource::<Pitched>().0, bazaar);
        // The library's place is its own.
        assert_eq!(Kept::read(&dir).unwrap().library, None);
        assert_eq!(app.world().resource::<Stood>().0, None);
    }
}
