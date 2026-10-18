//! What one plugin asks for, each capability with its reason and the
//! chief's choice.

use bevy::app::{App, Update};
use bevy::ecs::query::Has;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, ResMut};
use bevy::ui::AlignItems;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_sdk::capability::{
    AskToInstall, Asks, Capability, Choice, Choices, Uninstalled,
};
use hamlet_sdk::plugin::{Disable, Enable, PluginInfo, Status};

use super::control::control;
use super::{Open, moved, switch};
use crate::icons::Glyph;
use crate::looks::{
    Fills, back, bare, good_action, hairline, note, title,
};
use crate::platform::DisplayText;
use crate::{AnyView, View, with_cx};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Details>()
        .add_systems(Update, keep.run_if(moved));
}

/// One capability of a plugin, as its page shows it.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Row {
    pub capability: Capability,
    pub reason: String,
    /// The chief's choice, if it was answered.
    pub choice: Option<Choice>,
    /// Allowed for this run only.
    pub lent: bool,
}

/// One plugin and what it asks for.
#[derive(Clone, PartialEq, Debug)]
pub(super) struct Detail {
    pub id: String,
    pub name: String,
    pub version: String,
    pub status: Status,
    pub installed: bool,
    pub rows: Vec<Row>,
}

/// Every plugin that asks for something.
#[derive(Resource, Default, PartialEq, Debug)]
pub(super) struct Details(pub Vec<Detail>);

fn keep(
    plugins: Query<(
        &PluginInfo,
        &Status,
        &Asks,
        &Choices,
        Has<Uninstalled>,
    )>,
    mut details: ResMut<Details>,
) {
    let mut now = plugins
        .iter()
        .map(|(info, status, asks, choices, uninstalled)| Detail {
            id: info.id.clone(),
            name: info.name.clone(),
            version: info.version.clone(),
            status: *status,
            installed: !uninstalled,
            rows: asks
                .0
                .iter()
                .map(|ask| Row {
                    capability: ask.capability,
                    reason: ask.reason.clone(),
                    choice: choices.choice(ask.capability),
                    lent: choices.lent(ask.capability),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    now.sort_by(|a, b| a.name.cmp(&b.name));
    if details.0 != now {
        details.0 = now;
    }
}

fn row_of(detail: &Detail, row: &Row) -> AnyView {
    let state = match (row.choice, row.lent) {
        (_, true) => "Allowed until Hamlet quits.",
        (None, _) => {
            "Not answered yet. It is asked at the first use."
        }
        (Some(Choice::AskNextTime), _) => {
            "Asked at the next use after Hamlet starts."
        }
        _ => "",
    };
    let mut parts = vec![
        label(row.capability.name())
            .size(13.0)
            .bold(true)
            .tagged(DisplayText)
            .boxed(),
        note(row.capability.words()).boxed(),
        note(format!("Why: {}", row.reason)).boxed(),
    ];
    if detail.installed {
        parts.push(control(&detail.id, row.capability, row.choice));
        if !state.is_empty() {
            parts.push(note(state).boxed());
        }
    }
    column(parts).gap(4.0).boxed()
}

fn page(detail: &Detail) -> AnyView {
    let id = detail.id.clone();
    let status = match (detail.installed, detail.status) {
        (false, _) => "Not installed",
        (_, Status::Starting) => "Starting",
        (_, Status::Running) => "Running",
        (_, Status::Disabled) => "Disabled",
        (_, Status::Failed) => "Failed",
    };
    let on =
        matches!(detail.status, Status::Starting | Status::Running);
    let control = if detail.installed {
        switch(on, move |world, on| {
            if on {
                world.trigger(Enable(id.clone()));
            } else {
                world.trigger(Disable(id.clone()));
            }
            world.flush();
        })
        .boxed()
    } else {
        let id = detail.id.clone();
        good_action(Glyph::Plus, "INSTALL", move |world| {
            world.trigger(AskToInstall { plugin: id.clone() });
            world.flush();
        })
    };
    let head = row((
        column((
            title(detail.name.to_uppercase()),
            note(format!("{} - {status}", detail.version)),
        ))
        .gap(2.0)
        .fills_width(),
        control,
    ))
    .gap(12.0)
    .align(AlignItems::Center);
    let mut rows = vec![head.boxed()];
    rows.push(with_cx(|cx| hairline(cx.theme().palette.brass_6)));
    if detail.rows.is_empty() {
        rows.push(note("It asks for nothing.").boxed());
    }
    rows.extend(detail.rows.iter().map(|row| row_of(detail, row)));
    column(rows).gap(12.0).boxed()
}

/// The plugin of `id` and what it asks for, over the way back.
pub(super) fn view() -> impl View {
    let shown = derived(|world: &bevy::ecs::world::World| {
        let open = world.resource::<Open>().0.clone();
        let details = world.resource::<Details>();
        open.and_then(|id| {
            details.0.iter().find(|d| d.id == id).cloned()
        })
    })
    .when(crate::roll::either(
        crate::roll::moved::<Open>(),
        crate::roll::moved::<Details>(),
    ));
    column((
        back("PLUGINS", |world| {
            *world.resource_mut::<Open>() = Open(None)
        }),
        keyed(shown, |detail| match detail {
            Some(detail) => page(detail),
            None => note("That plugin is gone.").boxed(),
        })
        .within(bare()),
    ))
    .gap(12.0)
}
