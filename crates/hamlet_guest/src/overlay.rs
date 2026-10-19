//! Marks over the world: rectangles, discs, lines and words that the
//! host draws each frame where their anchors are seen, a few pixels
//! from them. It needs the capability `world.overlay`, allowed when
//! the chief lets the plugin in. Paint them with
//! [`draw::theme`](crate::draw::theme) or
//! [`draw::rgba`](crate::draw::rgba).

pub use crate::bindings::exports::hamlet::plugin::plugin::{
    Anchor, Mark, MarkShape,
};
use crate::bindings::exports::hamlet::plugin::plugin::{
    Intent, MarkLine, MarkSize, MarkText, Paint, Point,
};

/// The foot of the robot of the agent `name`.
pub fn agent(name: &str) -> Anchor {
    Anchor::Agent(name.into())
}

/// The common of the hamlet of the id `id`.
pub fn hamlet(id: &str) -> Anchor {
    Anchor::Hamlet(id.into())
}

/// A point of the world, in metres, `y` up.
pub fn point(x: f32, y: f32, z: f32) -> Anchor {
    Anchor::Point(Point { x, y, z })
}

/// A rectangle with its corner `x`, `y` pixels from where `anchor` is
/// seen.
pub fn rect(
    anchor: Anchor,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    paint: Paint,
) -> Mark {
    Mark {
        anchor,
        x,
        y,
        shape: MarkShape::Rect(MarkSize { w, h }),
        paint,
    }
}

/// A disc of radius `r` about `x`, `y` pixels from where `anchor` is
/// seen.
pub fn circle(
    anchor: Anchor,
    x: f32,
    y: f32,
    r: f32,
    paint: Paint,
) -> Mark {
    Mark {
        anchor,
        x,
        y,
        shape: MarkShape::Circle(r),
        paint,
    }
}

/// A line `width` pixels thick from `from` pixels off `anchor` to
/// `end` pixels off `to`, or off `anchor` where `to` is none.
pub fn line(
    anchor: Anchor,
    from: (f32, f32),
    to: Option<Anchor>,
    end: (f32, f32),
    width: f32,
    paint: Paint,
) -> Mark {
    Mark {
        anchor,
        x: from.0,
        y: from.1,
        shape: MarkShape::Line(MarkLine {
            to,
            x: end.0,
            y: end.1,
            width,
        }),
        paint,
    }
}

/// Words `size` pixels high with their top left `x`, `y` pixels from
/// where `anchor` is seen.
pub fn text(
    anchor: Anchor,
    x: f32,
    y: f32,
    words: &str,
    size: f32,
    paint: Paint,
) -> Mark {
    Mark {
        anchor,
        x,
        y,
        shape: MarkShape::Text(MarkText {
            text: words.into(),
            size,
        }),
        paint,
    }
}

/// The intent that replaces what the plugin draws over the world with
/// `marks`, or takes it away when there are none. To be returned from
/// `step`.
pub fn draw(marks: Vec<Mark>) -> Intent {
    Intent::Overlay(marks)
}
