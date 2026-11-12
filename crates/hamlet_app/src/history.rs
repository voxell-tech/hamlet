//! The Hub's sessions page: the selected agent's past sessions.

use bevy::app::{App, PreUpdate, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::{MotionTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, button, column, frame,
    label, menu_item, row, text_field,
};
use bevy_fynix::{
    ScopedExt, StateExt, ViewExt, derived, each, every_frame, keyed,
    resource, style,
};
use chrono::Datelike;
use hamlet_client::chats::{Chat, TalksTo, Transcript};
use hamlet_client::messages::{ChiefMessage, JobFinished, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::id_of;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{
    Fills, ICON, buttons, command, filling, good_action, hairline,
    hidden, link_row, note, scroll_area, section, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::cut;
use hamlet_ui::widgets::hint::search;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, built, with_cx};
use protocol::Past;

use crate::age::Age;
use crate::convo::Convo;
use crate::dock::{HUB, show_tab};
use crate::hub::{self, Showing};
use crate::{PanelSystems, chat};

mod import;

/// Characters of a title a row shows.
const TITLE_SHOWN: usize = 90;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(import::plugin)
        .init_resource::<History>()
        .init_resource::<Requests>()
        .add_observer(response)
        .add_observer(stale_on_job)
        .add_systems(Update, refresh.run_if(hub::reads_sessions))
        .add_systems(PreUpdate, keep_open.in_set(PanelSystems::Bind));
}

/// The session open in the reader.
pub struct Reading {
    pub number: i64,
    /// The entity with its [`Transcript`].
    pub holder: Entity,
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
    /// The heading of the day it begins, on the first row of a day.
    heading: Option<&'static str>,
}

/// The day a session was last used, as the list groups it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Day {
    Today,
    Yesterday,
    PastWeek,
    Earlier,
}

impl Day {
    /// The day of `then` at `now`, both in milliseconds since 1970.
    fn of(now: i64, then: i64) -> Self {
        match (local(now).date_naive() - local(then).date_naive())
            .num_days()
        {
            ..=0 => Self::Today,
            1 => Self::Yesterday,
            2..7 => Self::PastWeek,
            _ => Self::Earlier,
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Yesterday => "Yesterday",
            Self::PastWeek => "Past week",
            Self::Earlier => "Earlier",
        }
    }
}

/// `ms` since 1970 in the local time zone.
fn local(ms: i64) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap_or_default()
        .with_timezone(&chrono::Local)
}

#[derive(Resource, Default)]
struct Requests {
    agent: Option<protocol::AgentId>,
    pending: Option<RequestId>,
    open: bool,
}

/// How long a past session is and who began it, when that was not
/// `chief`.
fn counts(past: &Past, chief: &str) -> Vec<String> {
    let mut parts = vec![match past.messages {
        1 => "1 message".to_string(),
        n => format!("{n} messages"),
    }];
    if past.tokens > 0 {
        parts.push(format_tokens(past.tokens));
    }
    if !past.by.is_empty() && past.by != chief {
        parts.push(format!("begun by {}", past.by));
    }
    parts
}

/// The detail line of a past session's row: when it was last used,
/// in the words its day's heading leaves to say, then its
/// [`counts`].
fn detail(past: &Past, chief: &str) -> String {
    let now = crate::status::now_ms() as i64;
    let at = local(past.modified);
    let when = match Day::of(now, past.modified) {
        _ if past.current => "now".to_string(),
        Day::Today => {
            Age::of(now as u64, past.modified.max(0) as u64).ago()
        }
        Day::Yesterday => at.format("%H:%M").to_string(),
        Day::PastWeek => at.format("%a %H:%M").to_string(),
        Day::Earlier => dated(at, now),
    };
    let mut parts = vec![when];
    parts.extend(counts(past, chief));
    parts.join(" · ")
}

/// The date of `at`, with its year when that is not the year of
/// `now`.
fn dated(at: chrono::DateTime<chrono::Local>, now: i64) -> String {
    match at.year() == local(now).year() {
        true => at.format("%-d %b").to_string(),
        false => at.format("%-d %b %Y").to_string(),
    }
}

