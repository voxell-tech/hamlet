//! The server's timers. Each area adds its own: a system that says
//! when it is next due, or a period, and a system that runs then. The
//! runner sleeps until the soonest, so nothing runs on a tick.

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use bevy_app::App;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{IntoSystem, SystemId};
use bevy_ecs::world::World;

/// The system a timer runs when it is due.
pub(crate) type Fire = SystemId<(), Result<()>>;

/// A timer's next due time.
enum When {
    /// As a system of its area says; none when nothing waits.
    Asked(SystemId<(), Option<Instant>>),
    /// Every period, from the server's start.
    Every { period: Duration, next: Instant },
}

#[derive(Resource, Default)]
pub(crate) struct Deadlines(Vec<(When, Fire)>);

/// Adds timers to an app.
pub(crate) trait AddDeadline {
    /// Runs `fire` when `due` says.
    fn add_deadline<M, N>(
        &mut self,
        due: impl IntoSystem<(), Option<Instant>, M> + 'static,
        fire: impl IntoSystem<(), Result<()>, N> + 'static,
    ) -> &mut Self;

    /// Runs `fire` every `period`.
    fn add_every<N>(
        &mut self,
        period: Duration,
        fire: impl IntoSystem<(), Result<()>, N> + 'static,
    ) -> &mut Self;
}

impl AddDeadline for App {
    fn add_deadline<M, N>(
        &mut self,
        due: impl IntoSystem<(), Option<Instant>, M> + 'static,
        fire: impl IntoSystem<(), Result<()>, N> + 'static,
    ) -> &mut Self {
        let world = self.world_mut();
        let due = world.register_system(due);
        let fire = world.register_system(fire);
        world
            .get_resource_or_init::<Deadlines>()
            .0
            .push((When::Asked(due), fire));
        self
    }

    fn add_every<N>(
        &mut self,
        period: Duration,
        fire: impl IntoSystem<(), Result<()>, N> + 'static,
    ) -> &mut Self {
        let world = self.world_mut();
        let fire = world.register_system(fire);
        let next = Instant::now() + period;
        world
            .get_resource_or_init::<Deadlines>()
            .0
            .push((When::Every { period, next }, fire));
        self
    }
}

/// Each timer's due time, as the world says now.
fn whens(world: &mut World) -> Result<Vec<(Option<Instant>, Fire)>> {
    let timers = world
        .get_resource::<Deadlines>()
        .map(|deadlines| {
            deadlines
                .0
                .iter()
                .map(|(when, fire)| match when {
                    When::Asked(due) => (Some(*due), None, *fire),
                    When::Every { next, .. } => {
                        (None, Some(*next), *fire)
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    timers
        .into_iter()
        .map(|(due, next, fire)| {
            let at = match due {
                Some(due) => world
                    .run_system(due)
                    .map_err(|error| anyhow!("{error}"))?,
                None => next,
            };
            Ok((at, fire))
        })
        .collect()
}

/// The soonest any timer is due.
pub(crate) fn next(world: &mut World) -> Result<Option<Instant>> {
    Ok(whens(world)?.into_iter().filter_map(|(at, _)| at).min())
}

/// The timers due by `now`, each to run as an input of its own; a
/// periodic one is next due a period on.
pub(crate) fn due(
    world: &mut World,
    now: Instant,
) -> Result<Vec<Fire>> {
    let due = whens(world)?
        .into_iter()
        .filter(|(at, _)| at.is_some_and(|at| at <= now))
        .map(|(_, fire)| fire)
        .collect::<Vec<_>>();
    if let Some(mut deadlines) = world.get_resource_mut::<Deadlines>()
    {
        for (when, fire) in &mut deadlines.0 {
            if let When::Every { period, next } = when
                && due.contains(fire)
            {
                *next = now + *period;
            }
        }
    }
    Ok(due)
}
