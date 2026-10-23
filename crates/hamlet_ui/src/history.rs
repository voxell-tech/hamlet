//! The History page of the Inspector: the selected agent's
//! sessions.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::Changed;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_equals;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, button, column, frame,
    label, menu_item, scroll, text_field,
};
use bevy_fynix::{
    ScopedExt, StateExt, ViewExt, derived, each, every_frame, keyed,
    resource, style,
};
use hamlet_client::chats::{Chat, Entry, TalksTo, Transcript};
use hamlet_client::messages::{ChiefMessage, JobFinished, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::address_of;
use hamlet_protocol::client as protocol;
use protocol::Past;

use crate::dock::show_tab;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    Fills, away, back, buttons, command, filling, good_action,
    hairline, note, well,
};
use crate::me::Me;
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, PanelSystems, View, chat, inspector};

/// Characters of a title a row shows.
const TITLE_SHOWN: usize = 90;

/// Messages of a session built at a time.
pub const PAGE: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<History>()
        .init_resource::<Requests>()
        .add_observer(response)
        .add_observer(stale_on_job)
        .add_systems(
            Update,
            refresh
                .run_if(resource_equals(inspector::Section::History)),
        )
        .add_systems(
            PreUpdate,
            keep_reader.in_set(PanelSystems::Bind),
        );
}

/// The session open in the reader.
pub struct Reading {
    pub number: i64,
    /// The entity with its [`Transcript`].
    pub holder: Entity,
    /// Its messages, without the steps between them.
    pub said: Vec<String>,
    /// Messages built, counted from the end.
    pub reach: usize,
}

impl Reading {
    fn shown(&self) -> Vec<String> {
        let earlier = self.said.len().saturating_sub(self.reach);
        self.said[earlier..].to_vec()
    }
}

/// State of the History page.
#[derive(Resource, Default)]
pub struct History {
    pub pasts: Vec<Past>,
    pub query: String,
    pub reading: Option<Reading>,
    /// The error of the last action.
    pub failed: Option<String>,
    /// A history read is in flight or has answered.
    pub fresh: bool,
}

/// A row of the list.
#[derive(Clone, PartialEq)]
struct Line {
    number: i64,
    title: String,
    detail: String,
    live: bool,
}

#[derive(Resource, Default)]
struct Requests {
    agent: Option<protocol::Address>,
    pending: Option<RequestId>,
    open: bool,
}

fn since(t: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let s = now.saturating_sub(t).max(0) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

/// `title`, cut to what a row shows.
fn shortened(title: &str) -> String {
    let mut shown =
        title.chars().take(TITLE_SHOWN).collect::<String>();
    if title.chars().count() > TITLE_SHOWN {
        shown.push_str("...");
    }
    shown
}

/// The detail line of a past session's row.
fn detail(past: &Past) -> String {
    let count = match past.messages {
        1 => "1 message".to_string(),
        n => format!("{n} messages"),
    };
    let mut detail = if past.current {
        "In use now".to_string()
    } else {
        since(past.modified)
    };
    detail.push_str(&format!(", {count}"));
    if past.tokens > 0 {
        detail.push_str(&format!(", {}", format_tokens(past.tokens)));
    }
    if !past.by.is_empty() {
        detail.push_str(&format!(", begun by {}", past.by));
    }
    detail
}

impl History {
    fn lines(&self) -> Vec<Line> {
        self.pasts
            .iter()
            .filter(|past| {
                let words = format!("{} {}", past.title, past.by)
                    .to_lowercase();
                self.query
                    .to_lowercase()
                    .split_whitespace()
                    .all(|word| words.contains(word))
            })
            .map(|past| Line {
                number: past.number,
                title: shortened(&past.title),
                detail: detail(past),
                live: past.current,
            })
            .collect()
    }

    /// The count line over the list.
    fn count(&self) -> String {
        let all = self.pasts.len();
        let found = self.lines().len();
        match (all, self.query.trim().is_empty()) {
            (0, _) => "No sessions yet.".to_string(),
            (1, true) => "1 session".to_string(),
            (_, true) => format!("{all} sessions"),
            (_, false) => format!("{found} of {all} have that"),
        }
    }

    fn read(&self) -> Option<&Past> {
        let reading = self.reading.as_ref()?;
        self.pasts.iter().find(|past| past.number == reading.number)
    }
}

/// Closes the reader.
fn close(world: &mut World) {
    let mut history = world.resource_mut::<History>();
    if let Some(reading) = history.reading.take() {
        world.despawn(reading.holder);
    }
}

/// The messages of a session, without the steps between them.
pub fn said(transcript: &Transcript) -> Vec<String> {
    transcript
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item.entry,
                Entry::Chief { .. }
                    | Entry::Assistant(_)
                    | Entry::Agent { .. }
                    | Entry::Error(_)
            )
        })
        .map(|item| item.id.clone())
        .collect()
}

