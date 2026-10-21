//! One hamlet: what it is, what can be done to it, the folder it
//! works on, and its agents, bylaws and record as tabs.

use agents_ui::{Confirm, Heard, ask, confirm, moved};
use hamlet_engine_sdk::api::agents::{
    AgentsReply, AgentsRequest, Record, Roster, RosterAgent,
    RosterHamlet, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, Cx, Face, FrameProps, Size,
    Space, Tone, UiExt, ViewExt, World, button, column, derived,
    field, heading, icon, icon_button, keyed, label, list, resource,
    row, scroll,
};

use crate::overview::{pressed_card, standing};
use crate::{bylaws, select};

/// The first hamlet, which is the home itself and stays.
pub(crate) const HOME: &str = "home";

/// The characters of a purpose shown on a card.
const PURPOSE: usize = 96;

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
#[derive(Default)]
struct Shown(Tab);

/// The name a hamlet is being given, while it is renamed.
#[derive(Default)]
struct Naming(Option<String>);

/// The record of the hamlet last asked for.
#[derive(Default, PartialEq)]
struct Records {
    of: Option<String>,
    record: Record,
}

pub(crate) fn prepare(world: &mut World) {
    world.init_resource::<Shown>();
    world.init_resource::<Naming>();
    world.init_resource::<Records>();
}

/// Reads the record again as the hamlets change.
pub(crate) fn follow(world: &mut World) {
    if let Some(of) = world.resource::<Records>().of.clone() {
        ask(world, AgentsRequest::Record(of));
    }
}

pub(crate) fn heard(world: &mut World, heard: &Heard) {
    if let Heard::Reply(
        Some(AgentsRequest::Record(of)),
        AgentsReply::Record(record),
    ) = heard
    {
        let now = Records {
            of: Some(of.clone()),
            record: record.clone(),
        };
        if *world.resource::<Records>() != now {
            world.insert(now);
        }
    }
}

/// Gives the chief's command `text` to the hamlet `id`.
fn decree(world: &mut World, id: &str, text: String) {
    ask(
        world,
        AgentsRequest::Decree {
            hamlet: id.to_string(),
            text,
        },
    );
}

/// Asks before the hamlet `id` is closed down.
pub(crate) fn close_down(world: &mut World, id: &str) {
    let working = world
        .resource::<Roster>()
        .agents
        .iter()
        .filter(|agent| {
            agent.hamlet == id && agent.busy && !agent.gone
        })
        .map(|agent| agent.name.clone())
        .collect();
    let Some(to) = crate::with(world) else {
        return;
    };
    confirm(
        world,
        Confirm {
            question: format!("CLOSE DOWN {}?", id.to_uppercase()),
            outcome: format!(
                "Everyone in {id} stops and every house is \
                 archived. You can restore it from the list of \
                 hamlets."
            ),
            working,
            verb: "CLOSE DOWN".into(),
            then: vec![AgentsRequest::Send {
                to,
                text: format!("/remove-hamlet {id}"),
            }],
        },
    );
}

/// The field a hamlet is renamed in, while it is.
fn naming(id: String) -> AnyView {
    keyed(
        resource::<Naming, _>(|naming| naming.0.is_some()),
        move |open| {
            if !*open {
                return column(()).boxed();
            }
            let id = id.clone();
            row((
                field(resource::<Naming, _>(|naming| {
                    naming.0.clone().unwrap_or_default()
                }))
                .on_commit(|world, text| {
                    world.resource_mut::<Naming>().0 = Some(text)
                }),
                button("SAVE").style(ButtonStyle::Primary).on_click(
                    move |world| {
                        let name = world
                            .resource_mut::<Naming>()
                            .0
                            .take()
                            .unwrap_or_default();
                        // No name gives it back its own.
                        decree(
                            world,
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
    let mut parts = vec![
        label("WORKS ON")
            .bold(true)
            .face(Face::Display)
            .tone(Tone::Faint)
            .boxed(),
        row((icon_button(
            "folder",
            match hamlet.folder {
                Some(_) => "CHOOSE ANOTHER",
                None => "CHOOSE A FOLDER",
            },
        )
        .folder("Choose the folder this hamlet works on")
        .on_folder(move |world, folder| {
            decree(world, &chooser, format!("/folder {folder}"))
        }),))
        .boxed(),
    ];
    match &hamlet.folder {
        Some(folder) => {
            parts.push(label(folder.clone()).boxed());
            let stopped = id.clone();
            parts.push(
                row((icon_button("cross", "STOP")
                    .style(ButtonStyle::Critical)
                    .on_click(move |world| {
                        decree(world, &stopped, "/folder none".into())
                    }),))
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
                                decree(
                                    world,
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
        None => parts.push(
            label(
                "No folder. Its agents work in their own workspaces.",
            )
            .size(Size::Small)
            .tone(Tone::Dim)
            .boxed(),
        ),
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
                        decree(
                            world,
                            &id,
                            format!("/folder {folder}"),
                        )
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
        icon_button("back", "ALL HAMLETS")
            .style(ButtonStyle::Quiet)
            .on_click(|world| select(world, Selected::Nothing))
            .boxed(),
        icon_button("pencil", "RENAME")
            .on_click(|world| {
                world.resource_mut::<Naming>().0 = Some(String::new())
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
            heading(hamlet.title.to_uppercase()),
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

/// A card of an agent of the hamlet, pressed to talk to it, or to see
/// one that is gone.
fn agent_card(agent: &RosterAgent) -> AnyView {
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
    let name = agent.name.clone();
    pressed_card(
        title,
        beside,
        vec![
            agents_ui::short(&agent.purpose, PURPOSE),
            agent.account.clone(),
        ],
        move |world| select(world, Selected::Agent(name.clone())),
    )
}

fn agents(agents: &[RosterAgent]) -> AnyView {
    let living = agents
        .iter()
        .filter(|agent| !agent.gone)
        .map(agent_card)
        .collect::<Vec<_>>();
    let gone = agents
        .iter()
        .filter(|agent| agent.gone)
        .map(agent_card)
        .collect::<Vec<_>>();
    let mut parts = vec![
        label(format!("AGENTS ({})", living.len()))
            .bold(true)
            .face(Face::Display)
            .boxed(),
        list(living).boxed(),
    ];
    if !gone.is_empty() {
        parts.push(
            label(format!("GONE ({})", gone.len()))
                .bold(true)
                .face(Face::Display)
                .boxed(),
        );
        parts.push(list(gone).boxed());
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
                                decree(
                                    world,
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

/// Shows `tab` of the hamlet `id`.
fn show(world: &mut World, id: &str, tab: Tab) {
    world.resource_mut::<Shown>().0 = tab;
    match tab {
        Tab::Agents => {}
        Tab::Bylaws => bylaws::read(world, id),
        Tab::Record => {
            world.resource_mut::<Records>().of = Some(id.to_string());
            ask(world, AgentsRequest::Record(id.to_string()));
        }
    }
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
                .resource::<Roster>()
                .hamlets
                .iter()
                .find(|hamlet| hamlet.id == of)
                .cloned()
        })
        .when(moved::<Roster>()),
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
                            world
                                .resource::<Roster>()
                                .agents
                                .iter()
                                .filter(|agent| agent.hamlet == of)
                                .cloned()
                                .collect::<Vec<_>>()
                        })
                        .when(moved::<Roster>()),
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
