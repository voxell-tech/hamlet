//! The ARCHIVE tab: the past sessions of the open conversation's
//! agent, searched, each to be read, resumed or deleted, and the
//! reader of one.

use agents_ui::{
    AgentsWorldExt, Focused, Heard, breadcrumb, clip, entry_view,
    link_row, link_rows, name, show_chat, since,
};
use hamlet_engine_bevy::api::agents::{
    AgentsReply, AgentsRequest, Past, Pasts, SessionMessage,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Cx, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, button, column, divider, field,
    icon_button, keyed, label, menu_item, resource, row, scroll,
};

/// Characters of a title a row shows.
const TITLE_SHOWN: usize = 90;

/// Characters of a title the reader's breadcrumb shows, on one line.
const CRUMB_SHOWN: usize = 40;

/// Messages of a session built at a time.
const PAGE: usize = 40;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Archive>();
}

/// The session open in the reader.
#[derive(Clone, PartialEq, Debug)]
struct Reading {
    past: Past,
    /// Its messages, without the steps between them.
    said: Vec<SessionMessage>,
    /// Messages built, counted from the end.
    reach: usize,
}

/// The state of the archive.
#[derive(Resource, Default)]
pub(crate) struct Archive {
    query: String,
    pasts: Pasts,
    reading: Option<Reading>,
    /// A delete of the session being read waits for a yes.
    doomed: bool,
    /// Why the last delete or read failed.
    failed: Option<String>,
    /// The list was asked for since the archive was last forgotten.
    asked: bool,
}

/// Lists the past sessions again, as the search has it.
pub(crate) fn refresh(world: &mut World) {
    let mut archive = world.resource_mut::<Archive>();
    archive.asked = true;
    let query = archive.query.clone();
    world.ask(AgentsRequest::Pasts { query });
}

/// Starts afresh for another conversation.
pub(crate) fn forget(world: &mut World) {
    world.insert_resource(Archive::default());
}

pub(crate) fn heard(world: &mut World, heard: &Heard) {
    match heard {
        // The list of the search typed now, for the agent it was
        // asked for since the archive was last forgotten.
        Heard::Reply(
            Some(AgentsRequest::Pasts { query }),
            AgentsReply::Pasts(pasts),
        ) if world.resource::<Archive>().asked
            && *query == world.resource::<Archive>().query =>
        {
            let mut archive = world.resource_mut::<Archive>();
            archive.pasts = pasts.clone();
            // One that is gone is not read on.
            if let Some(reading) = &archive.reading
                && !pasts
                    .found
                    .iter()
                    .any(|past| past.name == reading.past.name)
            {
                archive.reading = None;
            }
        }
        Heard::Failed(Some(AgentsRequest::ReadPast(name)), why) => {
            let mut archive = world.resource_mut::<Archive>();
            if archive
                .reading
                .as_ref()
                .is_some_and(|reading| reading.past.name == *name)
            {
                archive.reading = None;
                archive.failed = Some(format!(
                    "Could not read that session: {why}"
                ));
            }
        }
        Heard::Reply(
            Some(AgentsRequest::ReadPast(name)),
            AgentsReply::Messages(said),
        ) => {
            if let Some(reading) =
                &mut world.resource_mut::<Archive>().reading
                && reading.past.name == *name
            {
                reading.said.clone_from(said);
            }
        }
        Heard::Reply(
            Some(AgentsRequest::DeletePast(_)),
            AgentsReply::Done,
        ) => {
            refresh(world);
        }
        Heard::Failed(Some(AgentsRequest::DeletePast(_)), why) => {
            world.resource_mut::<Archive>().failed =
                Some(format!("Could not delete that session: {why}"));
            refresh(world);
        }
        _ => {}
    }
}

