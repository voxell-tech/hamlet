//! What a plugin draws over the world, as the realm's view draws it:
//! Surveyor's marks over the agents at work, each with a line to its
//! hamlet, and the tag that names the plugin. The anchors are seen at
//! points made up for the picture of the world.

use bevy::prelude::*;
use hamlet_realm_api::overlay::{
    Anchor, Form, Mark, Overlay, Overlays, Paint,
};
use hamlet_sdk::theme::Themed;
use plugin_realm::view::overlay::{self, Pinned, Tag};

/// The agents at work and where each is seen in the picture.
const AGENTS: [(&str, Vec2); 2] = [
    ("Juniper", Vec2::new(706.0, 600.0)),
    ("Hawk", Vec2::new(880.0, 560.0)),
];

/// Where the common of the hamlet is seen.
const COMMON: Vec2 = Vec2::new(560.0, 740.0);

/// Where the corner of the world's panel is.
const CORNER: Vec2 = Vec2::new(352.0, 80.0);

/// The marks Surveyor sends for the agents at work.
fn overlays() -> Overlays {
    let theme = |name: &str| Paint::Theme(name.into());
    let marks = AGENTS
        .iter()
        .flat_map(|(name, _)| {
            let anchor = Anchor::Agent(name.to_string());
            [
                Mark {
                    anchor: anchor.clone(),
                    offset: Vec2::new(0.0, -30.0),
                    form: Form::Line {
                        to: Some(Anchor::Hamlet("home".into())),
                        end: Vec2::ZERO,
                        width: 1.5,
                    },
                    paint: theme("seam"),
                },
                Mark {
                    anchor: anchor.clone(),
                    offset: Vec2::new(0.0, -40.0),
                    form: Form::Circle { r: 5.0 },
                    paint: theme("lamp"),
                },
                Mark {
                    anchor,
                    offset: Vec2::new(9.0, -48.0),
                    form: Form::Text {
                        text: name.to_string(),
                        size: 12.0,
                    },
                    paint: theme("ink"),
                },
            ]
        })
        .collect();
    Overlays(vec![Overlay {
        plugin: "surveyor".into(),
        by: "Surveyor".into(),
        marks,
    }])
}

/// Spawns the marks and the tag as the realm's view does.
pub(super) fn draw(mut commands: Commands, theme: Themed) {
    overlay::spawn(&mut commands, &overlays(), &theme.palette);
}

/// Where `anchor` is seen in the picture.
fn seen(anchor: &Anchor) -> Vec2 {
    match anchor {
        Anchor::Agent(name) => AGENTS
            .iter()
            .find(|(agent, _)| agent == name)
            .map_or(COMMON, |(_, at)| *at),
        Anchor::Hamlet(_) | Anchor::Point(_) => COMMON,
    }
}

/// Places the marks at the points made up for them, over the panels,
/// and the tag in the corner of the world's panel.
#[allow(clippy::type_complexity)]
pub(super) fn place(
    mut commands: Commands,
    mut marks: Query<(
        Entity,
        &Pinned,
        &mut Node,
        &mut UiTransform,
        &mut Visibility,
    )>,
    mut tags: Query<
        (Entity, &mut Node),
        (With<Tag>, Without<Pinned>),
    >,
) {
    for (entity, pinned, mut node, mut turn, mut shown) in &mut marks
    {
        let Mark {
            anchor,
            offset,
            form,
            ..
        } = &pinned.0;
        let at = seen(anchor);
        let end = match form {
            Form::Line { to, end, .. } => {
                to.as_ref().map_or(at, seen) + *end
            }
            _ => at,
        };
        overlay::put(&mut node, &mut turn, form, at + *offset, end);
        shown.set_if_neq(Visibility::Inherited);
        commands.entity(entity).insert(GlobalZIndex(10));
    }
    for (entity, mut node) in &mut tags {
        let (left, top) = (px(CORNER.x), px(CORNER.y));
        if (node.left, node.top) != (left, top) {
            node.left = left;
            node.top = top;
        }
        commands.entity(entity).insert(GlobalZIndex(10));
    }
}