fn format_tokens(n: u64) -> String {
    match n {
        0..=999 => format!("{n} tok"),
        1000..=999_999 => format!("{:.1}k tok", n as f64 / 1e3),
        _ => format!("{:.2}M tok", n as f64 / 1e6),
    }
}

/// The numbered conversation opened as a read-only watched chat.
fn open(world: &mut World, number: i64) {
    close(world);
    let Some(with) = address_of(world, world.resource::<Me>().agent)
    else {
        return;
    };
    let holder = world
        .spawn((
            Chat(protocol::ChatRef {
                kind: None,
                from: None,
                with,
                past: Some(number),
            }),
            Transcript::default(),
        ))
        .id();
    let mut history = world.resource_mut::<History>();
    history.failed = None;
    history.reading = Some(Reading {
        number,
        holder,
        said: Vec::new(),
        reach: PAGE,
    });
}

/// The reader's conversation resumed by its stable number.
fn resume(world: &mut World) {
    let number =
        world.resource::<History>().read().map(|past| past.number);
    let Some(number) = number else {
        return;
    };
    let Some(to) = address_of(world, world.resource::<Me>().agent)
    else {
        return;
    };
    close(world);
    world.trigger(ChiefMessage {
        to,
        text: format!("/resume {number}"),
    });
    *world.resource_mut::<inspector::Section>() =
        inspector::Section::Conversation;
    show_tab(world, "agent");
}

/// The server's history requested while the page is open.
pub(crate) fn refresh(world: &mut World) {
    let me = world.resource::<Me>();
    let agent = address_of(world, me.agent);
    let moved = world.resource::<Requests>().agent != agent;
    if moved {
        close(world);
        let mut history = world.resource_mut::<History>();
        history.pasts.clear();
        history.failed = None;
        history.fresh = false;
    }
    let opened = !world.resource::<Requests>().open;
    if world.resource::<History>().fresh && !moved && !opened {
        return;
    }
    let Some(agent) = agent else {
        return;
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::History {
            id,
            agent: agent.clone(),
        })
    });
    let mut requests = world.resource_mut::<Requests>();
    requests.agent = Some(agent);
    requests.pending = Some(id);
    requests.open = true;
    let mut history = world.resource_mut::<History>();
    history.fresh = true;
}

fn response(reply: On<Response>, mut commands: Commands) {
    let reply = reply.event().clone();
    commands.queue(move |world: &mut World| {
        let requests = world.resource::<Requests>();
        if requests.pending.as_ref() != Some(&reply.to)
            || requests.agent
                != address_of(world, world.resource::<Me>().agent)
        {
            return;
        }
        world.resource_mut::<Requests>().pending = None;
        match reply.message {
            protocol::ServerMessage::Read(
                protocol::ReadResult::History { history, .. },
            ) => {
                world.resource_mut::<History>().pasts = history;
            }
            protocol::ServerMessage::Reply(reply)
                if reply.kind == protocol::ReplyKind::Error =>
            {
                world.resource_mut::<History>().failed =
                    Some(reply.text);
            }
            protocol::ServerMessage::Fault(fault) => {
                world.resource_mut::<History>().failed =
                    Some(fault.text);
            }
            _ => {}
        }
    });
}

fn stale_on_job(job: On<JobFinished>, mut commands: Commands) {
    let chat = job.chat;
    commands.queue(move |world: &mut World| {
        let me = world.resource::<Me>();
        if chat == me.session
            || world
                .get::<TalksTo>(chat)
                .is_some_and(|to| to.0 == me.agent)
        {
            world.resource_mut::<History>().fresh = false;
        }
    });
}

fn keep_reader(
    section: Res<inspector::Section>,
    mut requests: ResMut<Requests>,
    mut history: ResMut<History>,
    transcripts: Query<&Transcript, Changed<Transcript>>,
) {
    if *section != inspector::Section::History && requests.open {
        requests.open = false;
    }
    let Some(reading) = &history.reading else {
        return;
    };
    let Ok(transcript) = transcripts.get(reading.holder) else {
        return;
    };
    let now = said(transcript);
    if now != reading.said {
        history.reading.as_mut().unwrap().said = now;
    }
}

