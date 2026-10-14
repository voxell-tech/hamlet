//! The History page of the Inspector: the selected agent's
//! sessions.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, ContextMenuExt, FrameProps, button, column, frame,
    label, menu_item, row, scroll, text_field,
};
use bevy_fynix::{
    ScopedExt, StateExt, ViewExt, each, keyed, resource, style,
};
use hamlet_core::core::RefreshLists;
use hamlet_core::session::{
    self, Entry, JobFinished, Transcript, Usage,
};
use hamlet_model::log;
use hamlet_rules::budget::format_tokens;
use hamlet_rules::commands::RESUME;
use hamlet_rules::vote::BALLOTS;

use crate::dock::show_tab;
use crate::feel::Mood;
use crate::icons::Glyph;
use crate::looks::{
    Fills, away, back, bad_action, buttons, command, filling,
    good_action, hairline, note, well,
};
use crate::me::Me;
use crate::theme::{BRASS_4, BRASS_5, BRASS_6, CARD, Lamplight};
use crate::{Build, Ui, chat, inspector};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<History>()
        .add_observer(stale_on_refresh)
        .add_observer(stale_on_job)
        .add_systems(Update, refresh);
}

/// Characters of a title a row shows.
const TITLE_SHOWN: usize = 90;

/// Messages of a session built at a time.
pub const PAGE: usize = 40;

/// A session's log.
#[derive(Clone, PartialEq, Debug)]
pub struct Past {
    pub path: PathBuf,
    /// First line said in it.
    pub title: String,
    /// The user who said it.
    pub by: String,
    pub modified: SystemTime,
    /// The number it was archived under. `None` for a log in use.
    pub number: Option<String>,
    /// The chat shown can resume it.
    pub ours: bool,
    pub messages: usize,
    pub tokens: u64,
    /// Its text in lowercase, for the search.
    pub said: String,
}

/// The session open in the reader.
pub struct Reading {
    pub path: PathBuf,
    /// The entity with its [`Transcript`].
    pub holder: Entity,
    /// Its messages, without the steps between them.
    pub said: Vec<u64>,
    /// Messages built, counted from the end.
    pub reach: usize,
}

impl Reading {
    fn shown(&self) -> Vec<u64> {
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
    /// A delete of the session being read awaits a yes.
    pub doomed: bool,
    /// The error of the last action.
    pub failed: Option<String>,
    /// The chat session `pasts` were listed for.
    pub of: Option<Entity>,
    /// `pasts` match the disk.
    pub fresh: bool,
}

/// A row of the list.
#[derive(Clone, PartialEq)]
struct Line {
    path: PathBuf,
    title: String,
    detail: String,
    live: bool,
    ours: bool,
}

fn since(t: SystemTime) -> String {
    let s = t.elapsed().map(|d| d.as_secs()).unwrap_or(0);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86400),
    }
}

/// The digits between an archived log's name and `.jsonl`.
fn number(path: &Path) -> Option<String> {
    let stem = path.file_name()?.to_str()?.strip_suffix(".jsonl")?;
    let (_, number) = stem.rsplit_once('.')?;
    (!number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
        .then(|| number.to_string())
}

/// Summary of a log. `None` if nothing was said in it.
fn past(path: &Path, live: Option<&Path>) -> Option<Past> {
    let events = log::read_events(path).ok()?;
    // What was spent on a part since rewound was spent all the same.
    let tokens = Usage::from_log(&events).tokens();
    let events = log::standing(events);
    let mut title = None;
    let mut said = String::new();
    let mut messages = 0;
    for e in &events {
        let text = match e["type"].as_str() {
            // The hamlet's own prompts are not talk.
            Some("user") if e["user"] != "hamlet" => {
                let text = e["content"].as_str().unwrap_or_default();
                title.get_or_insert_with(|| {
                    let by = e["user"].as_str().unwrap_or_default();
                    (text.to_string(), by.to_string())
                });
                text
            }
            Some("assistant") => {
                e["message"]["content"].as_str().unwrap_or_default()
            }
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        messages += 1;
        said.push_str(&text.to_lowercase());
        said.push('\n');
    }
    let (title, by) = title?;
    let line = title.lines().find(|l| !l.trim().is_empty())?.trim();
    let mut shown =
        line.chars().take(TITLE_SHOWN).collect::<String>();
    if line.chars().count() > TITLE_SHOWN {
        shown.push_str("...");
    }
    let number = number(path);
    let ours = number.as_ref().is_some_and(|number| {
        live.is_some_and(|live| {
            live.with_extension(format!("{number}.jsonl")) == path
        })
    });
    Some(Past {
        path: path.to_path_buf(),
        title: shown,
        by,
        modified: std::fs::metadata(path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH),
        number,
        ours,
        messages,
        tokens,
        said,
    })
}

/// The sessions in `dir`, newest first. `live` is the log of the
/// chat shown.
fn pasts(dir: &Path, live: Option<&Path>) -> Vec<Past> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut pasts = entries
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(
                    // A ballot is not a session.
                    |name| {
                        name.ends_with(".jsonl")
                            && !name.starts_with(BALLOTS)
                    },
                )
        })
        .filter_map(|path| past(&path, live))
        .collect::<Vec<_>>();
    pasts.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| b.path.cmp(&a.path))
    });
    pasts
}