/// When `past` was last used, in full: "Today, 18:00", or its date
/// for an earlier day.
fn last_used(past: &Past) -> String {
    let now = crate::status::now_ms() as i64;
    let at = local(past.modified);
    let day = match Day::of(now, past.modified) {
        day @ (Day::Today | Day::Yesterday) => {
            day.heading().to_string()
        }
        Day::PastWeek | Day::Earlier => dated(at, now),
    };
    format!("{day}, {}", at.format("%H:%M"))
}

impl History {
    /// The sessions that hold every word searched for.
    fn found(&self) -> impl Iterator<Item = &Past> {
        let query = self.query.to_lowercase();
        self.pasts.iter().filter(move |past| {
            let words =
                format!("{} {}", past.title, past.by).to_lowercase();
            query.split_whitespace().all(|word| words.contains(word))
        })
    }

    /// The rows found, by the day they were last used, the one in
    /// use among today's.
    fn lines(&self, chief: &str) -> Vec<Line> {
        let now = crate::status::now_ms() as i64;
        let mut found = self
            .found()
            .map(|past| match past.current {
                true => (Day::Today, past),
                false => (Day::of(now, past.modified), past),
            })
            .collect::<Vec<_>>();
        found.sort_by_key(|(day, _)| *day);
        let mut was = None;
        found
            .into_iter()
            .map(|(day, past)| Line {
                number: past.number,
                title: cut(&past.title, TITLE_SHOWN),
                detail: detail(past, chief),
                live: past.current,
                heading: (was.replace(day) != Some(day))
                    .then(|| day.heading()),
            })
            .collect()
    }

    /// The count line over the list.
    fn count(&self) -> String {
        let all = self.pasts.len();
        let found = self.found().count();
        match (all, self.query.trim().is_empty()) {
            (0, _) => "No sessions yet.".to_string(),
            (1, true) => "1 session".to_string(),
            (_, true) => format!("{all} sessions"),
            (_, false) => format!("{found} of {all} have that"),
        }
    }

    pub(crate) fn read(&self) -> Option<&Past> {
        let reading = self.reading.as_ref()?;
        self.pasts.iter().find(|past| past.number == reading.number)
    }
}

/// Closes the reader.
pub(crate) fn close(world: &mut World) {
    let mut history = world.resource_mut::<History>();
    if let Some(reading) = history.reading.take() {
        world.despawn(reading.holder);
        chat::read_past(world, None);
    }
}

/// Opens the session `number`, whose transcript `holder` holds, in
/// the reader.
pub fn read(world: &mut World, number: i64, holder: Entity) {
    let mut history = world.resource_mut::<History>();
    history.failed = None;
    history.reading = Some(Reading { number, holder });
    chat::read_past(world, Some(holder));
}

fn format_tokens(n: u64) -> String {
    match n {
        0..=999 => format!("{n} tokens"),
        1000..=999_999 => format!("{:.1}k tokens", n as f64 / 1e3),
        _ => format!("{:.2}M tokens", n as f64 / 1e6),
    }
}

/// The numbered conversation opened as a read-only watched chat.
pub(crate) fn open(world: &mut World, number: i64) {
    close(world);
    world.resource_mut::<import::Importing>().0 = false;
    let Some(with) = id_of(world, world.resource::<Convo>().agent)
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
    read(world, number, holder);
}

/// The page of conversations of other programs to bring in.
pub(crate) fn import(world: &mut World) {
    close(world);
    import::open(world);
}

/// The reader's conversation resumed by its stable number.
fn resume(world: &mut World) {
    let number =
        world.resource::<History>().read().map(|past| past.number);
    let Some(number) = number else {
        return;
    };
    let Some(to) = id_of(world, world.resource::<Convo>().agent)
    else {
        return;
    };
    close(world);
    world.trigger(ChiefMessage::words(
        to,
        format!("/resume {number}"),
    ));
    hub::show(world, Showing::Thread);
    show_tab(world, HUB);
}

/// The server's history requested while the page is open.
pub(crate) fn refresh(world: &mut World) {
    let convo = world.resource::<Convo>();
    let agent = id_of(world, convo.agent);
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
            agent,
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
                != id_of(world, world.resource::<Convo>().agent)
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
        let convo = world.resource::<Convo>();
        if chat == convo.session
            || world
                .get::<TalksTo>(chat)
                .is_some_and(|to| to.0 == convo.agent)
        {
            world.resource_mut::<History>().fresh = false;
        }
    });
}

