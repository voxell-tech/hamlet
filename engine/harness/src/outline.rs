//! A panel as text: a line for each node, its key after `#`,
//! indented under the node that holds it, and a line for each text
//! and picture it shows.
//!
//! ```text
//! #3
//!   "Board"
//! #4
//!   picture 256x256
//! ```

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::world::World;
use bevy::ui::widget::Text;
use hamlet_engine_wasm::{NodeOf, ShownPicture};

/// The harness's methods on [`World`].
pub trait HarnessWorldExt {
    /// The panel under `root` as an outline.
    fn outline(&self, root: Entity) -> String;

    /// The nearest entity with a `C` from `entity` up to `root`, if
    /// `entity` is under `root`.
    fn holder<C: Component>(
        &self,
        entity: Entity,
        root: Entity,
    ) -> Option<Entity>;

    /// The first entity with a `C` under the nearest of `entity` and
    /// what holds it, up to `root`, that has one under it.
    fn nearest<C: Component>(
        &self,
        entity: Entity,
        root: Entity,
    ) -> Option<Entity>;
}

impl HarnessWorldExt for World {
    fn outline(&self, root: Entity) -> String {
        let mut lines = Vec::new();
        let mut open = vec![(root, 0)];
        while let Some((entity, depth)) = open.pop() {
            let mut inner = depth;
            if let Some(node) = self.get::<NodeOf>(entity) {
                lines.push(format!(
                    "{}#{}",
                    "  ".repeat(depth),
                    node.key
                ));
                inner += 1;
            }
            if let Some(text) = self.get::<Text>(entity) {
                lines.push(format!(
                    "{}{:?}",
                    "  ".repeat(inner),
                    text.0
                ));
            }
            if let Some(picture) = self.get::<ShownPicture>(entity) {
                lines.push(format!(
                    "{}picture {}x{}",
                    "  ".repeat(inner),
                    picture.0.x,
                    picture.0.y
                ));
            }
            let children =
                self.get::<Children>(entity).into_iter().flatten();
            open.extend(children.rev().map(|&child| (child, inner)));
        }
        lines.join("\n")
    }

    fn holder<C: Component>(
        &self,
        entity: Entity,
        root: Entity,
    ) -> Option<Entity> {
        let mut found = None;
        let mut at = entity;
        while at != root {
            if found.is_none() && self.get::<C>(at).is_some() {
                found = Some(at);
            }
            at = self.get::<ChildOf>(at)?.parent();
        }
        found
    }

    fn nearest<C: Component>(
        &self,
        entity: Entity,
        root: Entity,
    ) -> Option<Entity> {
        let mut at = entity;
        loop {
            let mut open = vec![at];
            while let Some(below) = open.pop() {
                if self.get::<C>(below).is_some() {
                    return Some(below);
                }
                open.extend(
                    self.get::<Children>(below).into_iter().flatten(),
                );
            }
            if at == root {
                return None;
            }
            at = self.get::<ChildOf>(at)?.parent();
        }
    }
}