impl Past {
    /// Every word of `query` is in it.
    fn matches(&self, query: &str) -> bool {
        query.to_lowercase().split_whitespace().all(|word| {
            self.said.contains(word)
                || self.title.to_lowercase().contains(word)
        })
    }

    fn detail(&self) -> String {
        let count = match self.messages {
            1 => "1 message".to_string(),
            n => format!("{n} messages"),
        };
        let mut detail = match self.number {
            Some(_) => since(self.modified),
            None => "In use now".to_string(),
        };
        detail.push_str(&format!(", {count}"));
        if self.tokens > 0 {
            detail.push_str(&format!(
                ", {}",
                format_tokens(self.tokens)
            ));
        }
        if !self.by.is_empty() {
            detail.push_str(&format!(", begun by {}", self.by));
        }
        detail
    }
}

impl History {
    fn lines(&self) -> Vec<Line> {
        self.pasts
            .iter()
            .filter(|past| past.matches(&self.query))
            .map(|past| Line {
                path: past.path.clone(),
                title: past.title.clone(),
                detail: past.detail(),
                live: past.number.is_none(),
                ours: past.ours,
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
        self.pasts.iter().find(|past| past.path == reading.path)
    }
}

/// Closes the reader.
fn close(world: &mut World) {
    let mut history = world.resource_mut::<History>();
    history.doomed = false;
    if let Some(reading) = history.reading.take() {
        world.despawn(reading.holder);
    }
}

/// The messages of a session, without the steps between them.
pub fn said(transcript: &Transcript) -> Vec<u64> {
    transcript
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item.entry,
                Entry::User { .. }
                    | Entry::Assistant(_)
                    | Entry::Agent { .. }
                    | Entry::Error(_)
            )
        })
        .map(|item| item.id)
        .collect()
}

/// Opens the log at `path` in the reader.
fn open(world: &mut World, path: PathBuf) {
    close(world);
    let events = log::read_standing(&path).unwrap_or_default();
    let transcript = Transcript::from_log(&events);
    let said = said(&transcript);
    let holder = world.spawn(transcript).id();
    let mut history = world.resource_mut::<History>();
    history.failed = None;
    history.reading = Some(Reading {
        path,
        holder,
        said,
        reach: PAGE,
    });
}

/// Resumes the session being read.
fn resume(world: &mut World) {
    let number = world
        .resource::<History>()
        .read()
        .filter(|past| past.ours)
        .and_then(|past| past.number.clone());
    let Some(number) = number else {
        return;
    };
    close(world);
    chat::send(world, format!("{RESUME} {number}"));
    // The talk it goes back to is what is shown.
    *world.resource_mut::<inspector::Section>() =
        inspector::Section::Conversation;
    show_tab(world, "agent");
}

/// Deletes the log of the session being read.
fn delete(world: &mut World) {
    let past = world.resource::<History>().read().cloned();
    close(world);
    // A log in use belongs to an open conversation.
    let Some(past) = past.filter(|past| past.number.is_some()) else {
        return;
    };
    let failed = std::fs::remove_file(&past.path)
        .err()
        .map(|e| format!("Could not delete that session: {e}"));
    let mut history = world.resource_mut::<History>();
    history.failed = failed;
    history.fresh = false;
}

/// Keeps `pasts` in step with the disk while the page is open.
pub(crate) fn refresh(world: &mut World) {
    if *world.resource::<inspector::Section>()
        != inspector::Section::History
    {
        return;
    }
    let session = world.resource::<Me>().session;
    let history = world.resource::<History>();
    let moved = history.of != Some(session);
    if history.fresh && !moved {
        return;
    }
    if moved {
        close(world);
    }
    let dir = inspector::house(world).sessions();
    let key = world.resource::<Me>().key.clone();
    let live = session::log_of(world, &key);
    let pasts = pasts(&dir, live.as_deref());
    let gone = world
        .resource::<History>()
        .reading
        .as_ref()
        .is_some_and(|reading| !reading.path.exists());
    if gone {
        close(world);
    }
    let mut history = world.resource_mut::<History>();
    history.of = Some(session);
    history.fresh = true;
    if moved {
        history.failed = None;
    }
    if history.pasts != pasts {
        history.pasts = pasts;
    }
}

