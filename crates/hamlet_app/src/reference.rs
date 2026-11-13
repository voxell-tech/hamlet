//! A hamlet or an agent copied as its address and name, in words and
//! in rich text that tells Hamlet what it names.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_client::realm::{self as agents, HamletId};
use hamlet_desktop::clipboard::CopyRich;

/// The attribute rich text carries the words of a reference in.
const ATTRIBUTE: &str = "data-hamlet-reference";

/// Copies `hamlet` as `hamlet <id> (<name>)`.
pub(crate) fn copy_hamlet(world: &mut World, hamlet: Entity) {
    let Some(id) =
        world.get::<HamletId>(hamlet).map(|id| id.0.clone())
    else {
        return;
    };
    let name = agents::shown_of(world, hamlet);
    copy(world, format!("hamlet {id} ({name})"));
}

/// Copies `agent` as its address and name, as in `hire#3 (Mira)`.
pub(crate) fn copy_agent(world: &mut World, agent: Entity) {
    // An agent's name, which commands know it by, is its address.
    let address = agents::name_of(world, agent);
    if address.is_empty() {
        return;
    }
    let name = agents::shown_of(world, agent);
    let text = match name == address {
        true => address,
        false => format!("{address} ({name})"),
    };
    copy(world, text);
}

fn copy(world: &mut World, text: String) {
    let escaped = escape(&text);
    let html =
        format!("<span {ATTRIBUTE}=\"{escaped}\">{escaped}</span>");
    world.trigger(CopyRich { text, html });
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
