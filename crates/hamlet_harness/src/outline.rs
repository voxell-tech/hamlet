//! A panel's tree as text, one line for each node.

use hamlet_sdk::tree::node::{Kind, Node};

/// The lines of `root`, each node indented by its depth.
pub fn lines(root: &Node) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out
}

fn walk(node: &Node, depth: usize, out: &mut Vec<String>) {
    out.push(format!("{}{}", "  ".repeat(depth), line(node)));
    for child in &node.children {
        walk(child, depth + 1, out);
    }
}

fn line(node: &Node) -> String {
    let id = node
        .id
        .as_deref()
        .map(|id| format!(" #{id}"))
        .unwrap_or_default();
    match &node.kind {
        Kind::Column => format!("column{id}"),
        Kind::Row => format!("row{id}"),
        Kind::Scroll => format!("scroll{id}"),
        Kind::Label(text) => format!("label {text:?}{id}"),
        Kind::Heading(text) => format!("heading {text:?}{id}"),
        Kind::Note(text) => format!("note {text:?}{id}"),
        Kind::Icon(glyph) => format!("icon {glyph:?}{id}"),
        Kind::Button(text) => format!("button {text:?}{id}"),
        Kind::Checkbox { label, checked } => {
            let mark = if *checked { "x" } else { " " };
            format!("checkbox [{mark}] {label:?}{id}")
        }
        Kind::TextField(text) => format!("text-field {text:?}{id}"),
        Kind::Divider => format!("divider{id}"),
    }
}

/// The node with this id, wherever it is in the tree.
pub fn find<'a>(root: &'a Node, id: &str) -> Option<&'a Node> {
    if root.id.as_deref() == Some(id) {
        return Some(root);
    }
    root.children.iter().find_map(|child| find(child, id))
}

/// Whether the words of some node of the tree contain `text`.
pub fn shows(root: &Node, text: &str) -> bool {
    root.text().contains(text)
        || root.children.iter().any(|child| shows(child, text))
}

/// The ids in the tree, in order.
pub fn ids(root: &Node) -> Vec<&str> {
    let mut found = Vec::new();
    collect(root, &mut found);
    found
}

fn collect<'a>(node: &'a Node, found: &mut Vec<&'a str>) {
    found.extend(node.id.as_deref());
    for child in &node.children {
        collect(child, found);
    }
}
