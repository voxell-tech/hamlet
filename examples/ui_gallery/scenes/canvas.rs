//! The charts plugin's panel, drawn from the tree it returns: a
//! canvas of bars, a line and a few words, in the colours of the
//! theme.

use bevy::ecs::world::World;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::tree::canvas::{Canvas, Paint, Shape, Swatch};
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};
use hamlet_sdk::tree::view;

/// The id of the panel in the dock.
pub(super) fn id() -> String {
    dock_id("charts", "chart")
}

/// Adds the panel and the tree it shows.
pub(super) fn put(world: &mut World) {
    let panel =
        Panel::by_id(id(), "CHARTS", Glyph::Book, view::panel);
    world.spawn(panel);
    let mut trees = PluginTrees::default();
    trees.set(&id(), Drawn::Tree(tree()));
    world.insert_resource(trees);
}

const DAYS: [(&str, f32); 5] = [
    ("Mon", 40.0),
    ("Tue", 72.0),
    ("Wed", 55.0),
    ("Thu", 96.0),
    ("Fri", 64.0),
];

/// What the charts plugin returns: a bar for each day, a line over
/// their tops and a disc on each.
fn tree() -> Node {
    let paint = |swatch| Paint::Swatch(swatch);
    let mut shapes = vec![Shape::Line {
        from: (10.0, 130.0),
        to: (290.0, 130.0),
        width: 1.0,
        paint: paint(Swatch::Seam),
    }];
    let mut tops = Vec::new();
    for (at, (day, height)) in DAYS.into_iter().enumerate() {
        let x = 24.0 + at as f32 * 54.0;
        let top = 130.0 - height;
        shapes.push(Shape::Rect {
            x,
            y: top,
            w: 30.0,
            h: height,
            paint: paint(Swatch::Brass),
        });
        shapes.push(Shape::Text {
            x: x + 4.0,
            y: 134.0,
            text: day.into(),
            size: 11.0,
            paint: paint(Swatch::Dim),
        });
        tops.push((x + 15.0, top));
    }
    for pair in tops.windows(2) {
        shapes.push(Shape::Line {
            from: pair[0],
            to: pair[1],
            width: 2.0,
            paint: paint(Swatch::Lamp),
        });
    }
    for (x, y) in tops {
        shapes.push(Shape::Circle {
            x,
            y,
            r: 4.0,
            paint: paint(Swatch::Flame),
        });
    }
    Node::new(Kind::Column).of(vec![
        Node::new(Kind::Heading("TURNS THIS WEEK".into())),
        Node::new(Kind::Canvas(Canvas {
            width: 300.0,
            height: 156.0,
            shapes,
        })),
        Node::new(Kind::Note(
            "A canvas draws rectangles, lines, circles and words in \
             the colours of the theme."
                .into(),
        )),
    ])
}
