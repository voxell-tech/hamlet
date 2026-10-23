//! One hamlet: what it is, what can be done to it, the folder it
//! works on, and its agents, bylaws and record as tabs.

use hamlet_engine_bevy::api::agents::{
    AgentsReply, AgentsRequest, Record, RosterAgent, RosterHamlet,
    Selected,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Cx, Face, FrameProps, Size,
    Space, Tone, UiExt, ViewExt, button, column, derived,
    description, field, icon, icon_button, keyed, label, menu_item,
    resource, row, scroll,
};
use hamlet_engine_bevy::{encode, warn};
use serde::{Deserialize, Serialize};

use crate::inspector::agent::AgentPapers;
use crate::inspector::more::About;
use crate::inspector::overview::{pressed_row, standing};
use crate::inspector::{HAMLETS, PURPOSE, bylaws, select};
use crate::shared::{
    AgentsWorldExt, Confirm, Focused, Heard, Rostered, breadcrumb,
    either, link_rows, moved, part,
};

/// The first hamlet, which is the home itself and stays.
pub(crate) const HOME: &str = "home";

/// The event that has the realm take a hamlet's town in hand, its
/// payload the hamlet's name on disk.
const CARRY: &str = "carry";

/// A rename under way is kept across a reload.
pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Shown>()
        .init_saved_resource::<Naming>()
        .init_resource::<Records>();
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Tab {
    #[default]
    Agents,
    Bylaws,
    Record,
}

const TABS: [(Tab, &str); 3] = [
    (Tab::Agents, "AGENTS"),
    (Tab::Bylaws, "BYLAWS"),
    (Tab::Record, "RECORD"),
];

/// The tab shown.
#[derive(Resource, Default)]
struct Shown(Tab);

/// The hamlet being renamed, by its name on disk, and the name it is
/// being given.
#[derive(Resource, Default, Serialize, Deserialize)]
pub(crate) struct Naming(pub(crate) Option<(String, String)>);

/// The record of the hamlet last asked for.
#[derive(Resource, Default, PartialEq)]
struct Records {
    of: Option<String>,
    record: Record,
}

/// Reads the record again as the hamlets change.
pub(crate) fn follow(world: &mut World) {
    if let Some(of) = world.resource::<Records>().of.clone() {
        world.ask(AgentsRequest::Record(of));
    }
}

pub(crate) fn heard(world: &mut World, heard: &Heard) {
    if let Heard::Reply(
        Some(AgentsRequest::Record(of)),
        AgentsReply::Record(record),
    ) = heard
        && world.resource::<Records>().of.as_ref() == Some(of)
    {
        world.keep(Records {
            of: Some(of.clone()),
            record: record.clone(),
        });
    }
}

/// Asks before the hamlet `id` is closed down.
pub(crate) fn close_down(world: &mut World, id: &str) {
    let working = world
        .resource::<Rostered>()
        .0
        .agents
        .iter()
        .filter(|agent| {
            agent.hamlet == id && agent.busy && !agent.gone
        })
        .map(|agent| agent.name.clone())
        .collect();
    let Some(to) = world.resource::<Focused>().with() else {
        return;
    };
    world.confirm(Confirm {
        question: format!("CLOSE DOWN {}?", id.to_uppercase()),
        outcome: format!(
            "Everyone in {id} stops and every house is archived. \
             You can restore it from the list of hamlets."
        ),
        working,
        verb: "CLOSE DOWN".into(),
        then: vec![AgentsRequest::Send {
            to,
            text: format!("/remove-hamlet {id}"),
        }],
    });
}

/// Has the realm take the town of the hamlet `id` in hand.
fn carry(world: &mut World, id: &str) {
    match encode(&id.to_owned()) {
        Ok(payload) => {
            world.emit(CARRY, &payload);
        }
        Err(why) => warn(&format!("carry {id}: {why}")),
    }
}