/// Look of a session's row.
fn card(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.card)
                .border(1.0)
                .border_color(t.palette.brass_6)
                .radius(t.space.round)
                .padding(UiRect::axes(px(14.0), px(10.0)))
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::FlexStart)
        })
        .hovered(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.card).border_color(t.palette.brass_4)
            })
        })
        .pressed(|s| {
            s.frame(|f, t| {
                f.fill(t.palette.card).border_color(t.palette.brass_5)
            })
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A session's context menu.
fn choices(line: &Line) -> Vec<AnyView> {
    let number = line.number;
    vec![
        menu_item(label("Read").wrap(false))
            .on_activate(move |world| open(world, number))
            .boxed(),
        menu_item(label("Resume").wrap(false))
            .on_activate(move |world| {
                open(world, number);
                resume(world);
            })
            .boxed(),
    ]
}

fn line_view(line: &Line) -> AnyView {
    let number = line.number;
    let menu = line.clone();
    button(
        column((
            label(line.title.clone()).size(13.0).bold(true).tone(
                if line.live { Tone::Accent } else { Tone::Body },
            ),
            note(line.detail.clone()),
        ))
        .gap(2.0)
        .fills_width(),
    )
    .width(percent(100.0))
    .rules(card)
    .tagged(Mood::Plain)
    .on_activate(move |world| open(world, number))
    .appear(away)
    .transition(Motion::Expand)
    .context_menu(move || choices(&menu))
    .boxed()
}

fn failed() -> impl View {
    keyed(resource::<History, _>(|h| h.failed.clone()), |failed| {
        match failed {
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => frame().boxed(),
        }
    })
    .within(column(()))
}

fn list() -> impl View {
    column((
        text_field(
            resource::<History, _>(|h| h.query.clone()),
            |world, text| {
                world.resource_mut::<History>().query = text;
            },
        )
        .rules(well),
        note(resource::<History, _>(History::count)),
        failed(),
        scroll((each(
            resource::<History, _>(History::lines),
            |line: &Line| {
                (line.number, line.title.clone(), line.detail.clone())
            },
            line_view,
        )
        .within(column(()).gap(8.0)),))
        .fills_height(),
    ))
    .fills_height()
    .gap(8.0)
}

/// Buttons for the session being read.
fn actions() -> AnyView {
    buttons(vec![
        back("BACK", close).boxed(),
        good_action(Glyph::Play, "RESUME", resume),
    ])
    .boxed()
}

/// The reader of the open session.
fn reader() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let history = cx.world.resource::<History>();
        let Some(reading) = &history.reading else {
            return cx.build(frame());
        };
        let holder = reading.holder;
        let past = history.read().cloned();
        let (title, detail) = match &past {
            Some(past) => (shortened(&past.title), detail(past)),
            None => Default::default(),
        };
        let hint = format!(
            "RESUME carries on from session {}.",
            reading.number
        );
        cx.build(
            column((
                label(title).size(13.0).bold(true),
                note(detail),
                actions(),
                note(hint),
                hairline(cx.theme().palette.brass_6),
                scroll((
                    keyed(
                        derived(|world: &World| {
                            world
                                .resource::<History>()
                                .reading
                                .as_ref()
                                .is_some_and(|r| {
                                    r.said.len() > r.reach
                                        || world
                                            .get::<Transcript>(
                                                r.holder,
                                            )
                                            .is_some_and(|t| {
                                                t.earlier().is_some()
                                            })
                                })
                        })
                        .when(every_frame()),
                        |earlier| {
                            if !*earlier {
                                return frame().boxed();
                            }
                            command("EARLIER", |world| {
                                let mut history =
                                    world.resource_mut::<History>();
                                let Some(reading) =
                                    &mut history.reading
                                else {
                                    return;
                                };
                                let (holder, more) = (
                                    reading.holder,
                                    reading.said.len()
                                        <= reading.reach,
                                );
                                reading.reach += PAGE;
                                if more {
                                    hamlet_client::chats::earlier(
                                        world, holder,
                                    );
                                }
                            })
                            .boxed()
                        },
                    )
                    .within(column(()).align(AlignItems::FlexStart)),
                    each(
                        resource::<History, _>(|h| {
                            h.reading
                                .as_ref()
                                .map(Reading::shown)
                                .unwrap_or_default()
                        }),
                        |id: &String| id.clone(),
                        move |id| {
                            chat::entry_view(holder, id.clone())
                        },
                    )
                    .within(
                        column(()).gap(8.0).width(percent(100.0)),
                    ),
                ))
                .fills_height(),
            ))
            .fills_height()
            .gap(8.0),
        )
    })
}

/// The Archive page.
pub fn screen() -> AnyView {
    inspector::of_agent("ARCHIVE", || {
        keyed(
            resource::<History, _>(|h| {
                h.reading.as_ref().map(|r| r.number)
            }),
            |reading| match reading {
                Some(_) => reader(),
                None => list().boxed(),
            },
        )
        .within(filling())
        .boxed()
    })
    .boxed()
}

