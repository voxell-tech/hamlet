//! The realm in a panel of its own: the endless land, a town for
//! every hamlet with a house and a robot for every agent, the roads
//! between them, and the chief's Hearth, drawn from the agents
//! service's roster. Clicking a robot, a house or a common selects it
//! in the window's focus and brings the Inspector forward.

mod ask;
mod bridge;
mod build;
mod cards;
mod folk;
mod fx;
mod places;
mod realm;
mod society;
mod sound;
mod tuning;
mod view;
mod world;

use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::scene::viewport;
use hamlet_engine_bevy::ui::{
    FrameProps, Space, Tone, UiExt, column, label, resource,
};

use crate::realm::{Pointer, focus_on_selected, reset_camera};
use crate::view::camera::Camera;
use crate::view::settings::{SkyChoice, ViewSettings};

/// The name of the panel the realm is drawn in.
const PANEL: &str = "realm";

/// The Inspector's event that has the realm take a hamlet's town in
/// hand, its payload the hamlet's name on disk.
const CARRY: &str = "hamlet.agents/carry";

#[derive(Default)]
struct RealmPlugin;

impl Plugin for RealmPlugin {
    fn build(&self, app: &mut App) {
        app.add_panel(PANEL, "Realm", panel)
            .panel_icon("globe")
            .panel_padding(Space::None)
            .add_command("reset_camera", "Reset Camera", reset_camera)
            .add_menu_entry("View", "reset_camera")
            .add_command(
                "focus",
                "Focus on Selection",
                (|| false).pipe(focus_on_selected),
            )
            .add_menu_entry("View", "focus")
            .add_shortcut("F", "focus")
            .add_command(
                "follow",
                "Follow Selection",
                (|| true).pipe(focus_on_selected),
            )
            .add_menu_entry("View", "follow")
            .add_shortcut("Shift+F", "follow")
            .add_command(
                "sky_clock",
                "Sky Follows the Clock",
                |mut settings: ResMut<ViewSettings>| {
                    settings.sky = SkyChoice::Clock
                },
            )
            .add_menu_entry("View", "sky_clock")
            .add_command(
                "sky_day",
                "Sky at Day",
                |mut settings: ResMut<ViewSettings>| {
                    settings.sky = SkyChoice::Day
                },
            )
            .add_menu_entry("View", "sky_day")
            .add_command(
                "sky_night",
                "Sky at Night",
                |mut settings: ResMut<ViewSettings>| {
                    settings.sky = SkyChoice::Night
                },
            )
            .add_menu_entry("View", "sky_night")
            .add_systems(Declare, watch)
            .add_plugins((
                realm::plugin,
                ask::plugin,
                cards::plugin,
                folk::plugin,
                world::plugin,
                fx::plugin,
            ));
    }
}

hamlet_engine_bevy::export!(RealmPlugin);

/// Watches the agents service and asks for what the realm is drawn
/// from, and hears the Inspector carry a town.
fn watch(mut commands: Commands) {
    for request in [
        AgentsRequest::Watch,
        AgentsRequest::Roster,
        AgentsRequest::Focus,
    ] {
        commands.agents(&request);
    }
    commands.subscribe(CARRY);
}

/// The realm's view, reaching the panel's edges, or why it cannot be
/// drawn.
fn panel(camera: Option<Res<Camera>>) -> AnyView {
    let Some(camera) = camera else {
        return column((label(
            "The realm cannot be drawn: the log says why",
        )
        .tone(Tone::Dim),))
        .padding(Space::Double)
        .boxed();
    };
    viewport(camera.view())
        .pointer(resource::<Pointer, _>(|pointer| pointer.0))
        .boxed()
}