fn keep_open(showing: Res<Showing>, mut requests: ResMut<Requests>) {
    if !hub::reads_sessions(showing) && requests.open {
        requests.open = false;
    }
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
    let (title, detail, live) =
        (line.title.clone(), line.detail.clone(), line.live);
    let tone = Tone::Body;
    let pressed = with_cx(move |cx| {
        let space = &cx.theme().space;
        let name = label(title.clone()).rules(move |cx: &mut Cx| {
            style::<HamletTheme>()
                .label(move |l, t| {
                    l.size(t.text.heading).bold(true).tone(tone)
                })
                .apply(cx);
        });
        let mut meta = vec![note(detail.clone()).boxed()];
        if live {
            // The session open now: a green check, then the words.
            meta.push(
                row((
                    glyph(Glyph::Check, 12.0).tone(Tone::Success),
                    note("In use"),
                ))
                .gap(4.0)
                .align(AlignItems::Center)
                .boxed(),
            );
        }
        button(
            row((
                column((
                    name,
                    row(meta)
                        .gap(space.gap)
                        .align(AlignItems::Center),
                ))
                .gap(space.gap / 2.0)
                .grow(1.0)
                .min_width(px(0.0)),
                glyph(Glyph::Into, ICON).tone(Tone::Dim),
            ))
            .gap(space.gap)
            .align(AlignItems::Center)
            .fills_width(),
        )
        .width(percent(100.0))
        .padding(UiRect::axes(
            px(space.card_padding_x),
            px(space.card_padding_y),
        ))
        .rules(link_row)
        .tagged(Variant::Default)
        .on_activate(move |world| open(world, number))
        .appear(hidden)
        .transition(HamletTheme::expand)
        .context_menu(move || choices(&menu))
    });
    let heading = line.heading;
    with_cx(move |cx| {
        let space = &cx.theme().space;
        // The first row of a day under its heading, the others under
        // a line.
        let over = match heading {
            Some(day) => column((section(day),))
                .padding(UiRect::vertical(px(space.gap)))
                .boxed(),
            None => hairline(cx.theme().palette.gold_6).boxed(),
        };
        column((over, pressed)).gap(0.0)
    })
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
    with_cx(|cx| {
        let space = &cx.theme().space;
        column((
            row((
                search(
                    text_field(
                        resource::<History, _>(|h| h.query.clone()),
                        |world, text| {
                            world.resource_mut::<History>().query =
                                text;
                        },
                    )
                    .rules(text_input),
                    "Search sessions",
                ),
                note(resource::<History, _>(History::count)),
                command("Import", import::open),
            ))
            .gap(space.section)
            .align(AlignItems::Center),
            failed(),
            scroll_area((each(
                derived(|world: &World| {
                    world
                        .resource::<History>()
                        .lines(&world.resource::<Convo>().name)
                })
                .when(every_frame()),
                |line: &Line| {
                    (
                        line.number,
                        line.title.clone(),
                        line.detail.clone(),
                        line.heading,
                    )
                },
                line_view,
            )
            .presence(HamletTheme::expand)
            .within(column(()).gap(0.0)),)),
        ))
        .fills_height()
        .gap(space.section)
    })
}

/// Buttons for the session being read.
fn actions() -> AnyView {
    buttons(vec![good_action(Glyph::Play, "Resume", resume)]).boxed()
}

/// The reader of the open session.
fn reader() -> impl View {
    built(|cx| {
        let history = cx.world.resource::<History>();
        if history.reading.is_none() {
            return cx.build(frame());
        }
        let past = history.read().cloned();
        let chief = &cx.world.resource::<Convo>().name;
        // Its title heads the page.
        let detail = past
            .map(|past| {
                let mut parts = vec![last_used(&past)];
                parts.extend(counts(&past, chief));
                parts.join(" · ")
            })
            .unwrap_or_default();
        let theme = cx.theme();
        let section = theme.space.section;
        let line = theme.palette.gold_6;
        cx.build(
            column((
                note(format!("Read only · {detail}")),
                actions(),
                hairline(line),
                scroll_area((
                    keyed(
                        resource::<chat::PastLines, _>(
                            chat::PastLines::has_earlier,
                        ),
                        |earlier| match earlier {
                            true => {
                                command("Earlier", chat::read_earlier)
                                    .boxed()
                            }
                            false => frame().boxed(),
                        },
                    )
                    .within(column(()).align(AlignItems::FlexStart)),
                    chat::past(),
                )),
            ))
            .fills_height()
            .gap(section),
        )
    })
}