/// The detail line of a past session's row.
fn detail(past: &Past) -> String {
    let count = match past.messages {
        1 => "1 message".to_string(),
        n => format!("{n} messages"),
    };
    let mut detail = match past.number {
        Some(_) => since(past.modified),
        None => "In use now".to_string(),
    };
    detail.push_str(&format!(", {count}"));
    if !past.tokens.is_empty() {
        detail.push_str(&format!(", {}", past.tokens));
    }
    if !past.by.is_empty() {
        detail.push_str(&format!(", begun by {}", past.by));
    }
    detail
}

/// The count line over the list.
fn count(archive: &Archive) -> String {
    let all = archive.pasts.all;
    let found = archive.pasts.found.len();
    let searched = !archive.query.trim().is_empty();
    match (all, searched) {
        (0, _) => "No sessions yet.".to_string(),
        (1, false) => "1 session".to_string(),
        (_, false) => format!("{all} sessions"),
        (_, true) => format!("{found} of {all} have that"),
    }
}

/// Opens `past` in the reader.
fn open(world: &mut World, past: &Past) {
    let mut archive = world.resource_mut::<Archive>();
    archive.failed = None;
    archive.doomed = false;
    archive.reading = Some(Reading {
        past: past.clone(),
        said: Vec::new(),
        reach: PAGE,
    });
    world.ask(AgentsRequest::ReadPast(past.name.clone()));
}

/// Closes the reader.
fn close(world: &mut World) {
    let mut archive = world.resource_mut::<Archive>();
    archive.reading = None;
    archive.doomed = false;
}

/// Carries on from `past` in the open conversation, which goes to
/// the archive.
fn resume(world: &mut World, past: &Past) {
    let Some(number) = past.number.clone() else {
        return;
    };
    close(world);
    world.tell(format!("/resume {number}"));
    show_chat();
}

/// Deletes the session being read.
fn delete(world: &mut World) {
    let past = world
        .resource::<Archive>()
        .reading
        .as_ref()
        .map(|reading| reading.past.clone());
    close(world);
    // A log in use belongs to an open conversation.
    if let Some(past) = past.filter(|past| past.number.is_some()) {
        world.ask(AgentsRequest::DeletePast(past.name));
    }
}

fn row_view(past: &Past) -> AnyView {
    let live = past.number.is_none();
    let (read, opened, resumed, doomed) =
        (past.clone(), past.clone(), past.clone(), past.clone());
    let mut items =
        vec![menu_item("Read", move |world| open(world, &opened))];
    if past.ours {
        items.push(menu_item("Resume", move |world| {
            resume(world, &resumed)
        }));
    }
    if !live {
        items.push(
            menu_item("Delete...", move |world| {
                // The reader asks before it deletes.
                open(world, &doomed);
                world.resource_mut::<Archive>().doomed = true;
            })
            .critical(),
        );
    }
    link_row(
        column((
            label(clip(&past.title, TITLE_SHOWN))
                .bold(true)
                .tone(if live { Tone::Accent } else { Tone::Body }),
            label(detail(past)).size(Size::Small).tone(Tone::Dim),
        ))
        .space(Space::Half)
        .boxed(),
    )
    .on_click(move |world| open(world, &read))
    .menu(items)
    .boxed()
}

/// The list, under whose it is.
fn list_view() -> AnyView {
    let whose = name(resource::<Focused, _>(|focused| {
        format!("{}: ARCHIVE", focused.whose())
    }));
    let list = column((
        field(resource::<Archive, _>(|archive| {
            archive.query.clone()
        }))
        .placeholder("Search")
        .on_commit(|world, text| {
            world.resource_mut::<Archive>().query = text;
            refresh(world);
        }),
        label(resource::<Archive, _>(|archive| {
            if archive.query.trim().is_empty()
                && archive.pasts.all > 0
            {
                format!(
                    "{}. Type above and press Enter to search them.",
                    count(archive)
                )
            } else {
                count(archive)
            }
        }))
        .size(Size::Small)
        .tone(Tone::Dim),
        label(resource::<Archive, _>(|archive| {
            archive.failed.clone().unwrap_or_default()
        }))
        .tone(Tone::Critical),
        scroll((keyed(
            resource::<Archive, _>(|archive| {
                archive.pasts.found.clone()
            }),
            |found| link_rows(found.iter().map(row_view).collect()),
        ),)),
    ))
    .space(Space::Half);
    column((whose, list)).space(Space::Gap).boxed()
}

