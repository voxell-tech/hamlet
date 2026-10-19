//! What a plugin draws over the world: its marks, checked against its
//! grant and the caps, kept in the realm's read model for the realm's
//! view to draw, and taken away when it may no longer draw.

use bevy::prelude::*;
use hamlet_realm_api::overlay::{
    Anchor, Form, Mark, Overlay, Overlays, Paint,
};
use hamlet_sdk::capability::{Capability, Granted, Verdict, Wanted};
use hamlet_sdk::plugin::{PluginInfo, Status};
use hamlet_sdk::tree::canvas::Swatch;

use crate::interface::exports::hamlet::plugin::plugin::{
    Anchor as WireAnchor, Intent, Mark as WireMark, MarkShape,
    Paint as WirePaint,
};
use crate::limits::Limits;
use crate::rate::Rate;
use crate::run::Done;

/// How far from its anchor a mark may be, in pixels.
const FAR: f32 = 100_000.0;

/// The thinnest and the thickest line, in pixels.
const MIN_LINE: f32 = 0.5;
const MAX_LINE: f32 = 64.0;

/// The smallest and the largest lettering, in pixels.
const MIN_TEXT: f32 = 6.0;
const MAX_TEXT: f32 = 128.0;

/// The overlays each plugin put up lately.
#[derive(Resource, Default)]
pub struct Redrawn {
    done: Rate,
    /// A warning is logged once a minute for each plugin.
    refused: Rate,
}

/// Keeps what the plugins that were stepped draw over the world. Only
/// the last overlay of a step is drawn.
pub fn take(
    mut done: MessageReader<Done>,
    plugins: Query<(&PluginInfo, &Granted)>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
    mut redrawn: ResMut<Redrawn>,
    mut overlays: ResMut<Overlays>,
    mut commands: Commands,
) {
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    for Done {
        plugin, intents, ..
    } in done.read()
    {
        let Some(wire) =
            intents.iter().rev().find_map(|intent| match intent {
                Intent::Overlay(marks) => Some(marks),
                _ => None,
            })
        else {
            continue;
        };
        let Ok((info, granted)) = plugins.get(*plugin) else {
            continue;
        };
        let checked = match granted.verdict(Capability::WorldOverlay)
        {
            Some(Verdict::Allowed) => marked(wire, &limits),
            Some(Verdict::Ask) => {
                commands.trigger(Wanted {
                    plugin: *plugin,
                    capability: Capability::WorldOverlay,
                    target: None,
                    reason: String::new(),
                });
                Err("world.overlay is yet to be decided".into())
            }
            _ => Err("world.overlay not granted".into()),
        }
        .and_then(|marks| {
            let most = limits.overlay_updates_per_second;
            match redrawn.done.within(*plugin, now, 1.0, most) {
                true => Ok(marks),
                false => Err(format!(
                    "over {most} overlays a second, some dropped"
                )),
            }
        });
        match checked {
            Ok(marks) => put(&mut overlays, info, marks),
            Err(why) => {
                if redrawn.refused.admit(*plugin, now, 1) {
                    warn!("plugin {}: overlay: {why}", info.id);
                }
            }
        }
    }
}

/// Takes away the overlay of a plugin that is not running or may no
/// longer draw over the world.
pub fn prune(
    plugins: Query<(&PluginInfo, &Status, &Granted)>,
    mut overlays: ResMut<Overlays>,
) {
    let keeps = |overlay: &Overlay| {
        plugins.iter().any(|(info, status, granted)| {
            info.id == overlay.plugin
                && *status == Status::Running
                && granted.holds(Capability::WorldOverlay)
        })
    };
    if !overlays.0.iter().all(keeps) {
        overlays.0.retain(keeps);
    }
}

/// Whether any plugin draws over the world.
pub fn any(overlays: Res<Overlays>) -> bool {
    !overlays.0.is_empty()
}

/// Puts `marks` in the place of what the plugin `info` drew, or takes
/// that away where there are none.
fn put(overlays: &mut Overlays, info: &PluginInfo, marks: Vec<Mark>) {
    let at = overlays.0.iter().position(|o| o.plugin == info.id);
    match (at, marks.is_empty()) {
        (Some(at), true) => {
            overlays.0.remove(at);
        }
        (None, true) => {}
        (Some(at), false) => {
            if overlays.0[at].marks != marks {
                overlays.0[at].marks = marks;
            }
        }
        (None, false) => overlays.0.push(Overlay {
            plugin: info.id.clone(),
            by: info.name.clone(),
            marks,
        }),
    }
}

/// The marks a plugin gave, or what is wrong with them.
fn marked(
    wire: &[WireMark],
    limits: &Limits,
) -> Result<Vec<Mark>, String> {
    if wire.len() > limits.overlay_marks {
        return Err(format!(
            "{} marks, the most is {}",
            wire.len(),
            limits.overlay_marks
        ));
    }
    let mut spent = 0;
    let mut marks = Vec::new();
    for mark in wire {
        let form = match &mark.shape {
            MarkShape::Rect(size) => Form::Rect {
                w: at(size.w)?.max(0.0),
                h: at(size.h)?.max(0.0),
            },
            MarkShape::Circle(r) => Form::Circle {
                r: at(*r)?.max(0.0),
            },
            MarkShape::Line(line) => Form::Line {
                to: line
                    .to
                    .as_ref()
                    .map(|to| anchor(to, &mut spent))
                    .transpose()?,
                end: Vec2::new(at(line.x)?, at(line.y)?),
                width: at(line.width)?.clamp(MIN_LINE, MAX_LINE),
            },
            MarkShape::Text(text) => {
                spent += text.text.len();
                Form::Text {
                    text: text.text.clone(),
                    size: at(text.size)?.clamp(MIN_TEXT, MAX_TEXT),
                }
            }
        };
        marks.push(Mark {
            anchor: anchor(&mark.anchor, &mut spent)?,
            offset: Vec2::new(at(mark.x)?, at(mark.y)?),
            form,
            paint: paint(&mark.paint)?,
        });
    }
    match spent <= limits.overlay_text_bytes {
        true => Ok(marks),
        false => Err(format!(
            "{spent} bytes of text and names, the most is {}",
            limits.overlay_text_bytes
        )),
    }
}

/// An anchor, whose name is spent from the text of the overlay.
fn anchor(
    wire: &WireAnchor,
    spent: &mut usize,
) -> Result<Anchor, String> {
    Ok(match wire {
        WireAnchor::Agent(name) => {
            *spent += name.len();
            Anchor::Agent(name.clone())
        }
        WireAnchor::Hamlet(id) => {
            *spent += id.len();
            Anchor::Hamlet(id.clone())
        }
        WireAnchor::Point(point) => Anchor::Point(Vec3::new(
            at(point.x)?,
            at(point.y)?,
            at(point.z)?,
        )),
    })
}

/// A position or a size, if it is a number at all.
fn at(value: f32) -> Result<f32, String> {
    match value.is_finite() {
        true => Ok(value.clamp(-FAR, FAR)),
        false => {
            Err("a mark has a position that is not a number".into())
        }
    }
}

fn paint(wire: &WirePaint) -> Result<Paint, String> {
    match wire {
        WirePaint::Palette(name) => match Swatch::named(name) {
            Some(_) => Ok(Paint::Theme(name.clone())),
            None => Err(format!("no colour named `{name}`")),
        },
        WirePaint::Rgba(rgba) => Ok(Paint::Rgba(rgba.to_be_bytes())),
    }
}
