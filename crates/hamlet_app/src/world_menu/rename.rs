//! The form an agent or a scholar is given a new name with: a hire's
//! or a worker's nickname, or a scholar's name.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::math::{Rect, Vec2};
use bevy::ui::{AlignItems, px};
use bevy_fynix::ScopedExt;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, popup, row,
};
use hamlet_client::realm::{
    self as agents, Deeds, Kind, Left, Scholar, scholar_address,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{Looks, button, description, lettered};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::{View, built};

use super::{asks_first, close};
use crate::chat;

/// The name typed in the form.
#[derive(Resource, Default, Clone, PartialEq)]
pub(crate) struct Renaming {
    name: String,
}

/// What is renamed.
enum Named {
    /// A hire or a worker, by its address.
    Agent(String),
    Scholar(protocol::ScholarId),
}

/// What `named` is, while the chief may rename it: an agent that is
/// not the Hand and has not left, or a scholar that is not archived.
fn named(world: &World, named: Entity) -> Option<Named> {
    if let Some(scholar) = world.get::<Scholar>(named) {
        return (scholar.0.left.is_none()
            && Deeds::is_allowed(world, "name-scholar"))
        .then_some(Named::Scholar(scholar.0.id));
    }
    let kind = world.get::<Kind>(named)?.0;
    (kind != protocol::AgentKind::Hand
        && world.get::<Left>(named).is_none()
        && Deeds::is_allowed(world, "nickname"))
    .then(|| Named::Agent(agents::name_of(world, named)))
}

/// Whether the chief may give `named` a new name.
pub(super) fn may_rename(world: &World, named: Entity) -> bool {
    self::named(world, named).is_some()
}

/// Starts the form with the name `named` is shown by now.
pub(super) fn start(world: &mut World, named: Entity) {
    let name = match world.get::<Scholar>(named) {
        Some(scholar) => scholar.0.name.clone(),
        None => agents::shown_of(world, named),
    };
    world.resource_mut::<Renaming>().name = name;
}

/// Gives `named` the name typed. A hire or a worker left with no
/// name goes by its own again; a scholar needs one.
pub(super) fn submit(world: &mut World, named: Entity) {
    let typed = world.resource::<Renaming>().name.trim().to_string();
    let command = match self::named(world, named) {
        Some(Named::Agent(address)) => {
            format!("/nickname {address} {typed}")
                .trim_end()
                .to_string()
        }
        Some(Named::Scholar(id)) if !typed.is_empty() => {
            format!("/name-scholar {} {typed}", scholar_address(id))
        }
        _ => return,
    };
    close(world);
    *world.resource_mut::<Renaming>() = Renaming::default();
    chat::command(world, command);
}

/// The form `named` is given a new name with.
pub(super) fn rename(named: Entity, at: Vec2) -> impl View {
    built(move |cx| {
        let Some(what) = self::named(cx.world, named) else {
            return cx.build(frame());
        };
        let shown = match cx.world.get::<Scholar>(named) {
            Some(scholar) => scholar.0.name.clone(),
            None => agents::shown_of(cx.world, named),
        };
        let says = match what {
            Named::Agent(_) => {
                "Left empty, it goes by its own name again."
            }
            Named::Scholar(_) => "Its hires go by the new name too.",
        };
        let (gap, between, padding, heading) = {
            let theme = cx.theme();
            (
                theme.space.gap,
                theme.space.section,
                theme.space.overlay_padding,
                theme.text.heading,
            )
        };
        let body = column((
            label(format!("Rename {shown}")).size(heading).bold(true),
            asks_first::<Renaming>(
                "Its name",
                |r| &r.name,
                |r, text| r.name = text,
            ),
            description(says),
            row((
                lettered("Rename")
                    .variant(Variant::Primary)
                    .on_activate(move |world| submit(world, named)),
                lettered("Cancel").rules(button).on_activate(close),
                super::send_keys(),
            ))
            .gap(gap)
            .align(AlignItems::Center),
        ))
        .gap(between)
        .width(px(300.0));
        cx.build(
            popup(Rect::from_corners(at, at), (body,))
                .on_dismiss(|world, _| close(world))
                .padding(bevy::ui::UiRect::all(px(padding))),
        )
    })
}