/// The field the hamlet `id` is renamed in, while it is.
fn naming(id: String) -> AnyView {
    let of = id.clone();
    keyed(
        resource::<Naming, _>(move |naming| {
            naming.0.as_ref().is_some_and(|(named, _)| *named == of)
        }),
        move |open| {
            if !*open {
                return column(()).boxed();
            }
            let (id, typed) = (id.clone(), id.clone());
            row((
                field(resource::<Naming, _>(|naming| {
                    naming
                        .0
                        .as_ref()
                        .map(|(_, name)| name.clone())
                        .unwrap_or_default()
                }))
                .on_commit(move |world, text| {
                    world.resource_mut::<Naming>().0 =
                        Some((typed.clone(), text))
                }),
                button("SAVE").style(ButtonStyle::Primary).on_click(
                    move |world| {
                        let name = world
                            .resource_mut::<Naming>()
                            .0
                            .take()
                            .map(|(_, name)| name)
                            .unwrap_or_default();
                        // No name gives it back its own.
                        world.decree(
                            &id,
                            format!(
                                "/name-hamlet {id} {}",
                                name.trim()
                            ),
                        );
                    },
                ),
                button("CANCEL").style(ButtonStyle::Quiet).on_click(
                    |world| world.resource_mut::<Naming>().0 = None,
                ),
            ))
            .space(Space::Half)
            .boxed()
        },
    )
    .boxed()
}