pub(crate) fn stale_on_refresh(
    _: On<RefreshLists>,
    mut history: ResMut<History>,
) {
    history.fresh = false;
}

pub(crate) fn stale_on_job(
    _: On<JobFinished>,
    mut history: ResMut<History>,
) {
    history.fresh = false;
}

/// Look of a session's row.
fn card(cx: &mut Build) {
    style::<Lamplight>()
        .frame(|f, _| {
            f.fill(CARD)
                .border(1.0)
                .border_color(BRASS_6)
                .radius(Lamplight::round())
                .padding(UiRect::axes(px(14.0), px(10.0)))
                .justify(JustifyContent::FlexStart)
                .align(AlignItems::FlexStart)
        })
        .hovered(|s| {
            s.frame(|f, _| f.fill(CARD).border_color(BRASS_4))
        })
        .pressed(|s| {
            s.frame(|f, _| f.fill(CARD).border_color(BRASS_5))
        })
        .transition(Motion::Interact)
        .apply(cx);
}

/// A row of a session's context menu.
fn choice(
    text: &'static str,
    path: &Path,
    act: fn(&mut World, PathBuf),
) -> Ui {
    let path = path.to_path_buf();
    menu_item(label(text).wrap(false))
        .on_activate(move |world| act(world, path.clone()))
        .boxed()
}

/// Rows of a session's context menu.
fn choices(line: &Line) -> Vec<Ui> {
    let mut choices = vec![choice("Read", &line.path, open)];
    if line.ours {
        choices.push(choice("Resume", &line.path, |world, path| {
            open(world, path);
            resume(world);
        }));
    }
    if !line.live {
        choices.push(choice(
            "Delete...",
            &line.path,
            |world, path| {
                // The reader asks before it deletes.
                open(world, path);
                world.resource_mut::<History>().doomed = true;
            },
        ));
    }
    choices
}

fn line_view(line: &Line) -> Ui {
    let path = line.path.clone();
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
    .on_activate(move |world| open(world, path.clone()))
    .appear(away)
    .transition(Motion::Expand)
    .context_menu(move || choices(&menu))
    .boxed()
}

fn failed() -> Ui {
    keyed(resource::<History, _>(|h| h.failed.clone()), |failed| {
        match failed {
            Some(why) => {
                label(why.clone()).tone(Tone::Critical).boxed()
            }
            None => frame().boxed(),
        }
    })
    .within(column(()))
    .boxed()
}

fn list() -> Ui {
    column((
        text_field(
            resource::<History, _>(|h| h.query.clone()),
            |world, text| {
                world.resource_mut::<History>().query = text
            },
        )
        .rules(well),
        note(resource::<History, _>(|h| {
            if h.query.trim().is_empty() && !h.pasts.is_empty() {
                format!(
                    "{}. Type above and press Enter to search them.",
                    h.count()
                )
            } else {
                h.count()
            }
        })),
        failed(),
        scroll((each(
            resource::<History, _>(History::lines),
            |line: &Line| {
                format!("{}|{}", line.path.display(), line.detail)
            },
            line_view,
        )
        .within(column(()).gap(8.0)),))
        .fills_height(),
    ))
    .fills_height()
    .gap(8.0)
    .boxed()
}

/// Buttons for the session being read.
fn actions(resumable: bool, kept: bool) -> Ui {
    keyed(resource::<History, _>(|h| h.doomed), move |doomed| {
        if *doomed {
            return column((
                label("Delete this session for good?")
                    .tone(Tone::Critical),
                row((
                    bad_action(
                        Glyph::Delete,
                        "DELETE FOR GOOD",
                        delete,
                    ),
                    command("KEEP IT", |world| {
                        world.resource_mut::<History>().doomed =
                            false;
                    }),
                ))
                .gap(8.0),
            ))
            .gap(6.0)
            .boxed();
        }
        let mut actions = vec![back("BACK", close)];
        if resumable {
            actions.push(good_action(Glyph::Play, "RESUME", resume));
        }
        if kept {
            actions.push(bad_action(
                Glyph::Delete,
                "DELETE",
                |world| {
                    world.resource_mut::<History>().doomed = true;
                },
            ));
        }
        buttons(actions)
    })
    .within(column(()))
    .boxed()
}

