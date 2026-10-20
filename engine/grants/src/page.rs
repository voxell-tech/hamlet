//! The Permissions page of Settings: each installed plugin's answers,
//! by plugin or by capability, each changed in place.

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::Added;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, label, row, segmented};
use bevy_fynix::{ModifierExt, ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{Capability, PanelId, PluginId};
use hamlet_engine_core::LogLine;
use hamlet_engine_plugins::PluginInfo;
use hamlet_engine_shell::{AnyView, SettingsPage};

use crate::file::{Answer, Grants};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Permissions>()
        .add_systems(Startup, add)
        .add_systems(Update, follow);
}

/// The answers as the page shows them.
#[derive(Resource, Clone, PartialEq, Default)]
struct Permissions {
    plugins: Vec<Line>,
    by_capability: bool,
}

/// A plugin and its answers.
#[derive(Clone, PartialEq)]
struct Line {
    id: PluginId,
    name: String,
    allowed: bool,
    answers: Vec<(Capability, String, Answer)>,
}

fn add(mut commands: Commands) {
    commands.spawn(SettingsPage {
        id: PanelId::new(ENGINE, "permissions").expect("a valid id"),
        title: "Permissions".into(),
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    keyed(resource::<Permissions, _>(Clone::clone), |shown| {
        let by = segmented(
            ["By plugin", "By capability"],
            usize::from(shown.by_capability),
            |world, picked| {
                world.resource_mut::<Permissions>().by_capability =
                    picked == 1;
            },
        );
        let body = if shown.plugins.is_empty() {
            label("No plugin is installed.").tone(Tone::Dim).boxed()
        } else if shown.by_capability {
            by_capability(&shown.plugins)
        } else {
            by_plugin(&shown.plugins)
        };
        column((by, body)).gap(16.0).boxed()
    })
    .boxed()
}

fn by_plugin(plugins: &[Line]) -> AnyView {
    let groups = plugins
        .iter()
        .map(|line| {
            let state = match (line.allowed, line.answers.is_empty())
            {
                (false, _) => "Not allowed",
                (true, true) => "Allowed, and asks for nothing",
                (true, false) => "Allowed",
            };
            let answers = line
                .answers
                .iter()
                .map(|(capability, target, answer)| {
                    answer_view(
                        format!("{} {target}", capability.name()),
                        &line.id,
                        *capability,
                        target,
                        *answer,
                    )
                })
                .collect::<Vec<_>>();
            column((
                label(line.name.clone()),
                label(state).tone(Tone::Dim),
                column(answers).gap(4.0),
            ))
            .gap(4.0)
            .boxed()
        })
        .collect::<Vec<_>>();
    column(groups).gap(16.0).boxed()
}

fn by_capability(plugins: &[Line]) -> AnyView {
    let mut capabilities = plugins
        .iter()
        .flat_map(|line| {
            line.answers.iter().map(|(capability, ..)| *capability)
        })
        .collect::<Vec<_>>();
    capabilities.sort();
    capabilities.dedup();
    if capabilities.is_empty() {
        return label("No plugin asks for anything.")
            .tone(Tone::Dim)
            .boxed();
    }
    let groups = capabilities
        .into_iter()
        .map(|capability| {
            let answers = plugins
                .iter()
                .filter(|line| line.allowed)
                .flat_map(|line| {
                    line.answers
                        .iter()
                        .filter(move |(asked, ..)| {
                            *asked == capability
                        })
                        .map(move |(_, target, answer)| {
                            answer_view(
                                format!("{} {target}", line.name),
                                &line.id,
                                capability,
                                target,
                                *answer,
                            )
                        })
                })
                .collect::<Vec<_>>();
            column((
                label(capability.name()),
                column(answers).gap(4.0),
            ))
            .gap(4.0)
            .boxed()
        })
        .collect::<Vec<_>>();
    column(groups).gap(16.0).boxed()
}

/// One answer, under `what`, with the three to choose from.
fn answer_view(
    what: String,
    id: &PluginId,
    capability: Capability,
    target: &str,
    answer: Answer,
) -> AnyView {
    let (id, target) = (id.clone(), target.to_owned());
    let picked =
        Answer::ALL.iter().position(|a| *a == answer).unwrap_or(1);
    row((
        label(what).tone(Tone::Dim).grown(1.0),
        segmented(
            Answer::ALL.map(Answer::label),
            picked,
            move |world, picked| {
                if let Some(&answer) = Answer::ALL.get(picked) {
                    change(world, &id, capability, &target, answer);
                }
            },
        ),
    ))
    .gap(8.0)
    .align(AlignItems::Center)
    .boxed()
}

fn change(
    world: &mut World,
    id: &PluginId,
    capability: Capability,
    target: &str,
    answer: Answer,
) {
    let mut grants = world.resource_mut::<Grants>();
    let Some(mut record) = grants.record(id).cloned() else {
        return;
    };
    record.set(capability, target, answer);
    if let Err(why) = grants.set(id, record) {
        world.write_message(LogLine::error(
            "grants",
            format!("{} cannot be written: {why}", crate::FILE),
        ));
    }
}

/// Gathers the lines again when the answers change, or a plugin comes
/// or goes.
fn follow(
    grants: Res<Grants>,
    added: Query<(), Added<PluginInfo>>,
    mut gone: RemovedComponents<PluginInfo>,
    plugins: Query<&PluginInfo>,
    mut shown: ResMut<Permissions>,
) {
    if !grants.is_changed()
        && added.is_empty()
        && gone.read().count() == 0
    {
        return;
    }
    let lines = grants
        .records()
        .map(|(id, record)| Line {
            id: id.clone(),
            name: plugins
                .iter()
                .find(|info| info.id == *id)
                .map_or_else(
                    || id.to_string(),
                    |info| info.name.clone(),
                ),
            allowed: record.allowed,
            answers: record
                .answers
                .iter()
                .map(|answered| {
                    (
                        answered.capability,
                        answered.target.clone(),
                        answered.answer,
                    )
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    if shown.plugins != lines {
        shown.plugins = lines;
    }
}