/// The folder the hamlet works on, and how.
fn folder(hamlet: &RosterHamlet) -> AnyView {
    let id = hamlet.id.clone();
    let chooser = id.clone();
    let choose = icon_button(
        "folder",
        match hamlet.folder {
            Some(_) => "CHOOSE ANOTHER",
            None => "CHOOSE A FOLDER",
        },
    )
    .folder("Choose the folder this hamlet works on")
    .on_folder(move |world, folder| {
        world.decree(&chooser, format!("/folder {folder}"))
    });
    let mut parts = vec![part("folder", "WORKS ON")];
    match &hamlet.folder {
        Some(folder) => {
            parts.push(label(folder.clone()).boxed());
            let stopped = id.clone();
            parts.push(
                row((
                    choose,
                    icon_button("cross", "STOP")
                        .style(ButtonStyle::Critical)
                        .on_click(move |world| {
                            world.decree(&stopped, "/folder none")
                        }),
                ))
                .space(Space::Half)
                .boxed(),
            );
            let modes =
                [("MANUAL", "manual", true), ("AUTO", "auto", false)];
            let buttons = modes
                .into_iter()
                .map(|(words, mode, manual)| {
                    let id = id.clone();
                    let now = hamlet.manual == manual;
                    button(words)
                        .style(if now {
                            ButtonStyle::Primary
                        } else {
                            ButtonStyle::Plain
                        })
                        .on_click(move |world| {
                            if !now {
                                world.decree(
                                    &id,
                                    format!("/mode {mode}"),
                                );
                            }
                        })
                        .boxed()
                })
                .collect::<Vec<_>>();
            parts.push(row(buttons).space(Space::Half).boxed());
            parts.push(
                label(hamlet.mode.clone())
                    .size(Size::Small)
                    .tone(Tone::Dim)
                    .boxed(),
            );
        }
        None => {
            parts.push(
                description(
                    "No folder. Its agents work in their own \
                     workspaces.",
                )
                .boxed(),
            );
            parts.push(row((choose,)).boxed());
        }
    }
    if !hamlet.before.is_empty() {
        parts.push(
            label("Worked on before:")
                .size(Size::Small)
                .tone(Tone::Dim)
                .boxed(),
        );
        let earlier = hamlet
            .before
            .iter()
            .map(|folder| {
                let (id, folder) = (id.clone(), folder.clone());
                let shown = folder
                    .rsplit(['/', '\\'])
                    .find(|part| !part.is_empty())
                    .unwrap_or(&folder)
                    .to_string();
                button(shown)
                    .on_click(move |world| {
                        world.decree(&id, format!("/folder {folder}"))
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        parts.push(row(earlier).space(Space::Half).boxed());
    }
    column(parts).space(Space::Half).boxed()
}

fn head(hamlet: &RosterHamlet) -> AnyView {
    let (mark, tone) = standing(hamlet);
    let id = hamlet.id.clone();
    let known = if hamlet.title == hamlet.id {
        String::new()
    } else {
        format!("Kept on disk as {}", hamlet.id)
    };
    let mut actions = vec![
        icon_button("pencil", "RENAME")
            .on_click({
                let id = id.clone();
                move |world| {
                    world.resource_mut::<Naming>().0 =
                        Some((id.clone(), String::new()))
                }
            })
            .boxed(),
        // The realm takes its town in hand, to be set down with a
        // click on the ground.
        icon_button("move", "MOVE")
            .on_click({
                let id = id.clone();
                move |world| carry(world, &id)
            })
            .boxed(),
    ];
    if hamlet.id != HOME {
        let closed = id.clone();
        actions.push(
            icon_button("struck", "CLOSE DOWN")
                .style(ButtonStyle::Critical)
                .on_click(move |world| close_down(world, &closed))
                .boxed(),
        );
    }
    column((
        row((
            breadcrumb(
                vec![HAMLETS.into()],
                hamlet.title.clone(),
                |world, _| select(world, Selected::Nothing),
            ),
            icon(mark).tone(tone),
            label(hamlet.state.clone()).tone(tone),
        ))
        .space(Space::Gap),
        label(hamlet.account.clone())
            .size(Size::Small)
            .tone(Tone::Dim),
        label(known).size(Size::Small).tone(Tone::Dim),
        row(actions).space(Space::Half),
        naming(id),
        folder(hamlet),
    ))
    .space(Space::Half)
    .boxed()
}

/// A row of an agent of the hamlet, pressed to talk to it, or to see
/// one that is gone, with its menu.
fn agent_row(
    (agent, about): &(RosterAgent, Option<About>),
) -> AnyView {
    let title = if agent.mayor {
        format!("{} (mayor)", agent.name.to_uppercase())
    } else {
        agent.name.to_uppercase()
    };
    let beside = match (agent.gone, agent.waiting, agent.busy) {
        (true, ..) => ("struck", agent.state.clone(), Tone::Critical),
        (_, 0, true) => ("bolt", agent.state.clone(), Tone::Accent),
        (_, 0, false) => ("moon", agent.state.clone(), Tone::Dim),
        (_, n, _) => {
            ("bell", format!("Waits for you ({n})"), Tone::Critical)
        }
    };
    let items = match about {
        Some(about) => about.items(),
        None => {
            let (hamlet, name) =
                (agent.hamlet.clone(), agent.name.clone());
            vec![menu_item(format!("Restore {name}"), move |world| {
                world.decree(&hamlet, format!("/restore {name}"))
            })]
        }
    };
    let name = agent.name.clone();
    pressed_row(
        title,
        beside,
        vec![
            crate::shared::short(&agent.purpose, PURPOSE),
            agent.account.clone(),
        ],
        move |world| select(world, Selected::Agent(name.clone())),
    )
    .menu(items)
    .boxed()
}

/// The agents of the hamlet `id`, each with what its menu offers.
fn residents(
    world: &World,
    id: &str,
) -> Vec<(RosterAgent, Option<About>)> {
    world
        .resource::<Rostered>()
        .0
        .agents
        .iter()
        .filter(|agent| agent.hamlet == id)
        .map(|agent| (agent.clone(), About::of(world, &agent.name)))
        .collect()
}

fn agents(agents: &[(RosterAgent, Option<About>)]) -> AnyView {
    let living = agents
        .iter()
        .filter(|(agent, _)| !agent.gone)
        .map(agent_row)
        .collect::<Vec<_>>();
    let gone = agents
        .iter()
        .filter(|(agent, _)| agent.gone)
        .map(agent_row)
        .collect::<Vec<_>>();
    let mut parts = vec![
        label(format!("AGENTS ({})", living.len()))
            .bold(true)
            .face(Face::Display)
            .boxed(),
        link_rows(living),
    ];
    if !gone.is_empty() {
        parts.push(
            label(format!("GONE ({})", gone.len()))
                .bold(true)
                .face(Face::Display)
                .boxed(),
        );
        parts.push(link_rows(gone));
    }
    column(parts).space(Space::Half).boxed()
}

fn record(id: String) -> AnyView {
    keyed(
        resource::<Records, _>(|records| records.record.clone()),
        move |record| {
            let words = |text: &str| {
                label(text.to_string())
                    .size(Size::Small)
                    .tone(Tone::Dim)
                    .boxed()
            };
            let title = |text: String| {
                label(text)
                    .bold(true)
                    .face(Face::Display)
                    .tone(Tone::Faint)
                    .boxed()
            };
            let mut parts = vec![
                title("THE BUDGET".into()),
                words(&record.budget),
                title(format!("VOTES ({})", record.votes.len())),
            ];
            if record.votes.is_empty() {
                parts.push(words("No votes yet."));
            }
            for vote in &record.votes {
                parts.push(words(&vote.report));
                if vote.open {
                    let (id, number) = (id.clone(), vote.id);
                    parts.push(
                        icon_button("restore", "OVERTURN")
                            .style(ButtonStyle::Critical)
                            .on_click(move |world| {
                                world.decree(
                                    &id,
                                    format!("/overturn {number}"),
                                )
                            })
                            .boxed(),
                    );
                }
            }
            parts.push(title("WHO CAME AND WENT".into()));
            parts.push(words(&record.record));
            column(parts).space(Space::Half).boxed()
        },
    )
    .boxed()
}

/// Shows `tab` of the hamlet `id`. What another hamlet or tab read is
/// let go, so it is not read again as the hamlets change.
fn show(world: &mut World, id: &str, tab: Tab) {
    world.resource_mut::<Shown>().0 = tab;
    let mut naming = world.resource_mut::<Naming>();
    if naming.0.as_ref().is_some_and(|(named, _)| named != id) {
        naming.0 = None;
    }
    if tab == Tab::Record {
        if world.resource::<Records>().of.as_deref() != Some(id) {
            world.insert_resource(Records {
                of: Some(id.to_string()),
                record: Record::default(),
            });
        }
        world.ask(AgentsRequest::Record(id.to_string()));
    } else {
        leave(world);
    }
    if tab == Tab::Bylaws {
        bylaws::read(world, id);
    } else {
        bylaws::leave(world);
    }
}

/// Lets go of the record, as its page is no longer shown.
pub(crate) fn leave(world: &mut World) {
    world.resource_mut::<Records>().of = None;
}

fn tabs(id: String) -> AnyView {
    keyed(resource::<Shown, _>(|shown| shown.0), move |now| {
        let buttons = TABS
            .iter()
            .map(|(tab, words)| {
                let (tab, id) = (*tab, id.clone());
                button(*words)
                    .style(if tab == *now {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Plain
                    })
                    .on_click(move |world| show(world, &id, tab))
                    .boxed()
            })
            .collect::<Vec<_>>();
        row(buttons).space(Space::Half).boxed()
    })
    .boxed()
}

/// The page of the hamlet `id`.
pub(crate) fn hamlet(id: String) -> AnyView {
    let (of, tabbed, paged) = (id.clone(), id.clone(), id.clone());
    let facts = keyed(
        derived(move |world: &World| {
            world
                .resource::<Rostered>()
                .0
                .hamlets
                .iter()
                .find(|hamlet| hamlet.id == of)
                .cloned()
        })
        .when(moved::<Rostered>()),
        |hamlet| match hamlet {
            Some(hamlet) => head(hamlet),
            None => column(()).boxed(),
        },
    );
    let page =
        keyed(resource::<Shown, _>(|shown| shown.0), move |tab| {
            let id = paged.clone();
            match tab {
                Tab::Agents => {
                    let of = id.clone();
                    keyed(
                        derived(move |world: &World| {
                            residents(world, &of)
                        })
                        .when(either(
                            moved::<Rostered>(),
                            either(
                                moved::<Focused>(),
                                moved::<AgentPapers>(),
                            ),
                        )),
                        |all| agents(all),
                    )
                    .boxed()
                }
                Tab::Bylaws => bylaws::bylaws(id),
                Tab::Record => record(id),
            }
        });
    AnyView::new(move |cx: &mut Cx| {
        // The record shown is this hamlet's.
        let tab = cx.world.resource::<Shown>().0;
        show(cx.world, &id, tab);
        cx.build(
            column((facts, tabs(tabbed), scroll((page,))))
                .space(Space::Gap),
        )
    })
}