/// The reader of the open session.
fn reader() -> Ui {
    Ui::new(|cx: &mut Build| {
        let history = cx.world.resource::<History>();
        let Some(reading) = &history.reading else {
            return cx.build(frame());
        };
        let holder = reading.holder;
        let past = history.read().cloned();
        let (title, detail, resumable, kept) = match &past {
            Some(past) => (
                past.title.clone(),
                past.detail(),
                past.ours,
                past.number.is_some(),
            ),
            None => Default::default(),
        };
        let number =
            past.as_ref().and_then(|past| past.number.clone());
        let hint = match (number, resumable) {
            (None, _) => "This session is open now. NEW SESSION \
                          files it here."
                .to_string(),
            (Some(number), true) => format!(
                "RESUME carries on from session {number} and files \
                 the open one."
            ),
            (Some(_), false) => "This belongs to another \
                                 conversation. You can read it or \
                                 delete it."
                .to_string(),
        };
        cx.build(
            column((
                label(title).size(13.0).bold(true),
                note(detail),
                actions(resumable, kept),
                note(hint),
                hairline(BRASS_6),
                scroll((
                    keyed(
                        resource::<History, _>(|h| {
                            h.reading.as_ref().is_some_and(|r| {
                                r.said.len() > r.reach
                            })
                        }),
                        |earlier| {
                            if !*earlier {
                                return frame().boxed();
                            }
                            command("EARLIER", |world| {
                                if let Some(reading) = &mut world
                                    .resource_mut::<History>()
                                    .reading
                                {
                                    reading.reach += PAGE;
                                }
                            })
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
                        |id: &u64| *id,
                        move |id| chat::entry_view(holder, *id),
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
pub fn screen() -> Ui {
    inspector::of_agent("ARCHIVE", || {
        keyed(
            resource::<History, _>(|h| {
                h.reading.as_ref().map(|r| r.path.clone())
            }),
            |reading| match reading {
                Some(_) => reader(),
                None => list(),
            },
        )
        .within(filling())
        .boxed()
    })
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;
    use serde_json::json;

    use super::*;

    fn write(path: &Path, lines: &[serde_json::Value]) {
        let text = lines
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        std::fs::write(path, text).unwrap();
    }

    fn talk(first: &str, reply: &str) -> Vec<serde_json::Value> {
        vec![
            json!({"type": "user", "user": "alice", "content": first}),
            json!({"type": "assistant",
                "message": {"content": reply}}),
        ]
    }

    #[test]
    fn sessions_are_listed_by_what_was_said_and_searched() {
        let dir = Scratch::new("history-list");
        let live = dir.join("app_alice_main.jsonl");
        write(&live, &talk("Plant the pears\nby the wall", "Done."));
        let old = dir.join("app_alice_main.1700000000.jsonl");
        write(&old, &talk("Count the hives", "There are Seven."));
        let other =
            dir.join("hamlet_scout_agent_bee.1700000001.jsonl");
        write(&other, &talk("Where is the key?", "Under the mat."));
        // Neither a ballot nor an empty log is a session.
        write(&dir.join("vote_3.jsonl"), &talk("Vote", "Yes."));
        write(&dir.join("app_alice_agent_x.jsonl"), &[]);

        let pasts = pasts(&dir, Some(&live));
        assert_eq!(pasts.len(), 3);
        let of = |path: &Path| {
            pasts.iter().find(|past| past.path == path).unwrap()
        };
        assert_eq!(of(&live).title, "Plant the pears");
        assert_eq!(of(&live).number, None);
        assert!(!of(&live).ours);
        assert_eq!(of(&old).number.as_deref(), Some("1700000000"));
        assert!(of(&old).ours);
        assert_eq!(of(&old).messages, 2);
        // Another conversation's past is not this chat's to resume.
        assert!(!of(&other).ours);

        let history = History {
            pasts,
            query: "seven hives".into(),
            ..Default::default()
        };
        let found = history.lines();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, old);
        assert_eq!(history.count(), "1 of 3 have that");
    }

    #[test]
    fn a_session_is_read_and_only_a_filed_one_is_deleted() {
        let dir = Scratch::new("history-delete");
        let live = dir.join("app_alice_main.jsonl");
        write(&live, &talk("Now", "Yes."));
        let old = dir.join("app_alice_main.1700000000.jsonl");
        let mut said = talk("Then", "Indeed.");
        said.push(json!({"type": "assistant", "message": {
            "content": "",
            "tool_calls": [{"function":
                {"name": "bash", "arguments": "{}"}}]}}));
        write(&old, &said);

        let mut world = World::new();
        world.insert_resource(History {
            pasts: pasts(&dir, Some(&live)),
            ..Default::default()
        });
        open(&mut world, old.clone());
        let history = world.resource::<History>();
        let reading = history.reading.as_ref().unwrap();
        // Tool steps are left out.
        assert_eq!(reading.shown().len(), 2);
        let holder = reading.holder;
        assert!(world.get::<Transcript>(holder).is_some());

        delete(&mut world);
        assert!(!old.exists());
        assert!(world.get_entity(holder).is_err());
        let history = world.resource::<History>();
        assert!(history.reading.is_none() && !history.fresh);

        // The log in use stays.
        open(&mut world, live.clone());
        delete(&mut world);
        assert!(live.exists());
    }
}
