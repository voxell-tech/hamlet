//! The chief's orders about an agent, given as a command to the
//! hamlet it lives in and said in the chief's chat.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use hamlet_agents_api::agents;
use hamlet_agents_api::hamlet::HOME;
use hamlet_realm_api::orders::Decree;

/// Gives a chief's command to the hamlet of `agent`, carried out by
/// the time this returns.
pub fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet =
        agents::hamlet_of(world, agent).unwrap_or(HOME).to_string();
    world.trigger(Decree { hamlet, text });
    world.flush();
}

/// Brings back `agent`, which was removed or merged away.
pub fn restore(world: &mut World, agent: Entity) {
    let text = format!("/restore {}", agents::name_of(world, agent));
    decree(world, agent, text);
}

/// Takes up the road between two agents at the chief's word.
pub fn part(world: &mut World, a: Entity, b: Entity) {
    if let (Some(a_name), Some(b_name)) =
        (world.get::<Name>(a), world.get::<Name>(b))
    {
        let text = format!("/unroad {a_name} {b_name}");
        decree(world, a, text);
    }
}