/// The buttons for the session being read.
fn actions(past: &Past) -> AnyView {
    let past = past.clone();
    keyed(
        resource::<Archive, _>(|archive| archive.doomed),
        move |doomed| {
            if *doomed {
                return column((
                    label("Delete this session for good?")
                        .tone(Tone::Critical),
                    row((
                        icon_button("bin", "DELETE FOR GOOD")
                            .style(ButtonStyle::Critical)
                            .on_click(delete),
                        button("KEEP IT").on_click(|world| {
                            world.resource_mut::<Archive>().doomed =
                                false;
                        }),
                    ))
                    .space(Space::Half),
                ))
                .space(Space::Half)
                .boxed();
            }
            let mut buttons = Vec::new();
            if past.ours {
                let resumed = past.clone();
                buttons.push(
                    icon_button("play", "RESUME")
                        .style(ButtonStyle::Primary)
                        .on_click(move |world| {
                            resume(world, &resumed)
                        })
                        .boxed(),
                );
            }
            if past.number.is_some() {
                buttons.push(
                    icon_button("bin", "DELETE")
                        .style(ButtonStyle::Critical)
                        .on_click(|world| {
                            world.resource_mut::<Archive>().doomed =
                                true;
                        })
                        .boxed(),
                );
            }
            row(buttons).space(Space::Half).boxed()
        },
    )
    .boxed()
}

/// The reader of the session `reading`.
fn reader(reading: &Reading, speaker: String) -> AnyView {
    let past = &reading.past;
    let hint = match (&past.number, past.ours) {
        (None, _) => "This session is open now. NEW SESSION files \
                      it here."
            .to_string(),
        (Some(number), true) => format!(
            "RESUME carries on from session {number} and files the \
             open one."
        ),
        (Some(_), false) => "This belongs to another conversation. \
                             You can read it or delete it."
            .to_string(),
    };
    let key = past.name.clone();
    let earlier = reading.said.len().saturating_sub(reading.reach);
    let mut shown = Vec::new();
    if earlier > 0 {
        shown.push(
            button("EARLIER")
                .style(ButtonStyle::Quiet)
                .on_click(|world| {
                    if let Some(reading) =
                        &mut world.resource_mut::<Archive>().reading
                    {
                        reading.reach += PAGE;
                    }
                })
                .boxed(),
        );
    }
    shown.extend(reading.said[earlier..].iter().map(|message| {
        entry_view(
            message,
            &speaker,
            format!("archive/{key}/{}", message.id),
            None,
        )
    }));
    column((
        breadcrumb(
            vec!["ARCHIVE".into()],
            clip(&past.title, CRUMB_SHOWN),
            |world, _| close(world),
        )
        .tip(clip(&past.title, TITLE_SHOWN)),
        label(detail(past)).size(Size::Small).tone(Tone::Dim),
        actions(past),
        label(hint).size(Size::Small).tone(Tone::Dim),
        divider(),
        scroll((column(shown).space(Space::Gap),)),
    ))
    .space(Space::Half)
    .boxed()
}

/// The ARCHIVE tab: the list under whose it is, or the reader of one
/// session.
pub(crate) fn screen() -> AnyView {
    keyed(
        resource::<Archive, _>(|archive| archive.reading.clone()),
        |reading| match reading {
            Some(reading) => {
                let reading = reading.clone();
                AnyView::new(move |cx: &mut Cx| {
                    let speaker = cx
                        .world
                        .resource::<Focused>()
                        .agent()
                        .unwrap_or_default();
                    cx.build(reader(&reading, speaker))
                })
            }
            None => list_view(),
        },
    )
    .boxed()
}
