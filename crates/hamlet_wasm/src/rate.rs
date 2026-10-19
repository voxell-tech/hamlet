//! How many times a plugin may do something in a minute or a second.

use std::collections::HashMap;

use bevy::ecs::entity::Entity;

/// The seconds a count is kept for, in a minute.
const MINUTE: f32 = 60.0;

/// What each plugin did lately, counted over a window by the app's
/// clock.
#[derive(Default)]
pub struct Rate(HashMap<Entity, (f32, u32)>);

impl Rate {
    /// Whether the plugin may do one more at `now`, seconds by the
    /// app's clock, when it may do `per_minute` in each minute. One
    /// it may not do is counted out.
    pub fn admit(
        &mut self,
        plugin: Entity,
        now: f32,
        per_minute: u32,
    ) -> bool {
        self.within(plugin, now, MINUTE, per_minute)
    }

    /// Whether the plugin may do one more at `now` when it may do
    /// `most` in each `window` seconds.
    pub fn within(
        &mut self,
        plugin: Entity,
        now: f32,
        window: f32,
        most: u32,
    ) -> bool {
        let (since, count) = self.0.entry(plugin).or_insert((now, 0));
        if now - *since >= window || now < *since {
            (*since, *count) = (now, 0);
        }
        if *count >= most {
            return false;
        }
        *count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_is_admitted_once_in_a_window_for_each_count() {
        let mut world = bevy::ecs::world::World::new();
        let (a, b) =
            (world.spawn_empty().id(), world.spawn_empty().id());
        let mut rate = Rate::default();
        assert!(rate.admit(a, 0.0, 1));
        assert!(!rate.admit(a, 30.0, 1));
        assert!(rate.admit(b, 30.0, 1));
        assert!(rate.admit(a, 61.0, 1));
    }
}
