//! What a service keeps while the person is asked about a first use.

use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use hamlet_engine_api::Capability;

use crate::petition::{Decided, FirstUse};

/// Items waiting for the person to answer whether a plugin may use
/// what each needs.
pub struct Waiting<T>(Vec<(Entity, Capability, String, T)>);

impl<T> Default for Waiting<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> Waiting<T> {
    /// Keeps `item` until the person answers whether `plugin` may use
    /// `capability` on `target`, and asks them.
    pub fn park(
        &mut self,
        plugin: Entity,
        capability: Capability,
        target: String,
        item: T,
        first_use: &mut MessageWriter<FirstUse>,
    ) {
        first_use.write(FirstUse {
            plugin,
            capability,
            target: target.clone(),
        });
        self.0.push((plugin, capability, target, item));
    }

    /// Takes the items the person answered for in `decided`, each
    /// with whether it may go on.
    pub fn settle<'a>(
        &mut self,
        decided: impl Iterator<Item = &'a Decided>,
    ) -> Vec<(T, bool)> {
        let mut settled = Vec::new();
        for decision in decided {
            let mut at = 0;
            while at < self.0.len() {
                let (plugin, capability, target, _) = &self.0[at];
                if *plugin == decision.plugin
                    && *capability == decision.capability
                    && *target == decision.target
                {
                    let (.., item) = self.0.remove(at);
                    settled.push((item, decision.allowed));
                } else {
                    at += 1;
                }
            }
        }
        settled
    }

    /// Lets go of what waits for plugins that `there` says are gone.
    pub fn keep(&mut self, there: impl Fn(Entity) -> bool) {
        self.0.retain(|(plugin, ..)| there(*plugin));
    }
}
