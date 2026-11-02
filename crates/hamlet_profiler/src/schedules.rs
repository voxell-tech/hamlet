//! Which schedule each system is in, which tells the recorder where
//! a system run on a task pool's thread belongs.

use bevy::ecs::schedule::Schedules;
use bevy::ecs::system::Local;
use bevy::ecs::world::World;
use bevy::render::MainWorld;

use crate::Profiler;

/// Tells the recorder the systems of every schedule of the main and
/// render worlds, whenever they change. It runs in the render world's
/// extract, the one time no main schedule runs; only the extract's
/// own schedule is out of reach then, and its systems run on the main
/// thread inside it.
pub(crate) fn publish(world: &World, mut seen: Local<usize>) {
    let main = &**world.resource::<MainWorld>();
    let count = |world: &World| {
        world.get_resource::<Schedules>().map_or(0, |schedules| {
            schedules
                .iter()
                .map(|(_, schedule)| {
                    schedule.systems_len()
                        + usize::from(schedule.systems().is_ok())
                })
                .sum()
        })
    };
    let count = count(main) + count(world);
    if count == *seen {
        return;
    }
    *seen = count;
    let mut recorder = world.resource::<Profiler>().lock();
    for schedules in [main, world]
        .into_iter()
        .filter_map(|world| world.get_resource::<Schedules>())
    {
        for (_, schedule) in schedules.iter() {
            let Ok(systems) = schedule.systems() else {
                continue;
            };
            let names = systems
                .map(|(_, system)| system.name().to_string())
                .collect::<Vec<_>>();
            recorder.add_schedule(
                &format!("{:?}", schedule.label()),
                names.iter().map(String::as_str),
            );
        }
    }
}
