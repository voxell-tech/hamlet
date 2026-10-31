//! Drawing a gizmo: its lines as gizmos and its fills as triangles.

use bevy::color::Alpha;
use bevy::gizmos::gizmos::Gizmos;

use super::fills::Fills;
use super::{
    Arc, Frame, GizmoMode, Handle, HandleGizmos, Look, RING_STEPS,
    Style,
};

/// A gizmo as the preview shows it.
pub(super) struct Shown {
    pub(super) frame: Frame,
    /// The handle under the pointer, or dragged.
    pub(super) hot: Option<Handle>,
    pub(super) dragging: bool,
    /// The arc a dragged ring has turned through, and its radius.
    pub(super) sweep: Option<(Arc, f32)>,
}

/// Draws the handles `look` has: all of them with the hot one
/// brighter, or the dragged one and what a drag keeps beside it.
pub(super) fn paint(
    gizmos: &mut Gizmos<HandleGizmos>,
    fills: &mut Fills,
    look: Look,
    shown: &Shown,
    style: &Style,
) {
    let Shown {
        frame,
        hot,
        dragging,
        sweep,
    } = *shown;
    let mode = look.mode;
    let toward = -frame.look;
    let plain = |handle: Handle| match handle {
        Handle::Axis(axis) | Handle::Plane(axis) => style.axes[axis],
        Handle::Centre | Handle::View | Handle::Ball => {
            style.centre_color
        }
    };
    let tint = |handle: Handle, fade: f32| {
        let alpha = if hot == Some(handle) {
            style.hot_alpha
        } else {
            style.rest_alpha
        };
        let color = plain(handle);
        color.with_alpha(color.alpha() * alpha * fade)
    };
    let held = |handle: Handle| dragging && hot == Some(handle);
    let dropped = |handle: Handle| dragging && hot != Some(handle);

    if mode == GizmoMode::Rotate && look.only.is_none() {
        let rings = (0..3)
            .map(|axis| {
                let arc = if dragging {
                    Arc::full(frame.axes[axis])
                } else {
                    frame.arc(axis, style)
                };
                (Handle::Axis(axis), arc, style.size)
            })
            .chain([(
                Handle::View,
                Arc::full(toward),
                style.outer_ring,
            )]);
        for (handle, arc, pixels) in rings {
            if !dropped(handle) {
                gizmos.linestrip(
                    frame.ring(arc, pixels),
                    tint(handle, 1.0),
                );
            }
        }
        if let Some((arc, pixels)) = sweep {
            let rim = frame.ring(arc, pixels).collect::<Vec<_>>();
            let edge = hot.map_or(style.centre_color, plain);
            for end in [rim[0], rim[RING_STEPS]] {
                gizmos.line(frame.origin, end, edge);
            }
            fills.fan(frame.origin, rim, style.sweep);
        }
        if hot == Some(Handle::Ball) {
            let color = plain(Handle::Ball);
            fills.fan(
                frame.origin,
                frame.ring(Arc::full(toward), style.size),
                color.with_alpha(color.alpha() * style.ball_alpha),
            );
        }
        return;
    }

    for axis in 0..3 {
        if look.only.is_some_and(|only| only != axis) {
            continue;
        }
        let handle = Handle::Axis(axis);
        let fade = frame.axis_fade(axis, style);
        // A move keeps the other axes to move against.
        let bare = dropped(handle);
        if fade <= 0.0 || (bare && mode == GizmoMode::Scale) {
            continue;
        }
        let color = tint(handle, fade);
        let from = if held(handle) || look.only.is_some() {
            frame.origin
        } else {
            frame.along(axis, style.centre)
        };
        if bare {
            gizmos.line(from, frame.along(axis, style.size), color);
        } else if mode == GizmoMode::Translate {
            let neck =
                frame.along(axis, style.size - style.cone_length);
            gizmos.line(from, neck, color);
            fills.cone(
                neck,
                frame.along(axis, style.size),
                style.cone_radius * frame.scale,
                color,
            );
        } else {
            let half = style.tip / 2.0;
            gizmos.line(
                from,
                frame.along(axis, style.size - style.tip),
                color,
            );
            fills.cuboid(
                frame.along(axis, style.size - half),
                frame.axes.map(|axis| axis * half * frame.scale),
                color,
            );
        }
    }
    if look.only.is_some() {
        return;
    }
    for axis in 0..3 {
        let handle = Handle::Plane(axis);
        let fade = frame.plane_fade(axis, style);
        if fade <= 0.0 || dropped(handle) {
            continue;
        }
        let corners = frame.plane(axis, style);
        let color = tint(handle, fade);
        gizmos.linestrip(
            corners.into_iter().chain([corners[0]]),
            color,
        );
        fills.fan(
            corners[0],
            [corners[1], corners[2], corners[3]],
            color.with_alpha(color.alpha() * style.plane_fill),
        );
    }
    if !dropped(Handle::Centre) {
        let color = tint(Handle::Centre, 1.0);
        let mut ring = |pixels: f32| {
            gizmos.linestrip(
                frame.ring(Arc::full(toward), pixels),
                color,
            );
        };
        ring(style.centre);
        if mode == GizmoMode::Scale {
            ring(style.outer_ring);
        }
    }
}
