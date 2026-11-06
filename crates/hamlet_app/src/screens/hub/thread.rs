//! The Hub's thread: the chat with one agent under a header that
//! says where it is, over a composer with its chips. The agent's
//! details take its place while open, and its sessions and prompt are
//! pages of their own.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Display, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, dropdown,
    frame, icon, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, every_frame, keyed, resource,
};
use hamlet_client::realm::{
    Activity, Busy, Kind, Left, LivesIn, Nickname, Parent, PeerChats,
    Settings, Works, id_of, name_of,
};
use hamlet_client::waits::Waiting;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Thing, select};
use hamlet_ui::styles::{
    Fills, ICON, back_link, bad_action, bare, close, filling,
    hairline, lettered, note, scroll_area,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Wraps;
use hamlet_ui::widgets::tabs;
use hamlet_ui::{AnyView, View, built, with_cx};

use super::{Showing, conversations, details, show};
use crate::chat;
use crate::convo::Convo;
use crate::history::History;
use crate::models::Shelf;
use crate::overview::tell;
use crate::roll::rolled;
use crate::screens::petition::{self, Need};
use crate::world_menu::{self};

/// What the thread's page shows: its chat, or its details in the
/// chat's place.
#[derive(Resource, Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Sheet {
    #[default]
    Chat,
    Details,
}

/// The tabs between a thread's chat and its details.
const SHEETS: [(Sheet, Glyph, &str); 2] = [
    (Sheet::Chat, Glyph::Talk, "CHAT"),
    (Sheet::Details, Glyph::Sliders, "DETAILS"),
];

/// The system prompt of an agent, as it was last read for the chief.
#[derive(Resource, Default, PartialEq)]
pub struct Briefing {
    pub agent: Option<Entity>,
    pub text: String,
}

/// An agent's purpose cut short, whole under the pointer.
pub(super) fn said(purpose: String) -> AnyView {
    let shown =
        crate::hover::short(&purpose, crate::hover::PURPOSE_SHOWN);
    if shown == purpose {
        return dim(purpose).boxed();
    }
    label(shown)
        .tone(Tone::Dim)
        .tooltip(move || {
            column((note(purpose.clone()),)).max_width(px(320.0))
        })
        .boxed()
}

pub(super) fn dim(text: impl Into<String>) -> impl View {
    label(text.into()).tone(Tone::Dim)
}

/// Where the session menu leads.
#[derive(Clone, PartialEq)]
enum Go {
    /// The chief's own chat with the agent.
    Chief,
    /// A session of the agent's, read in the sessions page.
    Past(i64),
    Peer(protocol::ConversationView),
    Worker(Entity),
}

/// The names of the session menu, what each leads to, and which of
/// them the thread shows now.
fn entries(world: &World) -> (Vec<(String, Go)>, usize) {
    let agent = world.resource::<Convo>().agent;
    let pasts = world
        .get_resource::<History>()
        .map(|history| history.pasts.as_slice())
        .unwrap_or_default();
    let live = pasts.iter().find(|past| past.current);
    let mut items = vec![(
        live.map_or("Session".to_string(), |past| {
            format!("Session {}", past.number)
        }),
        Go::Chief,
    )];
    let mut earlier = pasts
        .iter()
        .filter(|past| !past.current)
        .collect::<Vec<_>>();
    earlier.sort_by_key(|past| std::cmp::Reverse(past.number));
    for past in earlier {
        items.push((
            format!("Session {}", past.number),
            Go::Past(past.number),
        ));
    }
    let peers = world
        .get::<PeerChats>(agent)
        .map(|chats| chats.0.clone())
        .unwrap_or_default();
    for peer in peers {
        let kind = match peer.kind {
            protocol::ConversationViewKind::Letters => "Letters",
            protocol::ConversationViewKind::Questions => "Questions",
        };
        items.push((
            format!(
                "{kind} with {}",
                conversations::name_of_peer(world, peer.with)
            ),
            Go::Peer(peer),
        ));
    }
    for worker in workers(world, agent) {
        items.push((name_of(world, worker), Go::Worker(worker)));
    }
    let now = conversations::peer_shown(world)
        .and_then(|shown| {
            items.iter().position(|(_, go)| {
                matches!(go, Go::Peer(peer) if *peer == shown)
            })
        })
        .unwrap_or(0);
    (items, now)
}

/// The living workers `agent` has started.
pub(super) fn workers(world: &World, agent: Entity) -> Vec<Entity> {
    let Some(id) = id_of(world, agent) else {
        return Vec::new();
    };
    let Some(mut query) = world.try_query_filtered::<(
        Entity,
        &Kind,
        &Parent,
    ), bevy::ecs::query::Without<Left>>() else {
        return Vec::new();
    };
    let mut found = query
        .iter(world)
        .filter(|(_, kind, parent)| {
            kind.0 == protocol::AgentKind::Worker && parent.0 == id
        })
        .map(|(worker, ..)| worker)
        .collect::<Vec<_>>();
    found.sort();
    found
}

