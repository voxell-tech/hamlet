//! The chat drawn: its head with everyone who speaks in it, and each
//! row, a message in its bubble or the steps of a turn as a fold.

use hamlet_engine_bevy::api::agents::{
    AgentsRequest, Entry, Recipient, SessionMessage, SessionName,
};
use hamlet_engine_bevy::prelude::World;
use hamlet_engine_bevy::ui::{
    Align, AnyView, Button, ButtonProps, ButtonStyle, Cx, Face, Fill,
    FrameProps, Hovered, ScopedExt, ScrollProps, Size, Space,
    StateExt, Tone, UiExt, ViewExt, button, column, description,
    divider, each, icon, keyed, label, pressable, resource, row,
    scroll,
};

use crate::chat::bar::reply_to;
use crate::chat::reply::{unreplied, words};
use crate::chat::talk::Talk;
use crate::chat::timeline::{
    Fold, Kind, Line, Lines, Reach, Row, Unfolded, Who, refresh,
};
use crate::shared::{
    AgentsWorldExt, Confirm, Focused, entry_view, markdown,
};

/// The most pictures the head of the chat shows.
const FACES: usize = 6;

const EMPTY: &str = "Nothing has been said here yet. Agents can \
                     make mistakes, so check anything important.";

/// The icon that stands for `who`.
fn face(who: &Who) -> AnyView {
    let (name, tone) = match who.kind {
        Kind::Me | Kind::Chief => ("flame", Tone::Accent),
        Kind::Mayor => ("crown", Tone::Accent),
        Kind::Agent => ("robot", Tone::Body),
    };
    icon(name).tone(tone).boxed()
}

/// The tone a speaker's name is set in.
fn ink(who: &Who) -> Tone {
    match who.kind {
        Kind::Me | Kind::Chief => Tone::Accent,
        _ => Tone::Body,
    }
}

/// The names of everyone in `cast`, the chief at this window last.
fn names(cast: &[Who]) -> String {
    let mut names = cast
        .iter()
        .filter(|who| who.kind != Kind::Me && !who.name.is_empty())
        .map(|who| who.name.clone())
        .collect::<Vec<_>>();
    if cast.iter().any(|who| who.kind == Kind::Me) {
        names.push("you".to_string());
    }
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            format!("{} and {last}", rest.join(", "))
        }
        _ => names.concat(),
    }
}

/// Asks before a new session files this one in the archive.
fn new_session(world: &mut World) {
    let Some(to) = world.resource::<Focused>().with() else {
        return;
    };
    world.confirm(Confirm {
        question: "START A NEW SESSION?".into(),
        outcome: "This conversation goes to the Archive, where it \
                  can be read and resumed. The agent keeps its \
                  notes and skills."
            .into(),
        working: Vec::new(),
        verb: "NEW SESSION".into(),
        then: vec![AgentsRequest::Send {
            to,
            text: "/new".into(),
        }],
    });
}

/// Asks before the open conversation is taken back to the message
/// `entry` of `session`.
fn rewind(world: &mut World, session: SessionName, entry: u64) {
    let agent = world.resource::<Lines>().cast.first().map_or_else(
        || "The agent".to_string(),
        |who| who.name.clone(),
    );
    world.confirm(Confirm {
        question: "REWIND TO HERE?".into(),
        outcome: format!(
            "{agent} forgets this message and everything after it, \
             and the message goes back in the field. Changed files, \
             commands that ran, letters and spending are not undone."
        ),
        working: Vec::new(),
        verb: "REWIND".into(),
        then: vec![AgentsRequest::Rewind { session, entry }],
    });
}

/// The head of the chat: the house it is and everyone who speaks in
/// it.
pub fn header() -> AnyView {
    keyed(resource::<Lines, _>(|lines| lines.cast.clone()), |cast| {
        let Some(owner) = cast.first() else {
            return column(()).boxed();
        };
        let title = match owner.kind {
            Kind::Mayor => "THE MAYOR'S HALL".to_string(),
            _ => format!("{}'S HOUSE", owner.name.to_uppercase()),
        };
        let me = |who: &&Who| who.kind == Kind::Me;
        let faces = cast
            .iter()
            .filter(|who| !me(who))
            .chain(cast.iter().filter(me))
            .take(FACES)
            .map(face)
            .collect::<Vec<_>>();
        let head = row((
            row(faces).space(Space::Half),
            column((
                label(title)
                    .size(Size::Small)
                    .bold(true)
                    .face(Face::Display)
                    .tone(Tone::Faint),
                label(names(cast)).size(Size::Small).tone(Tone::Dim),
            ))
            .space(Space::None),
            pressable((icon("speech_plus"),))
                .style(ButtonStyle::Quiet)
                .on_click(new_session)
                .tip(
                    "Start a new session. This one goes to the \
                     Archive.",
                ),
        ))
        .space(Space::Gap);
        column((head, divider())).space(Space::Half).boxed()
    })
    .boxed()
}

