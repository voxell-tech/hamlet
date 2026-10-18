//! The components an evaluation gave, made into the nodes of a panel
//! tree.

use std::collections::HashMap;

use hamlet_sdk::tree::glyph;
use hamlet_sdk::tree::node::{Kind, Node};
use openui_lang::{Element, Prop, Value};

use crate::limits::PanelLimits;
use crate::wire::Wire;

/// A tree, and what each node of it that can be touched does.
pub(crate) struct Converted {
    pub root: Node,
    pub wires: HashMap<String, Wire>,
    pub problems: Vec<String>,
}

/// The tree of `root`. What goes over the limits is dropped.
pub(crate) fn tree(
    root: &Element,
    limits: &PanelLimits,
) -> Converted {
    let mut walk = Walk {
        limits,
        nodes: 0,
        wires: HashMap::new(),
        problems: Vec::new(),
    };
    let root = walk
        .element(root, "n", 0)
        .unwrap_or_else(|| Node::new(Kind::Column));
    Converted {
        root,
        wires: walk.wires,
        problems: walk.problems,
    }
}

struct Walk<'a> {
    limits: &'a PanelLimits,
    nodes: usize,
    wires: HashMap<String, Wire>,
    problems: Vec<String>,
}

impl Walk<'_> {
    /// The node of `element`, whose id is `path` if it can be
    /// touched.
    fn element(
        &mut self,
        element: &Element,
        path: &str,
        depth: usize,
    ) -> Option<Node> {
        if depth >= self.limits.tree_depth {
            self.raise(format!(
                "the panel nests deeper than {} levels",
                self.limits.tree_depth
            ));
            return None;
        }
        if self.nodes >= self.limits.tree_nodes {
            self.raise(format!(
                "the panel has more than {} nodes",
                self.limits.tree_nodes
            ));
            return None;
        }
        self.nodes += 1;
        let text = |name: &str| {
            element
                .prop(name)
                .map(Value::to_js_string)
                .unwrap_or_default()
        };
        let node = match element.component() {
            "Column" => {
                self.parent(Kind::Column, element, path, depth)
            }
            "Row" => self.parent(Kind::Row, element, path, depth),
            "Scroll" => {
                self.parent(Kind::Scroll, element, path, depth)
            }
            "Label" => Node::new(Kind::Label(text("text"))),
            "Heading" => Node::new(Kind::Heading(text("text"))),
            "Note" => Node::new(Kind::Note(text("text"))),
            "Icon" => Node::new(Kind::Icon(
                glyph::named(&text("name")).unwrap_or(glyph::UNKNOWN),
            )),
            "Button" => {
                self.wire(path, element, None);
                Node::new(Kind::Button(text("label"))).id(path)
            }
            "Checkbox" => {
                self.wire(path, element, Some("checked"));
                Node::new(Kind::Checkbox {
                    label: text("label"),
                    checked: element
                        .prop("checked")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
                .id(path)
            }
            "TextField" => {
                self.wire(path, element, Some("text"));
                Node::new(Kind::TextField(text("text"))).id(path)
            }
            "Divider" => Node::new(Kind::Divider),
            other => {
                self.raise(format!("no component `{other}` to draw"));
                return None;
            }
        };
        Some(node)
    }

    fn parent(
        &mut self,
        kind: Kind,
        element: &Element,
        path: &str,
        depth: usize,
    ) -> Node {
        let mut children = Vec::new();
        if let Some(Value::Array(items)) = element.prop("children") {
            self.gather(items, path, depth, &mut 0, &mut children);
        }
        Node::new(kind).of(children)
    }

    /// The nodes of the components among `values`, lists inside lists
    /// included, numbered from `at`.
    fn gather(
        &mut self,
        values: &[Value],
        path: &str,
        depth: usize,
        at: &mut usize,
        out: &mut Vec<Node>,
    ) {
        for value in values {
            match value {
                Value::Element(child) => {
                    let id = format!("{path}.{at}");
                    *at += 1;
                    out.extend(self.element(child, &id, depth + 1));
                }
                Value::Array(items) => {
                    self.gather(items, path, depth, at, out)
                }
                _ => {}
            }
        }
    }

    /// Keeps what the node at `path` writes to and runs.
    fn wire(
        &mut self,
        path: &str,
        element: &Element,
        bound: Option<&str>,
    ) {
        let binding = bound
            .and_then(|name| {
                element
                    .props()
                    .iter()
                    .find(|prop| prop.name() == name)
            })
            .and_then(Prop::binding)
            .map(str::to_string);
        let action = element
            .prop("action")
            .and_then(Value::as_action)
            .cloned();
        if binding.is_some() || action.is_some() {
            self.wires.insert(path.into(), Wire { binding, action });
        }
    }

    fn raise(&mut self, problem: String) {
        if !self.problems.contains(&problem) {
            self.problems.push(problem);
        }
    }
}