fn go(world: &mut World, to: &Go) {
    match to {
        Go::Chief => conversations::chief(world),
        Go::Past(number) => {
            crate::history::open(world, *number);
            show(world, Showing::Sessions);
        }
        Go::Peer(peer) => conversations::open(world, peer.clone()),
        Go::Worker(worker) => crate::convo::talk_to(world, *worker),
    }
}

/// The session menu: a button saying which session this is, over a
/// list of the others. A choice shows the chat, even from the
/// details.
fn session_menu() -> impl View {
    keyed(
        derived(|world: &World| {
            let (items, now) = entries(world);
            (
                items
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                items
                    .into_iter()
                    .map(|(_, to)| to)
                    .collect::<Vec<_>>(),
                now,
            )
        })
        .when(every_frame()),
        |(names, goes, now)| {
            let (goes, now) = (goes.clone(), *now);
            dropdown(
                names.clone(),
                now,
                Glyph::Chevron.image(ICON),
                move |world, chosen| {
                    if *world.resource::<Sheet>() != Sheet::Chat {
                        *world.resource_mut::<Sheet>() = Sheet::Chat;
                    }
                    if chosen != now
                        && let Some(to) = goes.get(chosen)
                    {
                        go(world, to);
                    }
                },
            )
            .boxed()
        },
    )
    .within(bare())
}

/// A part of the path over the thread that leads somewhere.
fn step(
    text: String,
    to: impl Fn(&mut World) + Send + Sync + 'static,
) -> impl View {
    button(label(text).wrap(false))
        .padding(UiRect::all(px(0.0)))
        .rules(back_link)
        .tagged(Variant::Text)
        .on_activate(to)
}

fn slash() -> impl View {
    label("/").tone(Tone::Dim).wrap(false)
}

/// What `agent` is called in the path: its nickname, else its address
/// without its hamlet, and the Hand as itself.
fn called(world: &World, agent: Entity) -> String {
    if world.get::<Kind>(agent).map(|kind| kind.0)
        == Some(protocol::AgentKind::Hand)
    {
        return "The Hand".into();
    }
    match world.get::<Nickname>(agent) {
        Some(nickname) => nickname.0.clone(),
        None => hamlet_realm::pick::short(&name_of(world, agent))
            .to_string(),
    }
}

/// The chain of command down to the thread: `Home / hamlet / mayor /
/// lead / agent / Session n`. Each step above the agent leads to its
/// page or its thread, and the session to the menu of the others.
fn path(agent: Entity) -> impl View {
    built(move |cx| {
        let world = &*cx.world;
        let hamlet = world.get::<LivesIn>(agent).map(|l| l.0);
        let mut parts = vec![
            step("Realm".into(), |world| show(world, Showing::Home))
                .boxed(),
        ];
        if let Some(hamlet) = hamlet {
            parts.push(slash().boxed());
            parts.push(
                step(name_of(world, hamlet), move |world| {
                    select(world, Some(Thing::Common(hamlet)));
                    show(world, Showing::Hamlet(hamlet));
                })
                .boxed(),
            );
        }
        // The leads above it in its hamlet, the mayor first.
        let mut above = Vec::new();
        let mut at = agent;
        while let Some(parent) =
            world.get::<Parent>(at).and_then(|parent| {
                hamlet_client::realm::agent(world, parent.0)
            })
        {
            let same = world.get::<LivesIn>(parent).map(|l| l.0);
            if above.contains(&parent)
                || hamlet.is_none()
                || same != hamlet
            {
                break;
            }
            above.insert(0, parent);
            at = parent;
        }
        for lead in above {
            parts.push(slash().boxed());
            parts.push(
                step(called(world, lead), move |world| {
                    crate::convo::talk_to(world, lead);
                })
                .boxed(),
            );
        }
        parts.push(slash().boxed());
        let address = name_of(world, agent);
        parts.push(
            label(called(world, agent))
                .bold(true)
                .wrap(false)
                .tooltip(move || note(address.clone()))
                .boxed(),
        );
        let gap = cx.theme().space.gap;
        cx.build(
            row(parts)
                .gap(gap)
                .align(AlignItems::Center)
                .tagged(Wraps),
        )
    })
}

/// The waits `agent` raised that the chief has not answered, oldest
/// first.
fn waits_of(world: &World, agent: Entity) -> Vec<Entity> {
    let address = name_of(world, agent);
    let Some(mut query) = world.try_query::<(Entity, &Waiting)>()
    else {
        return Vec::new();
    };
    let mut found = query
        .iter(world)
        .filter(|(_, waiting)| waiting.by == address)
        .map(|(entity, waiting)| (waiting.since, entity))
        .collect::<Vec<_>>();
    found.sort();
    found.into_iter().map(|(_, entity)| entity).collect()
}