/// The sessions page of the agent the chat is with.
pub fn screen() -> AnyView {
    keyed(
        resource::<Convo, _>(|m| (m.session, m.with.clone())),
        |_| {
            keyed(
                derived(|world: &World| {
                    (
                        world
                            .resource::<History>()
                            .reading
                            .as_ref()
                            .map(|r| r.number),
                        world.resource::<import::Importing>().0,
                    )
                })
                .when(every_frame()),
                |(reading, importing)| match (reading, importing) {
                    (Some(_), _) => reader().boxed(),
                    (None, true) => import::screen(),
                    (None, false) => list().boxed(),
                },
            )
            .within(filling())
            .boxed()
        },
    )
    .within(filling())
    .boxed()
}

#[cfg(test)]
mod tests {
    use hamlet_client::chats::Chat;
    use hamlet_client::outbox::Outbox;
    use hamlet_ui::theme::HamletTheme;
    use serde_json::json;

    use super::*;
    use crate::testing::{panel_app, press, words};

    #[test]
    fn history_reads_received_rows_searches_and_opens_a_numbered_chat()
     {
        let mut app = panel_app();
        app.add_plugins((hub::plugin, chat::plugin, plugin));
        *app.world_mut().resource_mut::<Showing>() =
            Showing::Sessions;
        app.update();
        let read = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["what"] == "history")
            .expect("read history when the tab opens");
        assert_eq!(read["agent"], json!(1));
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), screen());
        app.update();
        let now = chrono::Utc::now().timestamp_millis();
        hamlet_client::receive(app.world_mut(), serde_json::from_value(json!({"type": "read", "to": read["id"], "history": [
            {"number": 2, "current": true, "title": "Plant pears", "by": "alice", "began": now, "modified": now, "messages": 3, "tokens": 1200},
            {"number": 1, "current": false, "title": "Count hives", "by": "bob", "began": now - 300000, "modified": now - 300000, "messages": 2, "tokens": 800}
        ]})).unwrap());
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text
            == "now · 3 messages · 1.2k tokens · begun by alice"));
        assert!(shown.iter().any(|text| text
            == "5 min ago · 2 messages · 800 tokens · begun by bob"));
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
        assert_eq!(chat.with, protocol::AgentId(1));
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
                .any(|text| text.0.trim_end() == "Seven hives.")
        );

        assert!(
            !words(&mut app)
                .iter()
                .any(|text| text.contains("Delete"))
        );
        press(&mut app, "Resume");
        let sent = app
            .world()
            .resource::<Outbox>()
            .iter()
            .map(|m| serde_json::to_value(m).unwrap())
            .find(|m| m["type"] == "say")
            .unwrap();
        assert_eq!(sent["text"], "/resume 1");
        assert_eq!(sent["to"], json!(1));
        assert!(app.world().get_entity(holder).is_err());
    }

    #[test]
    fn opening_the_tab_and_finishing_its_chat_request_fresh_history()
    {
        let mut app = panel_app();
        app.add_plugins((hub::plugin, plugin));
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
        // The thread lists the sessions too, for its menu.
        *app.world_mut().resource_mut::<Showing>() = Showing::Thread;
        app.update();
        assert_eq!(reads(&app), 1);
        *app.world_mut().resource_mut::<Showing>() =
            Showing::Sessions;
        app.update();
        assert_eq!(reads(&app), 1);
        app.update();
        assert_eq!(reads(&app), 1);
        app.world_mut().trigger(JobFinished {
            chat: Entity::PLACEHOLDER,
        });
        app.update();
        assert_eq!(reads(&app), 1);
        let chat = app.world().resource::<Convo>().session;
        app.world_mut().trigger(JobFinished { chat });
        app.update();
        assert_eq!(reads(&app), 2);
        *app.world_mut().resource_mut::<Showing>() = Showing::Realm;
        app.update();
        *app.world_mut().resource_mut::<Showing>() =
            Showing::Sessions;
        app.update();
        assert_eq!(reads(&app), 3);
    }
}
