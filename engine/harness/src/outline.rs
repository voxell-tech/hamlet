//! A widget tree as text: a line for each widget, indented under the
//! one that holds it, with its kind, its id after `#` and what it
//! shows.
//!
//! ```text
//! column
//!   heading #title "Board"
//!   button #save "Save"
//! ```

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::{ChildOf, Children};
use bevy::ecs::world::World;
use bevy::ui::widget::Text;
use hamlet_engine_api::{Widget, WidgetTree};
use hamlet_engine_wasm::NodeOf;

/// The harness's methods on [`WidgetTree`].
pub trait WidgetTreeExt {
    /// The tree as an outline.
    fn outline(&self) -> String;
}

/// The harness's methods on [`World`].
pub trait HarnessWorldExt {
    /// The retained panel under `root` as an outline: a line for each
    /// node, its key after `#`, and a line for each text it shows.
    fn retained_outline(&self, root: Entity) -> String;

    /// The nearest entity with a `C` from `entity` up to `root`, if
    /// `entity` is under `root`.
    fn holder<C: Component>(
        &self,
        entity: Entity,
        root: Entity,
    ) -> Option<Entity>;
}

impl WidgetTreeExt for WidgetTree {
    fn outline(&self) -> String {
        let mut lines = Vec::new();
        // The children still to come of each widget open above this
        // one.
        let mut open = Vec::<u32>::new();
        for node in &self.nodes {
            let mut line = "  ".repeat(open.len());
            let (kind, shows) = match &node.widget {
                Widget::Text(text) => ("text", quoted(text)),
                Widget::Heading(text) => ("heading", quoted(text)),
                Widget::Icon(name) => ("icon", quoted(name)),
                Widget::Button(label) => ("button", quoted(label)),
                Widget::Toggle { label, on } => (
                    "toggle",
                    format!(
                        "{} {}",
                        quoted(label),
                        if *on { "on" } else { "off" }
                    ),
                ),
                Widget::Field { value, placeholder } => (
                    "field",
                    format!("{} ({})", quoted(value), placeholder),
                ),
                Widget::List => ("list", String::new()),
                Widget::Row => ("row", String::new()),
                Widget::Column => ("column", String::new()),
                Widget::Scroll => ("scroll", String::new()),
                Widget::Divider => ("divider", String::new()),
            };
            line.push_str(kind);
            if !node.id.0.is_empty() {
                line.push_str(&format!(" #{}", node.id.0));
            }
            if !shows.is_empty() {
                line.push(' ');
                line.push_str(&shows);
            }
            lines.push(line);
            if let Some(left) = open.last_mut() {
                *left -= 1;
            }
            if node.children > 0 {
                open.push(node.children);
            }
            while open.last() == Some(&0) {
                open.pop();
            }
        }
        lines.join("\n")
    }
}

impl HarnessWorldExt for World {
    fn retained_outline(&self, root: Entity) -> String {
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
                    "{}{}",
                    "  ".repeat(inner),
                    quoted(text)
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
}

fn quoted(text: &str) -> String {
    format!("{text:?}")
}