/// The words of a message set plainly, its quoted lines dim.
fn plain(text: &str) -> AnyView {
    let mut parts = Vec::<AnyView>::new();
    let mut run = Vec::<&str>::new();
    let mut quoting = false;
    let flush = |run: &mut Vec<&str>,
                 quoting: bool,
                 parts: &mut Vec<AnyView>| {
        if run.is_empty() {
            return;
        }
        let words = run.join("\n");
        parts.push(if quoting {
            row((
                label("|").tone(Tone::Faint),
                label(words).tone(Tone::Dim),
            ))
            .space(Space::Half)
            .boxed()
        } else {
            label(words).boxed()
        });
        run.clear();
    };
    for line in text.lines() {
        let quoted = line
            .strip_prefix("> ")
            .or_else(|| line.strip_prefix('>'));
        if quoted.is_some() != quoting {
            flush(&mut run, quoting, &mut parts);
            quoting = quoted.is_some();
        }
        run.push(quoted.unwrap_or(line));
    }
    flush(&mut run, quoting, &mut parts);
    column(parts).space(Space::None).boxed()
}

/// The key a change of the message `id` of `session` is folded by.
fn change_key(session: &SessionName, id: u64) -> String {
    format!(
        "{}/{}/{}/{id}",
        session.channel, session.chat, session.thread
    )
}

/// One message in its bubble: its speaker named at the head of a run,
/// its words, its hour, and what can be done to it under the pointer.
fn said(
    line: &Line,
    session: &SessionName,
    message: &SessionMessage,
    to: Option<Recipient>,
) -> AnyView {
    let mine = line.by.kind == Kind::Me;
    let mut inside = Vec::<AnyView>::new();
    if line.named && !mine {
        let mut head = vec![
            label(line.by.name.clone())
                .size(Size::Small)
                .bold(true)
                .tone(ink(&line.by))
                .boxed(),
        ];
        if let Some(to) = &line.to {
            head.push(
                label(format!("to {to}"))
                    .size(Size::Small)
                    .tone(Tone::Dim)
                    .boxed(),
            );
        }
        if !line.by.tag.is_empty() {
            let tag = label(line.by.tag.clone()).size(Size::Small);
            head.push(match line.by.kind {
                Kind::Mayor => tag
                    .bold(true)
                    .face(Face::Display)
                    .tone(Tone::Faint)
                    .boxed(),
                _ => tag.tone(Tone::Dim).boxed(),
            });
        }
        inside.push(row(head).space(Space::Half).boxed());
    }
    let speaker = line.by.name.clone();
    let key = change_key(session, message.id);
    let body = match &message.entry {
        Entry::User { .. } if !message.reply => {
            let typed = words(&message.entry);
            if let Some((answers, _)) = match &message.entry {
                Entry::User { text, .. } => unreplied(text),
                _ => None,
            } {
                inside.push(
                    label(format!(
                        "Replying to {}: {}",
                        answers.by, answers.text
                    ))
                    .size(Size::Small)
                    .tone(Tone::Dim)
                    .boxed(),
                );
            }
            plain(typed)
        }
        Entry::Answered(text) => plain(text),
        Entry::Error(_) => entry_view(message, &speaker, key, to),
        entry => markdown(words(entry), Tone::Body),
    };
    inside.push(body);
    let mut foot = vec![
        label(line.hour.clone())
            .size(Size::Small)
            .tone(if mine { Tone::Faint } else { Tone::Dim })
            .boxed(),
    ];
    // Shown under the pointer.
    let mut acts = Vec::<AnyView>::new();
    if let Some(by) = &line.replies {
        let by = by.clone();
        let text = words(&message.entry).to_string();
        acts.push(
            button("Reply")
                .style(ButtonStyle::Quiet)
                .on_click(move |world| reply_to(world, &by, &text))
                .boxed(),
        );
    }
    if line.rewinds {
        let session = session.clone();
        let entry = message.id;
        acts.push(
            button("Rewind to here")
                .style(ButtonStyle::Quiet)
                .on_click(move |world| {
                    rewind(world, session.clone(), entry)
                })
                .boxed(),
        );
    }
    if !acts.is_empty() {
        foot.push(
            row(acts)
                .space(Space::Half)
                .when::<Hovered, _>(|cx: &mut Cx| {
                    cx.set::<Button>(|button, _| button.opacity(1.0));
                })
                .rules(|cx: &mut Cx| {
                    cx.set::<Button>(|button, _| button.opacity(0.0));
                })
                .boxed(),
        );
    }
    inside.push(row(foot).space(Space::Gap).boxed());
    let bubble = column(inside).space(Space::None);
    // The chief's own at the right on the accent, anyone else's at
    // the left on a card by its speaker's disc, shown at the end of
    // its run.
    if mine {
        return row((bubble.fill(Fill::Accent),))
            .align(Align::End)
            .boxed();
    }
    let disc = column((face(&line.by),))
        .fill(Fill::Disc)
        .opacity(if line.last { 1.0 } else { 0.0 });
    row((disc, bubble.fill(Fill::Card)))
        .space(Space::Half)
        .boxed()
}

