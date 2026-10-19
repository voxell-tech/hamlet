//! A tree as a plugin returns it, checked and made into nodes.

use std::vec::IntoIter;

use hamlet_sdk::tree::canvas::{Canvas, Paint, Shape, Swatch};
use hamlet_sdk::tree::glyph;
use hamlet_sdk::tree::node::{Kind, Node};

use crate::interface::exports::hamlet::plugin::plugin::{
    Canvas as WireCanvas, Node as Wire, Paint as WirePaint,
    Shape as WireShape, Tree, View, Widget,
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
        View::Lang(text) if text.len() > limits.tree_text_bytes => {
            Err(format!(
                "the program has {} bytes, the most is {}",
                text.len(),
                limits.tree_text_bytes
            ))
        }
        View::Lang(text) => Ok(Shown::Lang(text)),
    }
}

/// Counts `text` against the limits of a tree.
fn spend(
    text: &str,
    limits: &Limits,
    spent: &mut usize,
) -> Result<(), String> {
    if text.len() > limits.text_bytes {
        return Err(format!(
            "a text has {} bytes, the most is {}",
            text.len(),
            limits.text_bytes
        ));
    }
    *spent += text.len();
    if *spent > limits.tree_text_bytes {
        return Err(format!(
            "the tree has more than {} bytes of text",
            limits.tree_text_bytes
        ));
    }
    Ok(())
}

/// The text a widget holds.
fn text_of(widget: &Widget) -> &str {
    match widget {
        Widget::Label(text)
        | Widget::Heading(text)
        | Widget::Note(text)
        | Widget::Icon(text)
        | Widget::Button(text)
        | Widget::TextField(text)
        | Widget::TextArea(text)
        | Widget::Markdown(text) => text,
        Widget::Checkbox(checkbox) => &checkbox.label,
        Widget::Column
        | Widget::Row
        | Widget::Scroll
        | Widget::Divider
        | Widget::Canvas(_) => "",
    }
}

/// The canvas a plugin drew, or what is wrong with it. The text of
/// its shapes is spent from the tree's, and its shapes are counted.
fn canvas(
    wire: WireCanvas,
    limits: &Limits,
    spent: &mut usize,
    shapes: &mut usize,
) -> Result<Canvas, String> {
    let side =
        |side: u32| match (1..=limits.canvas_px).contains(&side) {
            true => Ok(side as f32),
            false => Err(format!(
                "a canvas side is {side} pixels, from 1 to {}",
                limits.canvas_px
            )),
        };
    *shapes += wire.shapes.len();
    if *shapes > limits.canvas_shapes {
        return Err(format!(
            "the tree has more than {} shapes",
            limits.canvas_shapes
        ));
    }
    let mut drawn = Vec::new();
    for shape in wire.shapes {
        drawn.push(match shape {
            WireShape::Rect(s) => Shape::Rect {
                x: at(s.x)?,
                y: at(s.y)?,
                w: at(s.w)?,
                h: at(s.h)?,
                paint: paint(s.paint)?,
            },
            WireShape::Line(s) => Shape::Line {
                from: (at(s.x1)?, at(s.y1)?),
                to: (at(s.x2)?, at(s.y2)?),
                width: at(s.width)?.clamp(MIN_LINE, MAX_LINE),
                paint: paint(s.paint)?,
            },
            WireShape::Circle(s) => Shape::Circle {
                x: at(s.x)?,
                y: at(s.y)?,
                r: at(s.r)?.max(0.0),
                paint: paint(s.paint)?,
            },
            WireShape::Text(s) => {
                spend(&s.text, limits, spent)?;
                Shape::Text {
                    x: at(s.x)?,
                    y: at(s.y)?,
                    size: at(s.size)?.clamp(MIN_TEXT, MAX_TEXT),
                    text: s.text,
                    paint: paint(s.paint)?,
                }
            }
        });
    }
    Ok(Canvas {
        width: side(wire.width)?,
        height: side(wire.height)?,
        shapes: drawn,
    })
}

/// A position or a size, if it is a number at all.
fn at(value: f32) -> Result<f32, String> {
    match value.is_finite() {
        true => Ok(value.clamp(-FAR, FAR)),
        false => {
            Err("a shape has a position that is not a number".into())
        }
    }
}

fn paint(wire: WirePaint) -> Result<Paint, String> {
    match wire {
        WirePaint::Palette(name) => Swatch::named(&name)
            .map(Paint::Swatch)
            .ok_or_else(|| format!("no colour named `{name}`")),
        WirePaint::Rgba(rgba) => Ok(Paint::Rgba(rgba.to_be_bytes())),
    }
}

/// How far from the canvas a position may be, in pixels.
const FAR: f32 = 100_000.0;

/// The thinnest and the thickest line, in pixels.
const MIN_LINE: f32 = 0.5;
const MAX_LINE: f32 = 64.0;

/// The smallest and the largest lettering, in pixels.
const MIN_TEXT: f32 = 6.0;
const MAX_TEXT: f32 = 128.0;

/// Replaces each canvas of `node` with a note that it waits for
/// `ui.canvas`. Whether any was.
pub fn without_canvases(node: &mut Node) -> bool {
    let mut held = false;
    if matches!(node.kind, Kind::Canvas(_)) {
        node.kind =
            Kind::Note("(a drawing that waits for ui.canvas)".into());
        held = true;
    }
    for child in &mut node.children {
        held |= without_canvases(child);
    }
    held
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
    let root = read(&mut nodes, 0, limits, &mut Spent::default())?;
    match nodes.len() {
        0 => Ok(root),
        _ => Err("the tree has more than one root".into()),
    }
}

/// What a tree has used of its limits so far.
#[derive(Default)]
struct Spent {
    /// Bytes of text.
    text: usize,
    /// Shapes of canvases.
    shapes: usize,
}

fn read(
    nodes: &mut IntoIter<Wire>,
    depth: usize,
    limits: &Limits,
    spent: &mut Spent,
) -> Result<Node, String> {
    let wire = nodes.next().ok_or("the tree ends too soon")?;
    if depth >= limits.tree_depth {
        return Err("the tree nests too deep".into());
    }
    spend(text_of(&wire.widget), limits, &mut spent.text)?;
    spend(wire.id.as_deref().unwrap_or(""), limits, &mut spent.text)?;
    let kind = kind(wire.widget, limits, spent)?;
    let needs_id = matches!(
        kind,
        Kind::Button(_)
            | Kind::Checkbox { .. }
            | Kind::TextField(_)
            | Kind::TextArea(_)
    );
    if needs_id && wire.id.is_none() {
        return Err(format!("{kind:?} has no id"));
    }
    let mut children = Vec::new();
    for _ in 0..wire.children {
        children.push(read(nodes, depth + 1, limits, spent)?);
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

fn kind(
    widget: Widget,
    limits: &Limits,
    spent: &mut Spent,
) -> Result<Kind, String> {
    Ok(match widget {
        Widget::Canvas(drawn) => {
            let Spent { text, shapes } = spent;
            Kind::Canvas(canvas(drawn, limits, text, shapes)?)
        }
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
        Widget::TextArea(text) => Kind::TextArea(text),
        Widget::Markdown(text) => Kind::Markdown(text),
    })
}
