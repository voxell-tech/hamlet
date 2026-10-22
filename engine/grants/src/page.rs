//! The Permissions page of Settings: each installed plugin's answers,
//! by plugin or by capability, each changed in place, and what each
//! plugin that comes with Hamlet is allowed. On each plugin's page in
//! the Plugins panel, what it may do.

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
use hamlet_engine_plugins::{
    PluginInfo, PluginKind, PluginPageSection,
};
use hamlet_engine_shell::icons::Glyph;
use hamlet_engine_shell::style::{
    ButtonStyle, Wraps, action, buttons, choices, icon_button, list,
    note, section,
};
use hamlet_engine_shell::{AnyView, Cx, SettingsPage};

use crate::GrantsWorldExt;
use crate::file::{Answer, Grants};

/// The name of the Permissions page.
pub(crate) const PERMISSIONS: &str = "permissions";

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Permissions>()
        .add_systems(Startup, add)
        .add_systems(Update, follow);
}

/// The answers as the page shows them.
#[derive(Resource, Clone, PartialEq, Default)]
pub(crate) struct Permissions {
    plugins: Vec<Line>,
    by_capability: bool,
    /// The one plugin whose answers are shown, when not all are.
    only: Option<PluginId>,
}

/// A plugin's asks and the person's answers, as its page in the
/// Plugins panel shows them.
#[derive(Clone, PartialEq)]
struct Said {
    state: &'static str,
    /// Each ask, its reason and its answer.
    asks: Vec<(String, String, &'static str)>,
}

/// A plugin and its answers.
#[derive(Clone, PartialEq)]
struct Line {
    id: PluginId,
    name: String,
    allowed: bool,
    bundled: bool,
    answers: Vec<(Capability, String, Answer)>,
}

impl Permissions {
    /// Shows the answers of the plugin `id` alone, by plugin.
    pub(crate) fn show_only(&mut self, id: &PluginId) {
        self.by_capability = false;
        self.only = Some(id.clone());
    }
}

impl Said {
    /// What the plugin `info` asks, answered as in `grants`.
    fn of(grants: &Grants, info: &PluginInfo) -> Self {
        let record = grants.record(&info.id);
        let state = match (info.kind, record) {
            (PluginKind::BuiltIn, _) => {
                "Built into the engine, and not asked about"
            }
            (PluginKind::Bundled, _) => {
                "Comes with Hamlet, and is allowed all it asks"
            }
            (PluginKind::Installed, None) => "Not answered yet",
            (PluginKind::Installed, Some(record))
                if !record.allowed =>
            {
                "Not allowed"
            }
            (PluginKind::Installed, Some(_)) => "Allowed",
        };
        let asks = info
            .asks
            .iter()
            .map(|ask| {
                let answer = match info.kind {
                    PluginKind::BuiltIn => "Not asked",
                    _ => record
                        .and_then(|record| {
                            record.answer(ask.capability, &ask.target)
                        })
                        .map_or("Not answered", Answer::label),
                };
                (
                    format!(
                        "{} {}",
                        ask.capability.name(),
                        ask.target
                    ),
                    ask.reason.clone(),
                    answer,
                )
            })
            .collect();
        Self { state, asks }
    }

