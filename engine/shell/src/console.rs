//! The console: the engine's log, newest first.

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row};
use bevy_fynix::{ModifierExt, ViewExt, each, resource};
use hamlet_engine_api::PanelId;
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_core::{Log, LogLevel, LogLine};

use crate::dock::{Panel, PanelIcon};
use crate::icons::Glyph;
use crate::style::{StyleExt, scroll_area};
use crate::{AnyView, View, themed};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, open);
}

pub(crate) fn id() -> PanelId {
    PanelId::new(ENGINE, "console").expect("a valid id")
}

fn open(mut commands: Commands) {
    commands.spawn((
        Panel {
            id: id(),
            title: "Console".into(),
            content: view,
        },
        PanelIcon(Glyph::Page),
    ));
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
        |(_, line)| line_view(line).boxed(),
    );
    themed(move |cx| {
        let gap = cx.theme().space.gap / 2.0;
        scroll_area((lines.within(column(()).gap(gap)),))
    })
    .in_panel()
    .boxed()
}

/// A line: where it came from, dim, and what it says in the tone of
/// its level.
fn line_view(line: &LogLine) -> impl View {
    let tone = match line.level {
        LogLevel::Info => Tone::Body,
        LogLevel::Warn => Tone::Accent,
        LogLevel::Error => Tone::Critical,
    };
    let (source, text) = (line.source.clone(), line.text.clone());
    themed(move |cx| {
        let theme = cx.theme();
        let (size, gap) = (theme.text.small, theme.space.gap);
        row((
            label(source).size(size).tone(Tone::Dim).wrap(false),
            label(text).size(size).tone(tone).grown(1.0),
        ))
        .gap(gap)
    })
}
