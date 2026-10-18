//! How many times a plugin may do something in a minute.

use std::collections::HashMap;

use bevy::ecs::entity::Entity;

/// The seconds a count is kept for.
const WINDOW: f32 = 60.0;

/// What each plugin did lately, counted over a minute by the app's
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
        let (since, count) = self.0.entry(plugin).or_insert((now, 0));
        if now - *since >= WINDOW || now < *since {
            (*since, *count) = (now, 0);
        }
        if *count >= per_minute {
            return false;
        }
        *count += 1;
        true
    }
}