    /// Its state over each ask with its answer and, under them, its
    /// reason.
    fn view(&self) -> AnyView {
        if self.asks.is_empty() {
            return note(format!(
                "{}. It asks for nothing.",
                self.state
            ));
        }
        let asks = self
            .asks
            .iter()
            .map(|(what, reason, answer)| {
                let (what, answer) = (what.clone(), *answer);
                let reason = note(reason.clone());
                AnyView::new(move |cx: &mut Cx| {
                    let gap = cx.theme().space.gap;
                    cx.build(
                        column((
                            row((
                                label(what)
                                    .tone(Tone::Dim)
                                    .grown(1.0),
                                label(answer).wrap(false),
                            ))
                            .gap(gap)
                            .align(AlignItems::Center),
                            reason,
                        ))
                        .gap(gap / 2.0),
                    )
                })
            })
            .collect::<Vec<_>>();
        let state = note(format!("{}.", self.state));
        AnyView::new(move |cx: &mut Cx| {
            let gap = cx.theme().space.gap / 2.0;
            cx.build(column((state, list(asks))).gap(gap))
        })
    }
}

fn add(mut commands: Commands) {
    commands.spawn(SettingsPage {
        id: PanelId::new(ENGINE, PERMISSIONS).expect("a valid id"),
        title: "Permissions".into(),
        about: "What each plugin may do.".into(),
        icon: Glyph::Shield,
        content: view,
    });
    commands.spawn(PluginPageSection { content: may_do });
}

fn view(_: Entity) -> AnyView {
    keyed(resource::<Permissions, _>(Clone::clone), |shown| {
        let by = choices(
            vec!["By plugin".into(), "By capability".into()],
            usize::from(shown.by_capability),
            |world, picked| {
                let mut shown = world.resource_mut::<Permissions>();
                shown.by_capability = picked == 1;
                shown.only = None;
            },
        );
        let only = shown.only.as_ref().and_then(|id| {
            shown.plugins.iter().find(|line| line.id == *id)
        });
        let body = if shown.plugins.is_empty() {
            note("No plugin is installed.")
        } else if shown.by_capability {
            by_capability(&shown.plugins)
        } else if let Some(line) = only {
            let all = action(ButtonStyle::Quiet, "ALL PLUGINS")
                .on_activate(|world| {
                    world.resource_mut::<Permissions>().only = None;
                })
                .boxed();
            groups(vec![
                by_plugin(std::slice::from_ref(line)),
                buttons(vec![all]),
            ])
        } else {
            by_plugin(&shown.plugins)
        };
        groups(vec![by, body])
    })
    .boxed()
}

/// WHAT IT MAY DO on the page of `plugin`: each of its asks and the
/// person's answer, and for an installed one, a way to Permissions to
/// change them.
fn may_do(plugin: Entity) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let Some(info) = cx.world.get::<PluginInfo>(plugin).cloned()
        else {
            return cx.build(column(()));
        };
        let id = info.id.clone();
        let installed = info.kind == PluginKind::Installed;
        let said = keyed(
            resource::<Grants, _>(move |grants| {
                Said::of(grants, &info)
            }),
            Said::view,
        )
        .within(column(()).gap(0.0));
        let mut parts = vec![section("WHAT IT MAY DO"), said.boxed()];
        if installed {
            parts.push(buttons(vec![
                icon_button(
                    ButtonStyle::Plain,
                    Glyph::Shield,
                    "CHANGE IN PERMISSIONS",
                )
                .on_activate(move |world| world.show_permissions(&id))
                .boxed(),
            ]));
        }
        let gap = cx.theme().space.gap / 2.0;
        cx.build(column(parts).gap(gap))
    })
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
            let state = match (
                line.bundled,
                line.allowed,
                line.answers.is_empty(),
            ) {
                (true, _, _) => {
                    "Comes with Hamlet, and is allowed all it asks"
                }
                (false, false, _) => "Not allowed",
                (false, true, true) => {
                    "Allowed, and asks for nothing"
                }
                (false, true, false) => "Allowed",
            };
            let answers = line
                .answers
                .iter()
                .map(|(capability, target, answer)| {
                    answer_view(
                        format!("{} {target}", capability.name()),
                        line,
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
                                line,
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

/// One answer of the plugin of `line`, under `what`, with the three
/// to choose from, or as it is for a plugin that comes with Hamlet.
fn answer_view(
    what: String,
    line: &Line,
    capability: Capability,
    target: &str,
    answer: Answer,
) -> AnyView {
    let (id, target) = (line.id.clone(), target.to_owned());
    let picked =
        Answer::ALL.iter().position(|a| *a == answer).unwrap_or(1);
    let answers = if line.bundled {
        label(answer.label()).boxed()
    } else {
        choices(
            Answer::ALL
                .map(|answer| answer.label().to_owned())
                .to_vec(),
            picked,
            move |world, picked| {
                if let Some(&answer) = Answer::ALL.get(picked) {
                    world.change_answer(
                        &id, capability, &target, answer,
                    );
                }
            },
        )
    };
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
        // What a plugin that no longer comes with Hamlet was allowed
        // answers for nothing.
        .filter(|(id, record)| {
            !record.bundled
                || plugins.iter().any(|info| info.id == **id)
        })
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
            bundled: record.bundled,
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
