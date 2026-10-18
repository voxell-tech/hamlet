//! A tree as a plugin returns it, checked and made into nodes.

use std::vec::IntoIter;

use hamlet_sdk::tree::glyph;
use hamlet_sdk::tree::node::{Kind, Node};

use crate::interface::exports::hamlet::plugin::plugin::{
    Node as Wire, Tree, View, Widget,
};
use crate::limits::Limits;

/// What a plugin returned for a panel.
#[derive(Debug, PartialEq)]
pub enum Shown {
    Tree(Node),
    /// A program in the panel language.
    Lang(String),
}

/// What `view` shows, or what is wrong with it.
pub fn shown(view: View, limits: &Limits) -> Result<Shown, String> {
    match view {
        View::Tree(tree) => node(tree, limits).map(Shown::Tree),
        View::Lang(text) => Ok(Shown::Lang(text)),
    }
}

/// The root of `tree`, or what is wrong with it.
pub fn node(tree: Tree, limits: &Limits) -> Result<Node, String> {
    if tree.nodes.len() > limits.tree_nodes {
        return Err(format!(
            "the tree has {} nodes, the most is {}",
            tree.nodes.len(),
            limits.tree_nodes
        ));
    }
    let mut nodes = tree.nodes.into_iter();
    let root = read(&mut nodes, 0, limits)?;
    match nodes.len() {
        0 => Ok(root),
        _ => Err("the tree has more than one root".into()),
    }
}

fn read(
    nodes: &mut IntoIter<Wire>,
    depth: usize,
    limits: &Limits,
) -> Result<Node, String> {
    let wire = nodes.next().ok_or("the tree ends too soon")?;
    if depth >= limits.tree_depth {
        return Err("the tree nests too deep".into());
    }
    let kind = kind(wire.widget);
    let needs_id = matches!(
        kind,
        Kind::Button(_) | Kind::Checkbox { .. } | Kind::TextField(_)
    );
    if needs_id && wire.id.is_none() {
        return Err(format!("{kind:?} has no id"));
    }
    let mut children = Vec::new();
    for _ in 0..wire.children {
        children.push(read(nodes, depth + 1, limits)?);
    }
    let node = Node {
        id: wire.id,
        kind,
        children,
    };
    let keys = node.keys();
    if keys.iter().enumerate().any(|(at, key)| {
        keys[..at].iter().any(|before| before.name == key.name)
    }) {
        return Err("two siblings have the same id".into());
    }
    Ok(node)
}

fn kind(widget: Widget) -> Kind {
    match widget {
        Widget::Column => Kind::Column,
        Widget::Row => Kind::Row,
        Widget::Scroll => Kind::Scroll,
        Widget::Label(text) => Kind::Label(text),
        Widget::Heading(text) => Kind::Heading(text),
        Widget::Note(text) => Kind::Note(text),
        Widget::Icon(name) => {
            Kind::Icon(glyph::named(&name).unwrap_or(glyph::UNKNOWN))
        }
        Widget::Button(label) => Kind::Button(label),
        Widget::Checkbox(box_) => Kind::Checkbox {
            label: box_.label,
            checked: box_.checked,
        },
        Widget::TextField(text) => Kind::TextField(text),
        Widget::Divider => Kind::Divider,
    }
}
