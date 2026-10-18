//! The roster plugin's panel, drawn from the tree it returns with a
//! list of agents made up for the scene.

use bevy::ecs::world::World;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_wasm::panel::dock_id;
use hamlet_wasm::tree::{Kind, Node};
use hamlet_wasm::trees::{Drawn, PluginTrees};
use hamlet_wasm::view;

/// The id of the panel in the dock.
pub(super) fn id() -> String {
    dock_id("roster", "roster")
}

/// Adds the panel and the tree it shows.
pub(super) fn put(world: &mut World) {
    let panel =
        Panel::by_id(id(), "ROSTER", Glyph::Robot, view::panel);
    world.spawn(panel);
    let mut trees = PluginTrees::default();
    trees.set(&id(), Drawn::Tree(tree()));
    world.insert_resource(trees);
}

/// What the roster returns after two refreshes, with this list.
fn tree() -> Node {
    let agents = [
        ("Pear Tree Hollow", "Juniper"),
        ("Pear Tree Hollow", "press-hand"),
        ("Pear Tree Hollow", "row-keeper"),
        ("The Quay", "harbourmaster"),
        ("The Quay", "tally-clerk"),
    ];
    let rows = agents
        .into_iter()
        .map(|(hamlet, name)| {
            Node::new(Kind::Row)
                .of(vec![
                    Node::new(Kind::Label(name.into())),
                    Node::new(Kind::Note(hamlet.into())),
                ])
                .id(&format!("{hamlet}/{name}"))
        })
        .collect();
    Node::new(Kind::Column).of(vec![
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Icon(Glyph::Robot)),
            Node::new(Kind::Heading("EVERYONE IN THE REALM".into())),
        ]),
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Button("Refresh".into())).id("refresh"),
        ]),
        Node::new(Kind::Label("Refreshed 2 times".into()))
            .id("count"),
        Node::new(Kind::Checkbox {
            label: "Show the hint".into(),
            checked: true,
        })
        .id("hint"),
        Node::new(Kind::Note(
            "Refresh reads the agents again.".into(),
        ))
        .id("help"),
        Node::new(Kind::Divider),
        Node::new(Kind::Scroll).of(rows).id("agents"),
    ])
}
