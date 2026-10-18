//! The Notes plugin's panel, drawn from the tree it returns with
//! notes made up for the scene: the open note to edit, or as
//! markdown.

use bevy::ecs::world::World;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};
use hamlet_sdk::tree::view;

/// What the open note says.
const BODY: &str = "Plan for the harbour\n\nMend the nets *today*, \
                    **not** later.\n\n- the quay\n- the \
                    nets\n\n```\nlet tide = 3;\n```";

/// The id of the panel in the dock.
pub(super) fn id() -> String {
    dock_id("notes", "main")
}

/// Adds the panel and the tree it shows, with the open note drawn as
/// markdown where `preview`.
pub(super) fn put(world: &mut World, preview: bool) {
    let panel =
        Panel::by_id(id(), "NOTES", Glyph::Notes, view::panel);
    world.spawn(panel);
    let mut trees = PluginTrees::default();
    trees.set(&id(), Drawn::Tree(tree(preview)));
    world.insert_resource(trees);
}

fn tree(preview: bool) -> Node {
    let words = match preview {
        true => Node::new(Kind::Markdown(BODY.into())),
        false => Node::new(Kind::TextArea(BODY.into())).id("body"),
    };
    Node::new(Kind::Column).of(vec![
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::TextField(String::new())).id("search"),
            Node::new(Kind::Button("New".into())).id("new"),
        ]),
        Node::new(Kind::Row).of(vec![
            Node::new(Kind::Button(
                "Add from the agent's notes".into(),
            ))
            .id("from-agent"),
        ]),
        Node::new(Kind::Divider),
        Node::new(Kind::Column).of(vec![
            Node::new(Kind::Row).of(vec![
                Node::new(Kind::Icon(Glyph::Notes)),
                Node::new(Kind::Heading(
                    "Plan for the harbour".into(),
                )),
            ]),
            Node::new(Kind::Row).of(vec![
                Node::new(Kind::Button("Shopping".into()))
                    .id("note.1"),
            ]),
            Node::new(Kind::Row).of(vec![
                Node::new(Kind::Button(
                    "Pears are grafted in March.".into(),
                ))
                .id("note.2"),
            ]),
        ]),
        Node::new(Kind::Divider),
        Node::new(Kind::Column).of(vec![
            Node::new(Kind::Row).of(vec![
                Node::new(Kind::Checkbox {
                    label: "Preview".into(),
                    checked: preview,
                })
                .id("preview"),
                Node::new(Kind::Button("Delete".into())).id("delete"),
            ]),
            words,
        ]),
    ])
}
