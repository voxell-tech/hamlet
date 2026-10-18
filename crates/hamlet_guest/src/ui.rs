//! Building the tree of a panel: `column([label("Hi"),
//! button("Go").id("go")])`.

use crate::bindings::exports::hamlet::plugin::plugin::{
    Checkbox, Node as Wire, Tree, Widget,
};

/// One node of a panel, with the nodes in it.
pub struct Node {
    id: Option<String>,
    widget: Widget,
    children: Vec<Node>,
}

impl Node {
    fn new(widget: Widget, children: Vec<Node>) -> Self {
        Self {
            id: None,
            widget,
            children,
        }
    }

    /// This, with the id events and redraws know it by. A button, a
    /// checkbox and a text field need one.
    pub fn id(mut self, id: &str) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The tree the host is given.
    pub fn into_tree(self) -> Tree {
        let mut nodes = Vec::new();
        self.flatten(&mut nodes);
        Tree { nodes }
    }

    fn flatten(self, nodes: &mut Vec<Wire>) {
        nodes.push(Wire {
            id: self.id,
            widget: self.widget,
            children: self.children.len() as u32,
        });
        for child in self.children {
            child.flatten(nodes);
        }
    }
}

/// Nodes top to bottom.
pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    Node::new(Widget::Column, children.into_iter().collect())
}

/// Nodes left to right.
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    Node::new(Widget::Row, children.into_iter().collect())
}

/// Nodes top to bottom that scroll when they do not fit.
pub fn scroll(children: impl IntoIterator<Item = Node>) -> Node {
    Node::new(Widget::Scroll, children.into_iter().collect())
}

pub fn label(text: &str) -> Node {
    Node::new(Widget::Label(text.into()), Vec::new())
}

/// The name over a part of a panel.
pub fn heading(text: &str) -> Node {
    Node::new(Widget::Heading(text.into()), Vec::new())
}

/// Small dim words.
pub fn note(text: &str) -> Node {
    Node::new(Widget::Note(text.into()), Vec::new())
}

/// An icon, by the name of one of the panels' icons.
pub fn icon(name: &str) -> Node {
    Node::new(Widget::Icon(name.into()), Vec::new())
}

pub fn button(label: &str) -> Node {
    Node::new(Widget::Button(label.into()), Vec::new())
}

pub fn checkbox(label: &str, checked: bool) -> Node {
    Node::new(
        Widget::Checkbox(Checkbox {
            label: label.into(),
            checked,
        }),
        Vec::new(),
    )
}

pub fn text_field(text: &str) -> Node {
    Node::new(Widget::TextField(text.into()), Vec::new())
}

pub fn divider() -> Node {
    Node::new(Widget::Divider, Vec::new())
}
