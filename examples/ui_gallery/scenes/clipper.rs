//! The Clipper plugin's panel, drawn from the tree it returns after
//! it sent the selected agent's name to Notes: the reply, the titles
//! it read from Notes and the count of notes saved.

use bevy::ecs::world::World;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};
use hamlet_sdk::tree::view;

/// The id of the panel in the dock.
pub(super) fn id() -> String {
    dock_id("clipper", "main")
}

/// Adds the panel and the tree it shows.
pub(super) fn put(world: &mut World) {
    let panel =
        Panel::by_id(id(), "CLIPPER", Glyph::Notes, view::panel);
    world.spawn(panel);
    let mut trees = PluginTrees::default();
    trees.set(&id(), Drawn::Tree(tree()));
    world.insert_resource(trees);
}

fn tree() -> Node {
    let titles = [
        ("note.0", "Plan for the harbour"),
        ("note.1", "Shopping"),
        ("note.2", "Juniper"),
    ]
    .into_iter()
    .map(|(id, title)| Node::new(Kind::Label(title.into())).id(id))
    .collect();
    Node::new(Kind::Column).of(vec![
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Icon(Glyph::Notes)),
            Node::new(Kind::Heading("CLIP TO NOTES".into())),
        ]),
        Node::new(Kind::Note("Selected: Juniper".into()))
            .id("selected"),
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Button(
                "Send the selected agent's name to Notes".into(),
            ))
            .id("send"),
        ]),
        Node::new(Kind::Note("Notes saved it as note 2.".into()))
            .id("reply"),
        Node::new(Kind::Divider),
        Node::new(Kind::Heading("IN NOTES".into())),
        Node::new(Kind::Column).of(titles),
        Node::new(Kind::Divider),
        Node::new(Kind::Label("Notes saved: 3".into())).id("saved"),
    ])
}
