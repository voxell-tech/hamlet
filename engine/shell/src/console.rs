//! The console: the engine's log, newest first.

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::ui::px;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row, scroll};
use bevy_fynix::{ModifierExt, ViewExt, each, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_core::{Log, LogLevel, LogLine};

use crate::AnyView;
use crate::dock::{Panel, panel};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, open);
}

pub(crate) fn id() -> PanelId {
    PanelId::new(ENGINE, "console").expect("a valid id")
}

fn open(mut commands: Commands) {
    commands.spawn(Panel {
        id: id(),
        title: "Console".into(),
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    let lines = each(
        resource::<Log, _>(|log| {
            log.lines()
                .rev()
                .map(|(number, line)| (number, line.clone()))
                .collect::<Vec<_>>()
        }),
        |(number, _)| *number,
        |(_, line)| line_view(line),
    )
    .within(column(()).gap(2.0));
    panel(scroll((lines,)).grow(1.0).min_height(px(0.0)))
}

fn line_view(line: &LogLine) -> AnyView {
    let tone = match line.level {
        LogLevel::Info => Tone::Body,
        LogLevel::Warn => Tone::Accent,
        LogLevel::Error => Tone::Critical,
    };
    row((
        label(line.source.clone()).tone(Tone::Dim).wrap(false),
        label(line.text.clone()).tone(tone).grown(1.0),
    ))
    .boxed()
}
