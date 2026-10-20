//! The chat drawn: its head with everyone who speaks in it, and each
//! row, a message in its bubble or the steps of a turn as a fold.

use agents_ui::{Confirm, Focused, confirm, entry_view, markdown};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, Entry, Recipient, SessionName,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, Cx, Face, FrameProps, Hovered,
    ScopedExt, Size, Space, StateExt, Tone, UiExt, ViewExt, World,
    button, column, each, icon, keyed, label, pressable, resource,
    row, scroll,
};

use crate::bar::reply_to;
use crate::reply::{unreplied, words};
use crate::timeline::{
    Fold, Kind, Line, Lines, Reach, Row, Unfolded, Who, refresh,
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

/// Who the open conversation is with.
fn with(world: &World) -> Option<Recipient> {
    world
        .resource::<Focused>()
        .0
        .as_ref()
        .map(|focus| focus.with.clone())
}

/// Asks before a new session files this one in the archive.
fn new_session(world: &mut World) {
    let Some(to) = with(world) else {
        return;
    };
    confirm(
        world,
        Confirm {
            question: "START A NEW SESSION?".into(),
            outcome: "This conversation goes to the Archive, where \
                      it can be read and resumed. The agent keeps \
                      its notes and skills."
                .into(),
            working: Vec::new(),
            verb: "NEW SESSION".into(),
            then: vec![AgentsRequest::Send {
                to,
                text: "/new".into(),
            }],
        },
    );
}

/// Asks before the open conversation is taken back to the message
/// `entry` of `session`.
fn rewind(world: &mut World, session: SessionName, entry: u64) {
    let agent = world.resource::<Lines>().cast.first().map_or_else(
        || "The agent".to_string(),
        |who| who.name.clone(),
    );
    confirm(
        world,
        Confirm {
            question: "REWIND TO HERE?".into(),
            outcome: format!(
                "{agent} forgets this message and everything after \
                 it, and the message goes back in the field. \
                 Changed files, commands that ran, letters and \
                 spending are not undone."
            ),
            working: Vec::new(),
            verb: "REWIND".into(),
            then: vec![AgentsRequest::Rewind { session, entry }],
        },
    );
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
        row((
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
            pressable((icon("speech_plus"), label("New Session")))
                .style(ButtonStyle::Quiet)
                .on_click(new_session),
        ))
        .space(Space::Gap)
        .boxed()
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
    message: &hamlet_engine_sdk::api::agents::SessionMessage,
    to: Option<Recipient>,
) -> AnyView {
    let mut inside = Vec::<AnyView>::new();
    if line.named {
        let mut head = vec![
            face(&line.by),
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
        if !line.by.tag.is_empty() && line.by.kind != Kind::Me {
            head.push(
                label(line.by.tag.clone())
                    .size(Size::Small)
                    .tone(Tone::Faint)
                    .boxed(),
            );
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
            .tone(Tone::Faint)
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
                    cx.set::<hamlet_engine_sdk::ui::Button>(
                        |button, _| button.opacity(1.0),
                    );
                })
                .rules(|cx: &mut Cx| {
                    cx.set::<hamlet_engine_sdk::ui::Button>(
                        |button, _| button.opacity(0.0),
                    );
                })
                .boxed(),
        );
    }
    inside.push(row(foot).space(Space::Gap).boxed());
    column(inside).space(Space::None).boxed()
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
        let open = &mut world.resource_mut::<Unfolded>().0;
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
        Row::Notice(text) => label(text.clone())
            .size(Size::Small)
            .tone(Tone::Dim)
            .boxed(),
    }
}

/// The chat with whoever the open conversation is with.
pub fn transcript() -> AnyView {
    let rows = each(
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
                let to = with(cx.world);
                cx.build(line_view(&line, &owner, to))
            })
        },
    )
    .within(column(()).space(Space::Gap));
    let earlier = keyed(
        resource::<Lines, _>(|lines| (lines.empty, lines.earlier)),
        |(empty, earlier)| match (*empty, *earlier) {
            (true, _) => label(EMPTY).tone(Tone::Dim).boxed(),
            (false, 0) => column(()).boxed(),
            (false, n) => row((
                label(format!("{n} earlier lines."))
                    .size(Size::Small)
                    .tone(Tone::Dim),
                button("EARLIER").style(ButtonStyle::Quiet).on_click(
                    |world| {
                        world.resource_mut::<Reach>().earlier();
                        refresh(world);
                    },
                ),
            ))
            .space(Space::Gap)
            .boxed(),
        },
    );
    scroll((earlier, rows)).boxed()
}