/// What the agent waits on the chief for, each with its answers, over
/// its chat, so a row of NEEDS YOU lands where it can be answered.
fn waits(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| waits_of(world, agent))
            .when(every_frame()),
        |waits| {
            let cards = waits
                .iter()
                .copied()
                .map(|wait| {
                    AnyView::new(move |cx: &mut hamlet_ui::Cx| {
                        match cx.world.get::<Waiting>(wait) {
                            Some(waiting) => {
                                let need = Need::of(wait, waiting);
                                cx.build(petition::strip(&need))
                            }
                            None => cx.build(frame()),
                        }
                    })
                })
                .collect::<Vec<_>>();
            if cards.is_empty() {
                return frame().boxed();
            }
            // Never more than part of the page, so the chat stays in
            // view; the rest scrolls.
            column((scroll_area((column(cards).gap(8.0),)),))
                .max_height(percent(40.0))
                .min_height(px(0.0))
                .boxed()
        },
    )
    .within(bare())
}

/// The state shown for `agent`: its waits first, then work or rest.
fn standing(world: &World, agent: Entity) -> (Glyph, Tone, String) {
    if !waits_of(world, agent).is_empty() {
        return (Glyph::Bell, Tone::Critical, "Waits for you".into());
    }
    let words = world
        .get::<Activity>(agent)
        .cloned()
        .unwrap_or_default()
        .words;
    match world.get::<Busy>(agent) {
        Some(_) => (Glyph::Bolt, Tone::Accent, words),
        None => (Glyph::Moon, Tone::Faint, words),
    }
}

/// What it is doing, with the mark of its state.
fn state(agent: Entity) -> impl View {
    let words = label(rolled(move |world| standing(world, agent).2))
        .tone(rolled(move |world| standing(world, agent).1));
    let mark = icon(rolled(move |world| {
        standing(world, agent).0.image(ICON)
    }))
    .size(ICON)
    .tone(rolled(move |world| standing(world, agent).1));
    row((mark, words.wrap(false)))
        .gap(5.0)
        .align(AlignItems::Center)
}

fn header(agent: Entity) -> impl View {
    built(move |cx| {
        // Everything else that can be done to it, as a right click on
        // it in the world offers: a new session, gifts, its prompt,
        // stopping, roads, merging, ending.
        let more = button(hamlet_ui::widgets::icons::glyph(
            Glyph::More,
            ICON,
        ))
        .padding(UiRect::axes(px(8.0), px(4.0)))
        .rules(hamlet_ui::styles::button)
        .tagged(Variant::Default)
        .tooltip(|| {
            note("A new session, its prompt, gifts, roads, ending it")
        })
        .on_activate(move |world| {
            let hall = hamlet_client::realm::first_mayor(world);
            let thing = if hall == Some(agent) {
                Thing::Keeper
            } else {
                Thing::Agent(agent)
            };
            world_menu::open_for(world, thing);
        });
        // Which of its chats the chat tab shows: this session, an
        // earlier one, its letters and questions, its workers. It is
        // there on both tabs, so that the tabs keep their place.
        let sessions = session_menu().tooltip(|| {
            note(
                "Switch to an earlier session, its letters or \
                 questions with other agents, or a worker it started",
            )
        });
        let gap = cx.theme().space.gap;
        cx.build(
            // The path with its state; under it the tabs between the
            // chat and the details, the one shown underlined, with
            // what the chat tab shows and the rest at their end.
            column((
                row((path(agent), frame().grow(1.0), state(agent)))
                    .gap(gap)
                    .align(AlignItems::Center)
                    .tagged(Wraps),
                row((
                    column((tabs::strip(&SHEETS, |world, sheet| {
                        *world.resource_mut::<Sheet>() = sheet;
                    }),))
                    .grow(1.0)
                    .min_width(px(0.0)),
                    sessions,
                    more,
                ))
                .gap(gap)
                .align(AlignItems::Center),
            ))
            .gap(gap),
        )
    })
}

/// The agent's model, with the effort it thinks at.
fn model_words(world: &World, agent: Entity) -> String {
    let model = world
        .get::<hamlet_client::realm::Account>(agent)
        .map(|account| account.model.clone())
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| "default model".into());
    let effort = world
        .get_resource::<Settings>()
        .and_then(|settings| settings.0.effort);
    match effort {
        Some(effort) => {
            format!("{model} · {effort:?}").to_lowercase()
        }
        None => model,
    }
}

