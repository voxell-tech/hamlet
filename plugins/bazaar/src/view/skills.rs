//! The Skills stall drawn: a skill's row, the search past the
//! leaderboard, and a skill's page with its SKILL.md and the way to
//! the library.

use agents_ui::{Shelves, either, markdown, moved, note};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Face, FrameProps, Size, Space,
    Tone, UiExt, ViewExt, column, derived, description, icon,
    icon_button, keyed, label, row,
};
use hamlet_engine_bevy::{DesktopRequest, HostExt};

use crate::shelf::{Listing, Shelf};
use crate::skills::skills_sh::{Scroll, Unrolled};
use crate::skills::{self, Desk, SkillId, Taking, shelved};
use crate::view::shelf::{Shown, part, summary};
use crate::view::words::{count, grouped};

/// The most characters of a SKILL.md shown on its page.
const READ_MOST: usize = 6000;

impl Shown for Scroll {
    const BACK: &'static str = "SKILLS";
    const NOUN: &'static str = "skills";
    const FIND: &'static str = "Search skills";
    const FROM: Option<&'static str> = Some("skills.sh");

    fn controls() -> Vec<AnyView> {
        vec![
            row((
                icon_button("lens", "SEARCH ALL OF SKILLS.SH")
                    .on_click(|world| {
                        let words = world
                            .resource::<Shelf<Scroll>>()
                            .words
                            .clone();
                        skills::seek(world, &words);
                    }),
                description(
                    "Past the most installed, for the words typed.",
                ),
            ))
            .space(Space::Half)
            .boxed(),
        ]
    }

    fn again(world: &mut World) {
        skills::fetch_board(world);
    }

    fn summary(world: &World) -> String {
        let all = summary::<Scroll>(world);
        let words = &world.resource::<Shelf<Scroll>>().words;
        let more = match &world.resource::<Desk>().found {
            Some((asked, _)) if asked.trim() != words.trim() => {
                return all;
            }
            None => return all,
            Some((_, None)) => "Asking skills.sh...".to_string(),
            Some((_, Some(Err(why)))) => why.clone(),
            Some((_, Some(Ok(0)))) => {
                "skills.sh has nothing more for those words.".into()
            }
            Some((_, Some(Ok(more)))) => {
                format!(
                    "skills.sh found {more} more for those words."
                )
            }
        };
        format!("{all} {more}")
    }

    fn title(&self) -> String {
        self.name.clone()
    }

    fn row(&self) -> AnyView {
        let mut told = Vec::new();
        if let Some(rank) = self.rank {
            told.push(format!("#{rank}"));
        }
        told.push(self.source.clone());
        if self.official {
            told.push("official".into());
        }
        told.push(format!("{} installs", count(self.installs)));
        column((
            row((
                label(self.name.clone()).bold(true),
                in_library(self),
            ))
            .space(Space::Half),
            description(told.join(", ")),
        ))
        .space(Space::None)
        .boxed()
    }

    fn page(&self) -> AnyView {
        let id = self.id();
        let repository = self.repository();
        let files = {
            let id = id.clone();
            derived(move |world: &World| {
                world
                    .resource::<Desk>()
                    .unrolled
                    .get(&id)
                    .cloned()
                    .flatten()
            })
            .when(moved::<Desk>())
        };
        let about = keyed(files, |files| match files {
            Some(Ok(files))
                if !files.description().trim().is_empty() =>
            {
                description(files.description()).boxed()
            }
            _ => column(()).boxed(),
        });
        column((
            note(self.source.clone()),
            about,
            taking(id.clone(), self.clone()),
            row((
                note(format!("{} installs", grouped(self.installs))),
                icon_button("globe", "VIEW SOURCE")
                    .style(ButtonStyle::Quiet)
                    .on_click(move |world| {
                        world.desktop(&DesktopRequest::Open(
                            repository.clone(),
                        ));
                    })
                    .leads_to(self.repository()),
            ))
            .space(Space::Half),
            preview(id),
        ))
        .boxed()
    }

