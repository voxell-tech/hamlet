//! The About page of Settings: the engine's name and version, and the
//! version of the interface its plugins are built against.

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{API_VERSION, PanelId};

use crate::icons::Glyph;
use crate::style::heading;
use crate::{AnyView, Cx, SettingsPage};

/// The name of the About page.
const ABOUT: &str = "about";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, add);
}

fn add(mut commands: Commands) {
    commands.spawn(SettingsPage {
        id: PanelId::new(ENGINE, ABOUT).expect("a valid id"),
        title: "About".into(),
        about: "The engine's version and its plugins' interface."
            .into(),
        icon: Glyph::House,
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                heading("Hamlet"),
                label(format!(
                    "Engine {}",
                    env!("CARGO_PKG_VERSION")
                )),
                label(format!("Plugin interface {API_VERSION}"))
                    .tone(Tone::Dim),
            ))
            .gap(gap),
        )
    })
}