/// The models a chief may pick for a hire, the first being its
/// hamlet's own.
fn model_choices(world: &World, agent: Entity) -> Vec<String> {
    let mut names = vec!["hamlet's model".to_string()];
    names.extend(
        world
            .get_resource::<Shelf>()
            .into_iter()
            .flat_map(|shelf| &shelf.models)
            .filter(|model| model.allowed)
            .map(|model| model.name.clone()),
    );
    if let Some(now) = world
        .get::<hamlet_client::realm::Account>(agent)
        .map(|account| account.model.clone())
        .filter(|now| !now.is_empty() && !names.contains(now))
    {
        names.push(now);
    }
    names
}

/// The models the Hand may speak with, the one it does among them.
fn default_choices(world: &World, agent: Entity) -> Vec<String> {
    let mut names = world
        .get_resource::<Shelf>()
        .into_iter()
        .flat_map(|shelf| &shelf.models)
        .filter(|model| model.allowed)
        .map(|model| model.name.clone())
        .collect::<Vec<_>>();
    if let Some(now) = world
        .get::<hamlet_client::realm::Account>(agent)
        .map(|account| account.model.clone())
        .filter(|now| !now.is_empty() && !names.contains(now))
    {
        names.insert(0, now);
    }
    names
}

/// The Hand's chip: it speaks with the realm's default model, which
/// a pick here changes.
fn hand_model_chip(agent: Entity) -> AnyView {
    keyed(
        derived(move |world: &World| {
            (default_choices(world, agent), model_words(world, agent))
        })
        .when(every_frame()),
        move |(names, now)| {
            let names = names.clone();
            let at = names
                .iter()
                .position(|name| now.starts_with(name.as_str()))
                .unwrap_or(0);
            dropdown(
                names.clone(),
                at,
                Glyph::Chevron.image(ICON),
                move |world, chosen| {
                    if chosen == at {
                        return;
                    }
                    if let Some(picked) = names.get(chosen) {
                        crate::models::save_default(world, picked);
                    }
                },
            )
            .boxed()
        },
    )
    .within(bare())
    .boxed()
}

/// The chip that shows the model and effort, and for a hire its
/// hamlet's mayor can change it from, and for the Hand the realm's
/// default.
fn model_chip(agent: Entity) -> impl View {
    let kind =
        rolled(move |world| world.get::<Kind>(agent).map(|k| k.0));
    keyed(kind, move |kind| {
        if *kind == Some(protocol::AgentKind::Hand) {
            return hand_model_chip(agent);
        }
        if *kind != Some(protocol::AgentKind::Hire) {
            return label(rolled(move |world| {
                model_words(world, agent)
            }))
            .tone(Tone::Dim)
            .wrap(false)
            .boxed();
        }
        keyed(
            derived(move |world: &World| {
                (
                    model_choices(world, agent),
                    model_words(world, agent),
                )
            })
            .when(every_frame()),
            move |(names, now)| {
                let names = names.clone();
                let was = now.clone();
                let at = names
                    .iter()
                    .position(|name| now.starts_with(name.as_str()))
                    .unwrap_or(0);
                dropdown(
                    names.clone(),
                    at,
                    Glyph::Chevron.image(ICON),
                    move |world, chosen| {
                        let Some(picked) = names.get(chosen) else {
                            return;
                        };
                        if *picked == was {
                            return;
                        }
                        let model = if chosen == 0 {
                            "none"
                        } else {
                            picked.as_str()
                        };
                        let Some(hamlet) = world
                            .get::<LivesIn>(agent)
                            .map(|lives| lives.0)
                        else {
                            return;
                        };
                        let address = name_of(world, agent);
                        tell(
                            world,
                            hamlet,
                            format!("/model {address} {model}"),
                        );
                    },
                )
                .boxed()
            },
        )
        .within(bare())
        .boxed()
    })
    .within(bare())
}

/// The chip of the hamlet's mode, which a press turns over.
fn mode_chip(agent: Entity) -> impl View {
    keyed(
        derived(move |world: &World| {
            let lives = world.get::<LivesIn>(agent)?.0;
            Some((lives, world.get::<Works>(lives)?.mode))
        })
        .when(every_frame()),
        move |at| {
            let Some((hamlet, mode)) = *at else {
                return frame().boxed();
            };
            let words = match mode {
                hamlet_client::realm::Mode::Manual => "MANUAL",
                hamlet_client::realm::Mode::Auto => "AUTO",
            };
            lettered(words)
                .rules(hamlet_ui::styles::button)
                .tagged(Variant::Default)
                .tooltip(|| {
                    note(
                        "How this hamlet's work is overseen. Press \
                         to turn it over.",
                    )
                })
                .on_activate(move |world| {
                    let to = match mode {
                        hamlet_client::realm::Mode::Manual => "auto",
                        hamlet_client::realm::Mode::Auto => "manual",
                    };
                    tell(world, hamlet, format!("/mode {to}"));
                })
                .boxed()
        },
    )
    .within(bare())
}

