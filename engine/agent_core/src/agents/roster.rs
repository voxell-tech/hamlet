//! The agents and the hamlets looked up by name.

use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use bevy_ecs::system::{SystemParam, SystemState};
use hamlet_rules::hamlet::{HOME, MAYOR};

use super::{Citizen, Left, LivesIn, Mayor, Settlement};

/// The name of `entity`, empty if it has none.
pub fn name_of(world: &World, entity: Entity) -> String {
    world
        .get::<Name>(entity)
        .map(|name| name.as_str().to_string())
        .unwrap_or_default()
}

/// The agents and the hamlets, for a system that looks one up by
/// name.
#[derive(SystemParam)]
pub struct Roster<'w, 's> {
    living: Query<
        'w,
        's,
        (Entity, &'static Name),
        (With<Citizen>, Without<Left>),
    >,
    mayors: Query<
        'w,
        's,
        (Entity, &'static LivesIn),
        (With<Mayor>, Without<Left>),
    >,
    settlements: Query<'w, 's, (Entity, &'static Settlement)>,
}

impl Roster<'_, '_> {
    /// The living agent called `name` in whichever hamlet, or the
    /// mayor of the first hamlet for the office.
    pub fn named(&self, name: &str) -> Option<Entity> {
        if name == MAYOR {
            return self.mayor();
        }
        self.living
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(entity, _)| entity)
    }

    /// The agent holding the mayor's office in the first hamlet.
    pub fn mayor(&self) -> Option<Entity> {
        self.mayor_of(HOME)
    }

    /// The agent holding the mayor's office in the hamlet `name`.
    pub fn mayor_of(&self, name: &str) -> Option<Entity> {
        let hamlet = self
            .settlements
            .iter()
            .find(|(_, settlement)| settlement.name == name)
            .map(|(entity, _)| entity)?;
        self.mayors
            .iter()
            .find(|(_, lives)| lives.0 == hamlet)
            .map(|(entity, _)| entity)
    }
}

/// The [`Roster`] that exclusive code asks, kept between calls.
#[derive(Resource, Default)]
struct Asking(Option<SystemState<Roster<'static, 'static>>>);

/// Puts `question` to the [`Roster`] of `world`.
pub fn ask<T>(
    world: &mut World,
    question: impl FnOnce(&Roster) -> T,
) -> Option<T> {
    let kept = world.get_resource_or_init::<Asking>().0.take();
    let mut state = kept.unwrap_or_else(|| SystemState::new(world));
    let answer =
        state.get(world).ok().map(|roster| question(&roster));
    world.resource_mut::<Asking>().0 = Some(state);
    answer
}

/// The living agent called `name` in whichever hamlet, or the mayor
/// of the first hamlet for the office.
pub fn named(world: &mut World, name: &str) -> Option<Entity> {
    ask(world, |roster| roster.named(name)).flatten()
}

/// The agent holding the mayor's office in the first hamlet.
pub fn mayor(world: &mut World) -> Option<Entity> {
    mayor_of(world, HOME)
}

/// The agent holding the mayor's office in the hamlet `name`.
pub fn mayor_of(world: &mut World, name: &str) -> Option<Entity> {
    ask(world, |roster| roster.mayor_of(name)).flatten()
}

/// The name of the hamlet `agent` lives in.
pub fn hamlet_of(world: &World, agent: Entity) -> Option<&str> {
    let lives = world.get::<LivesIn>(agent)?;
    Some(world.get::<Settlement>(lives.0)?.name.as_str())
}

pub(super) fn settlements(
    world: &mut World,
) -> Vec<(Entity, Settlement)> {
    let all = |roster: &Roster| {
        roster
            .settlements
            .iter()
            .map(|(entity, settlement)| (entity, settlement.clone()))
            .collect()
    };
    ask(world, all).unwrap_or_default()
}