/// The steps of a turn as one line that opens.
fn fold_view(
    fold: &Fold,
    speaker: &str,
    to: Option<Recipient>,
) -> AnyView {
    let tone = match (fold.running, fold.failed) {
        (true, _) => Tone::Accent,
        (false, 0) => Tone::Dim,
        _ => Tone::Critical,
    };
    let toggled = (fold.session.clone(), fold.first);
    let head = pressable((label(fold.heading())
        .size(Size::Small)
        .tone(tone),))
    .style(ButtonStyle::Quiet)
    .on_click(move |world| {
        let mut unfolded = world.resource_mut::<Unfolded>();
        let open = &mut unfolded.0;
        match open.iter().position(|one| *one == toggled) {
            Some(at) => {
                open.remove(at);
            }
            None => open.push(toggled.clone()),
        }
        refresh(world);
    });
    let inside = fold
        .shown()
        .iter()
        .map(|(message, _)| {
            entry_view(
                message,
                speaker,
                change_key(&fold.session, message.id),
                to.clone(),
            )
        })
        .collect::<Vec<_>>();
    column((head, column(inside).space(Space::Half)))
        .space(Space::Half)
        .boxed()
}

/// A row of the chat.
fn line_view(
    line: &Line,
    owner: &str,
    to: Option<Recipient>,
) -> AnyView {
    match &line.row {
        Row::Message {
            session, message, ..
        } => said(line, session, message, to),
        Row::Fold(fold) => fold_view(fold, owner, to),
        Row::Notice(text) => description(text.clone()).boxed(),
    }
}

/// The chat with whoever the open conversation is with.
pub fn transcript() -> AnyView {
    // Another conversation is shown from its end.
    keyed(resource::<Talk, _>(|talk| talk.open.clone()), |_| {
        scroll((earlier(), rows()))
            .stick(true)
            .on_top(|world| {
                if world.resource::<Lines>().earlier > 0 {
                    world.resource_mut::<Reach>().earlier();
                    refresh(world);
                }
            })
            .boxed()
    })
    .boxed()
}

/// The rows of the chat that are built.
fn rows() -> AnyView {
    each(
        resource::<Lines, _>(|lines| {
            let owner = lines
                .cast
                .first()
                .map(|who| who.name.clone())
                .unwrap_or_default();
            lines
                .rows
                .iter()
                .map(|line| (line.clone(), owner.clone()))
                .collect::<Vec<_>>()
        }),
        |(line, owner): &(Line, String)| {
            (line.clone(), owner.clone())
        },
        |(line, owner)| {
            let line = line.clone();
            let owner = owner.clone();
            AnyView::new(move |cx: &mut Cx| {
                let to = cx.world.resource::<Focused>().with();
                cx.build(line_view(&line, &owner, to))
            })
        },
    )
    .within(column(()).space(Space::Gap))
    .boxed()
}

/// What stands over the rows: how many more there are above, or
/// that nothing was said.
fn earlier() -> AnyView {
    keyed(
        resource::<Lines, _>(|lines| (lines.empty, lines.earlier)),
        |(empty, earlier)| match (*empty, *earlier) {
            (true, _) => description(EMPTY).boxed(),
            (false, 0) => column(()).boxed(),
            (false, n) => description(format!(
                "{n} earlier lines. Scroll up to read them."
            ))
            .boxed(),
        },
    )
    .boxed()
}