    fn opened(world: &mut World, id: &SkillId) {
        skills::unroll(world, id);
    }
}

/// IN YOUR LIBRARY beside a skill the library has.
fn in_library(scroll: &Scroll) -> AnyView {
    let scroll = scroll.clone();
    keyed(
        derived(move |world: &World| shelved(world, &scroll))
            .when(moved::<Shelves>()),
        |shelved| match shelved {
            true => kept(),
            false => column(()).boxed(),
        },
    )
    .boxed()
}

fn kept() -> AnyView {
    row((
        icon("check").tone(Tone::Accent),
        label("IN YOUR LIBRARY")
            .size(Size::Small)
            .tone(Tone::Accent)
            .face(Face::Display),
    ))
    .space(Space::Half)
    .boxed()
}

/// The way to the library, or how the skill is on its way there.
fn taking(id: SkillId, scroll: Scroll) -> AnyView {
    let now = {
        let id = id.clone();
        derived(move |world: &World| {
            let desk = world.resource::<Desk>();
            let mine = desk
                .taking
                .as_ref()
                .filter(|(taken, _)| *taken == id)
                .map(|(_, going)| going.clone());
            (mine, desk.busy(), shelved(world, &scroll))
        })
        .when(either(moved::<Desk>(), moved::<Shelves>()))
    };
    keyed(now, move |(mine, busy, shelved)| {
        let said = |words: String| description(words).boxed();
        let id = id.clone();
        let take = || {
            let id = id.clone();
            row((icon_button("book", "ADD TO LIBRARY")
                .style(ButtonStyle::Primary)
                .on_click(move |world| skills::take(world, &id)),))
            .boxed()
        };
        let parts = match mine {
            Some(Taking::Fetching) => {
                vec![said("Downloading...".into())]
            }
            Some(Taking::Giving(files)) => vec![said(format!(
                "Adding {files} file{}...",
                if *files == 1 { "" } else { "s" }
            ))],
            Some(Taking::Taken(name)) => vec![
                kept(),
                said(format!(
                    "Saved as `{name}`. All agents can use it now."
                )),
            ],
            _ if *shelved => vec![kept()],
            Some(Taking::Failed(why)) => vec![
                take(),
                label(format!(
                    "Couldn't add it: {}.",
                    why.trim_end_matches('.')
                ))
                .tone(Tone::Critical)
                .boxed(),
            ],
            None if *busy => vec![said(
                "Adding another skill. Try again in a moment.".into(),
            )],
            None => vec![take()],
        };
        column(parts).space(Space::Half).boxed()
    })
    .boxed()
}

/// The SKILL.md of the skill `id`, below its front matter, and its
/// other files.
fn preview(id: SkillId) -> AnyView {
    let files = derived(move |world: &World| {
        world.resource::<Desk>().unrolled.get(&id).cloned()
    })
    .when(moved::<Desk>());
    keyed(files, |files| match files {
        None | Some(None) => {
            description("Unrolling its SKILL.md from skills.sh...")
                .boxed()
        }
        Some(Some(Err(why))) => description(format!(
            "Its files could not be fetched: {why}"
        ))
        .boxed(),
        Some(Some(Ok(files))) => unrolled(files),
    })
    .boxed()
}

fn unrolled(files: &Unrolled) -> AnyView {
    let body = files.body();
    let mut shown = body.chars().take(READ_MOST).collect::<String>();
    if shown.len() < body.len() {
        shown.push_str("\n\n*The rest is in its source.*");
    }
    let mut told = vec![
        files
            .files
            .iter()
            .map(|(path, _)| path.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    ];
    if !files.left.is_empty() {
        told.push(format!("Left out: {}", files.left.join(", ")));
    }
    column((
        part("SKILL.MD", markdown(&shown, Tone::Body)),
        part("FILES", description(told.join(". ")).boxed()),
    ))
    .boxed()
}
