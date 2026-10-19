//! A panel's tree as text, one line for each node.

use hamlet_sdk::tree::canvas::Shape;
use hamlet_sdk::tree::node::{Kind, Node};

/// The lines of `root`, each node indented by its depth.
pub fn lines(root: &Node) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, 0, &mut out);
    out
}

fn walk(node: &Node, depth: usize, out: &mut Vec<String>) {
    out.push(format!("{}{}", "  ".repeat(depth), line(node)));
    if let Kind::Canvas(canvas) = &node.kind {
        for shape in &canvas.shapes {
            out.push(format!(
                "{}{}",
                "  ".repeat(depth + 1),
                shape_line(shape)
            ));
        }
    }
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
        Kind::TextArea(text) => format!("text-area {text:?}{id}"),
        Kind::Markdown(text) => format!("markdown {text:?}{id}"),
        Kind::Canvas(canvas) => {
            format!("canvas {}x{}{id}", canvas.width, canvas.height)
        }
    }
}

fn shape_line(shape: &Shape) -> String {
    match shape {
        Shape::Rect { x, y, w, h, .. } => {
            format!("rect {w}x{h} at {x},{y}")
        }
        Shape::Line { from, to, .. } => {
            format!("line {},{} to {},{}", from.0, from.1, to.0, to.1)
        }
        Shape::Circle { x, y, r, .. } => {
            format!("circle r{r} at {x},{y}")
        }
        Shape::Text { x, y, text, .. } => {
            format!("text {text:?} at {x},{y}")
        }
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
    let drawn = match &root.kind {
        Kind::Canvas(canvas) => canvas.shapes.iter().any(|shape| {
            matches!(shape, Shape::Text { text: said, .. } if said.contains(text))
        }),
        _ => false,
    };
    drawn
        || root.text().contains(text)
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