/// The `+` that offers the skills an agent could be given.
fn gifts(agent: Entity) -> impl View {
    row((button(hamlet_ui::widgets::icons::glyph(Glyph::Plus, ICON))
        .padding(UiRect::axes(px(8.0), px(0.0)))
        .min_height(px(hamlet_ui::styles::CONTROL))
        .tooltip(|| note("Give it a skill"))
        .rules(hamlet_ui::styles::button)
        .tagged(Variant::Default)
        .on_activate(move |world| {
            world_menu::open_skills(world, agent);
        }),))
    .display(rolled(move |world| {
        if hamlet_client::gifts::offers(world, agent).is_empty() {
            Display::None
        } else {
            Display::Flex
        }
    }))
}

/// Stops what the agent is doing, there only while it is working.
fn stop(agent: Entity) -> impl View {
    row((bad_action(Glyph::Stop, "STOP", move |world| {
        world_menu::stop(world, agent);
    }),))
    .display(rolled(move |world| {
        if conversations::is_read_only(world) {
            return Display::None;
        }
        match world.get::<Busy>(agent) {
            Some(_) => Display::Flex,
            None => Display::None,
        }
    }))
}

/// Under the message field: the gifts, the model and effort, the
/// hamlet's mode, and the stop.
fn chips(agent: Entity) -> impl View {
    with_cx(move |cx| {
        row((
            gifts(agent),
            model_chip(agent),
            mode_chip(agent),
            frame().grow(1.0),
            stop(agent),
        ))
        .gap(cx.theme().space.gap)
        .align(AlignItems::Center)
        .tagged(Wraps)
    })
}

/// The transcript over its composer, or over a note when the chat
/// is only watched.
fn talk(agent: Entity) -> AnyView {
    keyed(resource::<Convo, _>(|m| m.session), move |_| {
        with_cx(move |cx| {
            let gap = cx.theme().space.gap;
            // Another's chat is only watched, and leads back to the
            // chief's by the path over it.
            if conversations::is_read_only(cx.world) {
                column((
                    chat::transcript(),
                    close((note("READ ONLY"),)),
                ))
                .fills_height()
                .gap(gap)
                .boxed()
            } else {
                column((
                    chat::transcript(),
                    chat::message_bar(),
                    chips(agent),
                ))
                .fills_height()
                .gap(gap)
                .boxed()
            }
        })
        .boxed()
    })
    .within(filling())
    .boxed()
}

/// The thread of the agent the chat is with.
pub(super) fn living() -> impl View {
    keyed(
        resource::<Convo, _>(|m| (m.session, m.agent)),
        |(_, agent)| {
            let agent = *agent;
            // The details take the chat's place while they are open.
            let body = keyed(
                resource::<Sheet, _>(|sheet| {
                    *sheet == Sheet::Details
                }),
                move |open| {
                    if *open {
                        details::sheet(agent).boxed()
                    } else {
                        talk(agent)
                    }
                },
            )
            .within(filling());
            with_cx(move |cx| {
                let gap = cx.theme().space.gap;
                column((header(agent), waits(agent), body))
                    .fills_height()
                    .gap(gap)
            })
            .boxed()
        },
    )
    .within(filling())
}

/// Reads the prompt the agent the chat is with would be given now,
/// and turns the page to it.
pub(crate) fn show_prompt(world: &mut World) {
    let convo = world.resource::<Convo>();
    let agent = convo.agent;
    *world.resource_mut::<Briefing>() = Briefing {
        agent: Some(agent),
        text: String::new(),
    };
    super::papers::prompt(world);
    show(world, Showing::Prompt);
}

/// Leaves the prompt of an agent the chat is no longer with.
pub(super) fn leave_prompt(
    convo: Res<Convo>,
    briefing: Res<Briefing>,
    mut showing: ResMut<Showing>,
) {
    if *showing == Showing::Prompt
        && briefing.agent != Some(convo.agent)
    {
        *showing = Showing::Thread;
    }
}

/// A page about the agent the chat is with, under the way back.
fn aside(name: &'static str, body: impl View + 'static) -> impl View {
    with_cx(move |cx| {
        column((
            hamlet_ui::styles::breadcrumb(
                vec!["THREAD".into()],
                name,
                |world, _| show(world, Showing::Thread),
            ),
            body,
        ))
        .fills_height()
        .gap(cx.theme().space.section)
    })
}