#[cfg(test)]
mod tests {
    use hamlet_client::chats::Chat;
    use hamlet_client::outbox::Outbox;
    use serde_json::json;

    use super::*;
    use crate::testing::{panel_app, press, words};

    #[test]
    fn history_reads_received_rows_searches_and_opens_a_numbered_chat()
     {
        let mut app = panel_app();
        app.add_plugins((inspector::plugin, plugin));
        *app.world_mut().resource_mut::<inspector::Section>() =
            inspector::Section::History;
        app.update();
        let read = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["what"] == "history")
            .expect("read history when the tab opens");
        assert_eq!(
            read["agent"],
            json!({"hamlet": "home", "agent": "founder"})
        );
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), screen());
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "No sessions yet.")
        );
        let now = chrono::Utc::now().timestamp_millis();
        hamlet_client::receive(app.world_mut(), serde_json::from_value(json!({"type": "read", "to": read["id"], "history": [
            {"number": 2, "current": true, "title": "Plant pears", "by": "alice", "began": now, "modified": now, "messages": 3, "tokens": 1200},
            {"number": 1, "current": false, "title": "Count hives", "by": "bob", "began": now - 300000, "modified": now - 300000, "messages": 2, "tokens": 800}
        ]})).unwrap());
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "2 sessions"));
        assert!(shown.iter().any(|text| text
            == "In use now, 3 messages, 1.2k tok, begun by alice"));
        assert!(shown.iter().any(|text| text
            == "5 min ago, 2 messages, 800 tok, begun by bob"));
        app.world_mut().resource_mut::<History>().query =
            "hives".into();
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "1 of 2 have that")
        );
        app.world_mut().resource_mut::<History>().query =
            "bob".into();
        app.update();
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text == "1 of 2 have that")
        );
        press(&mut app, "Count hives");
        app.update();
        let holder = app
            .world()
            .resource::<History>()
            .reading
            .as_ref()
            .unwrap()
            .holder;
        let chat = app
            .world()
            .get::<Chat>(holder)
            .expect("a watched past conversation")
            .0
            .clone();
        assert_eq!(chat.past, Some(1));
        assert_eq!(chat.with.agent, "founder");
        let open = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "open" && m["chat"]["past"] == 1)
            .unwrap();
        hamlet_client::receive(app.world_mut(), serde_json::from_value(json!({"type": "transcript", "to": open["id"], "chat": chat, "items": [{"id": "old:0", "at": now, "kind": "assistant", "text": "Seven hives."}], "earlier": null, "last": null, "working": false, "pending": 0, "usage": {"calls": 0, "unreported": 0, "input": 0, "cached": 0, "output": 0, "reasoning": 0, "ms": 0, "micros": 0, "lastInput": 0, "summary": ""}})).unwrap());
        app.update();
        assert!(
            app.world()
                .get::<Transcript>(holder)
                .unwrap()
                .item("old:0")
                .is_some()
        );
        assert!(
            app.world_mut()
                .query::<&bevy::text::TextSpan>()
                .iter(app.world())
                .any(|text| text.0 == "Seven hives.")
        );

        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text.contains("DELETE"))
        );
        press(&mut app, "RESUME");
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "say")
            .unwrap();
        assert_eq!(sent["text"], "/resume 1");
        assert_eq!(
            sent["to"],
            json!({"hamlet": "home", "agent": "founder"})
        );
        assert!(app.world().get_entity(holder).is_err());
    }

    #[test]
    fn opening_the_tab_and_finishing_its_chat_request_fresh_history()
    {
        let mut app = panel_app();
        app.add_plugins((inspector::plugin, plugin));
        app.update();
        let reads = |app: &App| {
            app.world()
                .resource::<Outbox>()
                .iter()
                .filter(|m| {
                    matches!(
                        m,
                        protocol::ClientMessage::Read(
                            protocol::Read::History { .. }
                        )
                    )
                })
                .count()
        };
        assert_eq!(reads(&app), 0);
        *app.world_mut().resource_mut::<inspector::Section>() =
            inspector::Section::History;
        app.update();
        assert_eq!(reads(&app), 1);
        app.update();
        assert_eq!(reads(&app), 1);
        app.world_mut().trigger(JobFinished {
            chat: Entity::PLACEHOLDER,
        });
        app.update();
        assert_eq!(reads(&app), 1);
        let chat = app.world().resource::<Me>().session;
        app.world_mut().trigger(JobFinished { chat });
        app.update();
        assert_eq!(reads(&app), 2);
        *app.world_mut().resource_mut::<inspector::Section>() =
            inspector::Section::Notes;
        app.update();
        *app.world_mut().resource_mut::<inspector::Section>() =
            inspector::Section::History;
        app.update();
        assert_eq!(reads(&app), 3);
    }
}
