//! The Permissions page of Settings: each installed plugin's answers,
//! by plugin or by capability, each changed in place.

use bevy::app::{App, Startup, Update};
use bevy::ecs::change_detection::DetectChanges;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::query::Added;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{ModifierExt, ViewExt, keyed, resource};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{Capability, PanelId, PluginId};
use hamlet_engine_plugins::PluginInfo;
use hamlet_engine_shell::style::{Wraps, choices, note, section};
use hamlet_engine_shell::{AnyView, Cx, SettingsPage};

use crate::GrantsWorldExt;
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
        let by = choices(
            vec!["By plugin".into(), "By capability".into()],
            usize::from(shown.by_capability),
            |world, picked| {
                world.resource_mut::<Permissions>().by_capability =
                    picked == 1;
            },
        );
        let body = if shown.plugins.is_empty() {
            note("No plugin is installed.")
        } else if shown.by_capability {
            by_capability(&shown.plugins)
        } else {
            by_plugin(&shown.plugins)
        };
        groups(vec![by, body])
    })
    .boxed()
}

/// `parts` one under another, a part of a panel apart.
fn groups(parts: Vec<AnyView>) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.panel;
        cx.build(column(parts).gap(gap))
    })
}

/// A group's name over its `answers`, with what is said of it.
fn group(
    name: String,
    said: Option<&str>,
    answers: Vec<AnyView>,
) -> AnyView {
    let said = said.map_or_else(|| frame().boxed(), note);
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap / 2.0;
        cx.build(
            column((section(name), said, column(answers).gap(gap)))
                .gap(gap),
        )
    })
}

fn by_plugin(plugins: &[Line]) -> AnyView {
    let groups_of = plugins
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
            group(line.name.to_uppercase(), Some(state), answers)
        })
        .collect::<Vec<_>>();
    groups(groups_of)
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
        return note("No plugin asks for anything.");
    }
    let groups_of = capabilities
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
            group(capability.name().to_uppercase(), None, answers)
        })
        .collect::<Vec<_>>();
    groups(groups_of)
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
    let answers = choices(
        Answer::ALL.map(|answer| answer.label().to_owned()).to_vec(),
        picked,
        move |world, picked| {
            if let Some(&answer) = Answer::ALL.get(picked) {
                world.change_answer(&id, capability, &target, answer);
            }
        },
    );
    AnyView::new(move |cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            row((label(what).tone(Tone::Dim).grown(1.0), answers))
                .gap(gap)
                .align(AlignItems::Center)
                .tagged(Wraps),
        )
    })
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