/// The whole of the agent's prompt.
pub(super) fn prompt_page() -> impl View {
    let shown =
        keyed(resource::<Briefing, _>(|b| b.text.clone()), |text| {
            // Each line as the model is given it, where Markdown
            // would run them together.
            hamlet_ui::markdown::fixed(
                &text.replace('\n', "  \n"),
                Tone::Body,
            )
            .boxed()
        })
        .within(bare());
    with_cx(move |cx| {
        let gold = cx.theme().palette.gold_6;
        let gap = cx.theme().space.section;
        aside(
            "PROMPT",
            column((
                note(
                    "What this agent is told at the start of a turn.",
                ),
                hairline(gold),
                scroll_area((shown,)),
            ))
            .fills_height()
            .gap(gap),
        )
    })
}

/// The past sessions of the agent the chat is with.
pub(super) fn sessions() -> impl View {
    aside("SESSIONS", crate::history::screen())
}

#[cfg(test)]
mod tests {
    use bevy::app::App;
    use bevy::ecs::hierarchy::ChildOf;
    use bevy::ecs::query::With;
    use hamlet_client::outbox::Outbox;

    use super::*;
    use crate::testing::{panel_app, press, words};

    fn hub_app() -> App {
        let mut app = panel_app();
        app.add_plugins((chat::plugin, crate::hub::plugin));
        app.init_resource::<crate::status::Bar>();
        app
    }

    #[test]
    fn the_header_shows_received_work_and_stops_only_a_busy_agent() {
        let mut app = hub_app();
        let mut state = hamlet_testing::realm::state();
        state.agents[0].busy = true;
        state.agents[0].activity.doing_words =
            "Reading the received orchard ledger".into();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            living(),
        );
        app.update();
        assert!(words(&mut app).iter().any(
            |text| text == "Reading the received orchard ledger"
        ));
        press(&mut app, "STOP");
        let stopped =
            app.world().resource::<Outbox>().iter().any(|message| {
                let protocol::ClientMessage::Stop(stop) = message
                else {
                    return false;
                };
                stop.agent == Some(protocol::AgentId(1))
            });
        assert!(stopped);
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(
                hamlet_testing::realm::state(),
            ),
        );
        app.update();
        let world = app.world_mut();
        let stop = world
            .query::<(Entity, &bevy::ui::widget::Text)>()
            .iter(world)
            .find(|(_, text)| text.0 == "STOP")
            .unwrap()
            .0;
        let mut at = stop;
        let mut hidden = false;
        loop {
            hidden |= world
                .get::<bevy::ui::Node>(at)
                .is_some_and(|node| node.display == Display::None);
            let Some(parent) = world.get::<ChildOf>(at) else {
                break;
            };
            at = parent.parent();
        }
        assert!(
            hidden,
            "STOP is hidden while the received agent is at rest"
        );
    }

    /// The Hub on `ada@home`, with her details open.
    fn ada_app() -> App {
        let mut app = hub_app();
        let ada = crate::testing::seat_ada(&mut app);
        app.insert_resource(hamlet_realm::pick::Selected(Some(
            Thing::Agent(ada),
        )))
        .insert_resource(Showing::Thread)
        .insert_resource(Sheet::Details);
        app.update();
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            crate::hub::screen(),
        );
        app.update();
        app
    }

    fn request(app: &App, what: &str) -> serde_json::Value {
        app.world()
            .resource::<Outbox>()
            .iter()
            .map(|message| serde_json::to_value(message).unwrap())
            .filter(|message| message["what"] == what)
            .last()
            .unwrap_or_else(|| panic!("no read {what}"))
    }

    fn has_span(app: &mut App, text: &str) -> bool {
        app.world_mut()
            .query::<&bevy::text::TextSpan>()
            .iter(app.world())
            .any(|span| span.0 == text)
    }

    fn receive(app: &mut App, message: serde_json::Value) {
        hamlet_client::receive(
            app.world_mut(),
            serde_json::from_value(message).unwrap(),
        );
        app.update();
    }

    #[test]
    fn papers_of_the_agent_and_only_its_finished_work_refresh_the_sheet()
     {
        use serde_json::json;
        let mut app = ada_app();
        let read = request(&app, "papers");
        assert_eq!(read["of"], json!({"agent": 2}));
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {
                "memory": [], "learned": [], "character": "", "skills": []
            }}),
        );
        let shown = words(&mut app);
        assert!(
            shown.iter().any(|text| text == "No memory files yet.")
        );
        assert!(shown.iter().any(|text| text
            == "Write here how this agent behaves and speaks."));
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        app.world_mut().trigger(
            hamlet_client::messages::JobFinished {
                chat: Entity::PLACEHOLDER,
            },
        );
        assert!(
            app.world().resource::<Outbox>().iter().next().is_none()
        );
        let chat = app.world().resource::<Convo>().session;
        app.world_mut()
            .trigger(hamlet_client::messages::JobFinished { chat });
        app.update();
        let read = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {
                "memory": [{"path": "notes.md", "description": "", "text": "Received notes"}], "learned": ["notes.md"], "character": "Received character", "skills": [{"name": "orchard", "description": "Tends trees"}]
            }}),
        );
        assert!(has_span(&mut app, "Received character"));
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text.contains("notes.md (learned)"))
        );
        press(&mut app, "notes.md (learned)");
        app.update();
        assert!(has_span(&mut app, "Received notes"));
        bevy_fynix::mount::<hamlet_ui::theme::HamletTheme>(
            app.world_mut(),
            crate::hub::skills_screen(),
        );
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "orchard"));
        assert!(shown.iter().any(|text| text == "Tends trees"));
        press(&mut app, "REFLECT NOW");
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "say")
            .unwrap();
        assert_eq!(sent["to"], json!(2));
        assert_eq!(sent["text"], "/reflect");
    }

    #[test]
    fn a_prompt_reply_or_failure_is_shown_on_its_page() {
        use serde_json::json;
        let mut app = ada_app();
        show_prompt(app.world_mut());
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Prompt
        );
        let read = request(&app, "prompt");
        assert_eq!(read["agent"], json!(2));
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "prompt": "The received prompt"}),
        );
        assert_eq!(
            app.world().resource::<Briefing>().text,
            "The received prompt"
        );
        show_prompt(app.world_mut());
        let read = request(&app, "prompt");
        receive(
            &mut app,
            json!({"type": "reply", "to": read["id"], "kind": "error", "text": "No such agent"}),
        );
        assert_eq!(
            app.world().resource::<Briefing>().text,
            "The prompt could not be read: No such agent"
        );
        assert!(
            app.world_mut()
                .query::<&bevy::text::TextSpan>()
                .iter(app.world())
                .any(|text| text.0
                    == "The prompt could not be read: No such agent")
        );
    }

    #[test]
    fn edits_of_the_character_and_of_a_memory_file_return_to_where_they_began()
     {
        use bevy::ecs::observer::On;
        use bevy::ecs::resource::Resource;
        use bevy::ecs::system::ResMut;
        use hamlet_desktop::editor::{Edit, Edited};
        use serde_json::json;
        #[derive(Resource, Default)]
        struct Edits(Vec<Edit>);
        let mut app = ada_app();
        app.init_resource::<Edits>().add_observer(
            |edit: On<Edit>, mut edits: ResMut<Edits>| {
                edits.0.push(edit.event().clone());
            },
        );
        let papers = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Pears"}], "learned": [], "character": "", "skills": []}}),
        );
        press(&mut app, "EDIT");
        let read = request(&app, "papers");
        assert_eq!(read["full"], true);
        let text = "character ".repeat(1000);
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"memory": [], "learned": [], "character": text, "skills": []}}),
        );
        let edit =
            app.world().resource::<Edits>().0.last().unwrap().clone();
        assert_eq!(edit.text, text);
        app.world_mut().trigger(Edited {
            key: edit.key.clone(),
            text: "Saved character".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "edit")
            .unwrap();
        assert_eq!(sent["doc"], "character");
        assert_eq!(sent["scholar"], json!(2));
        assert_eq!(sent["text"], "Saved character");
        receive(
            &mut app,
            json!({"type": "reply", "to": sent["id"], "kind": "error", "text": "Character refused"}),
        );
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "Character refused")
        );
        // A memory file of the agent's lodge is edited the same way,
        // and removed by sending no text.
        super::super::papers::edit_memory(
            app.world_mut(),
            "farm/pears.md".into(),
        );
        let read = request(&app, "papers");
        receive(
            &mut app,
            json!({"type": "read", "to": read["id"], "papers": {"memory": [{"path": "farm/pears.md", "description": "", "text": "Whole pears"}], "learned": [], "character": "", "skills": []}}),
        );
        let edit =
            app.world().resource::<Edits>().0.last().unwrap().clone();
        assert_eq!(edit.text, "Whole pears");
        app.world_mut().trigger(Edited {
            key: edit.key,
            text: "Ripe pears".into(),
        });
        app.update();
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["doc"] == "memory")
            .last()
            .unwrap();
        assert_eq!(sent["of"], json!({"agent": 2}));
        assert_eq!(sent["path"], "farm/pears.md");
        assert_eq!(sent["text"], "Ripe pears");
        super::super::papers::remove_memory(
            app.world_mut(),
            "farm/pears.md".into(),
        );
        let removal = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .filter(|m| m["doc"] == "memory")
            .last()
            .unwrap();
        assert_eq!(removal["text"], "");
    }

    fn select_scout(app: &mut App) {
        use hamlet_client::chats::Chat;
        let mut state = hamlet_testing::realm::state();
        state.agents.push(hamlet_testing::realm::agent(
            3, "home", 1, "scout",
        ));
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        let agent = hamlet_client::realm::named(
            app.world_mut(),
            "scout@home",
        )
        .unwrap();
        let to = protocol::AgentId(3);
        let session = app
            .world_mut()
            .spawn(Chat(protocol::ChatRef {
                kind: None,
                from: None,
                with: to,
                past: None,
            }))
            .id();
        let mut convo = app.world_mut().resource_mut::<Convo>();
        convo.agent = agent;
        convo.session = session;
        convo.with = Some("scout@home".into());
        app.update();
    }

    #[test]
    fn late_reads_cannot_fill_the_next_agents_pages() {
        use serde_json::json;
        let mut app = ada_app();
        let papers = request(&app, "papers");
        show_prompt(app.world_mut());
        let prompt = request(&app, "prompt");
        select_scout(&mut app);
        let scout = request(&app, "papers");
        assert_eq!(scout["of"], json!({"agent": 3}));
        receive(
            &mut app,
            json!({"type": "read", "to": scout["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Scout notes"}], "learned": [], "character": "Scout character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": papers["id"], "papers": {"memory": [{"path": "notes.md", "description": "", "text": "Wrong notes"}], "learned": [], "character": "Wrong character", "skills": []}}),
        );
        receive(
            &mut app,
            json!({"type": "read", "to": prompt["id"], "prompt": "Wrong prompt"}),
        );
        // The prompt page is left for the thread of the next agent.
        assert_eq!(
            *app.world().resource::<Showing>(),
            Showing::Thread
        );
        assert!(has_span(&mut app, "Scout character"));
        assert!(!has_span(&mut app, "Wrong character"));
        press(&mut app, "notes.md");
        app.update();
        assert!(has_span(&mut app, "Scout notes"));
        assert!(!has_span(&mut app, "Wrong notes"));
        assert_ne!(
            app.world().resource::<Briefing>().text,
            "Wrong prompt"
        );
    }

    #[test]
    fn letters_and_questions_are_in_the_session_menu_and_watched_read_only()
     {
        use hamlet_client::chats::Chat;
        use serde_json::json;
        let mut app = ada_app();
        // The chat is under the details, so they are shut.
        app.insert_resource(Sheet::Chat);
        let mut state = hamlet_testing::realm::state();
        state.agents.push(hamlet_testing::realm::agent(
            3, "home", 1, "scout",
        ));
        state.agents[1].conversations =
            serde_json::from_value(json!([
                {"with":3,"kind":"letters","count":2,"last":1000},
                {"with":3,"kind":"questions","count":1,"last":2000}
            ]))
            .unwrap();
        receive(
            &mut app,
            serde_json::to_value(protocol::ServerMessage::State(
                state,
            ))
            .unwrap(),
        );
        let ada = app.world().resource::<Convo>().agent;
        let (items, now) = entries(app.world());
        let names = items
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "Session",
                "Letters with scout@home",
                "Questions with scout@home"
            ]
        );
        assert_eq!(now, 0);
        for (kind, (_, go)) in
            ["letters", "questions"].iter().zip(&items[1..])
        {
            go_to(&mut app, go);
            let session = app.world().resource::<Convo>().session;
            let chat =
                app.world().get::<Chat>(session).unwrap().0.clone();
            assert_eq!(
                serde_json::to_value(&chat).unwrap(),
                json!({"with":2, "from":3, "kind":kind})
            );
            assert!(app.world().resource::<Outbox>().iter().any(|m| matches!(m, protocol::ClientMessage::Open(open) if open.chat == chat)));
            receive(
                &mut app,
                json!({"type":"transcript","chat":chat,"items":[
                    {"id":"1:0","at":1,"kind":"user","text":"Question","author":"scout@home"},
                    {"id":"2:0","at":2,"kind":"assistant","text":"Answer"}
                ], "earlier":"1:0","last":null,"working":true,"pending":0,"usage":{
                    "input":0,"output":0,"cached":0,"calls":0,"lastInput":0,"reasoning":0,"unreported":0,"summary":"","micros":0,"ms":0
                }}),
            );
            let world = app.world_mut();
            assert_eq!(
                world
                    .query_filtered::<Entity, With<chat::ChatField>>()
                    .iter(world)
                    .count(),
                0
            );
            assert_eq!(
                world.query::<&chat::Rewinds>().iter(world).count(),
                0
            );
            assert!(words(&mut app).contains(&"READ ONLY".into()));
            // The menu shows which of them is open.
            assert_ne!(entries(app.world()).1, 0);
        }
        // The first of the menu leads back to the chief's chat.
        go_to(&mut app, &items[0].1);
        let session = app.world().resource::<Convo>().session;
        assert_eq!(app.world().resource::<Convo>().agent, ada);
        assert!(
            app.world()
                .get::<Chat>(session)
                .unwrap()
                .0
                .from
                .is_none()
        );
    }

    fn go_to(app: &mut App, to: &Go) {
        go(app.world_mut(), to);
        app.update();
    }
}
