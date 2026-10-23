//! The Skills stall drawn: a skill's row, the search past the
//! leaderboard, and a skill's page with its SKILL.md and the way to
//! the library.

use agents_ui::{Shelves, either, markdown, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, Tone, UiExt, ViewExt, column,
    derived, icon_button, keyed,
};
use hamlet_engine_bevy::{DesktopRequest, HostExt};

use crate::shelf::{Listing, Shelf};
use crate::skills::skills_sh::{Scroll, Unrolled};
use crate::skills::{self, Desk, SkillId, Taking, shelved};
use crate::view::shelf::{Shown, summary};
use crate::view::style::{
    Cost, Line, action_box, aside, meta, owned, prose, section,
    sections,
};
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
            icon_button("lens", "SEARCH SKILLS.SH")
                .on_click(|world| {
                    let words = world
                        .resource::<Shelf<Scroll>>()
                        .words
                        .clone();
                    skills::seek(world, &words);
                })
                .tip(
                    "Searches all of skills.sh for the words typed, \
                     past the most installed.",
                )
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
            Some((_, Some(Err(why)))) => {
                format!("Searching skills.sh failed: {why}")
            }
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

    fn byline(&self) -> String {
        let mut told = vec![
            format!("From {}", self.source),
            format!("{} installs", grouped(self.installs)),
        ];
        if self.official {
            told.push("official".into());
        }
        told.join(", ")
    }

    fn row(&self) -> Line {
        let mut told = Vec::new();
        if let Some(rank) = self.rank {
            told.push(format!("#{rank}"));
        }
        told.push(format!("{} installs", count(self.installs)));
        if self.official {
            told.push("official".into());
        }
        Line {
            cover: "book",
            name: self.name.clone(),
            purpose: self.source.clone(),
            meta: told.join(", "),
            cost: Cost::free(),
            mark: Some(in_library(self)),
        }
    }

    fn page(&self) -> AnyView {
        let id = self.id();
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
                prose(files.description()).boxed()
            }
            _ => column(()).boxed(),
        });
        sections(vec![
            taking(id.clone(), self.clone()),
            about.boxed(),
            preview(id),
        ])
    }

    fn opened(world: &mut World, id: &SkillId) {
        skills::unroll(world, id);
    }
}

/// In your library, beside a skill the library has.
fn in_library(scroll: &Scroll) -> AnyView {
    let scroll = scroll.clone();
    keyed(
        derived(move |world: &World| shelved(world, &scroll))
            .when(moved::<Shelves>()),
        |shelved| match shelved {
            true => owned("In your library"),
            false => column(()).boxed(),
        },
    )
    .boxed()
}

/// The box a skill is taken from: the way to the library, how the
/// skill is on its way there, and its source.
fn taking(id: SkillId, scroll: Scroll) -> AnyView {
    let repository = scroll.repository();
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
        let said = |words: String| meta(words).boxed();
        let take = || {
            let id = id.clone();
            icon_button("book", "TAKE INTO THE LIBRARY")
                .style(ButtonStyle::Primary)
                .on_click(move |world| skills::take(world, &id))
                .boxed()
        };
        let source = {
            let address = repository.clone();
            icon_button("globe", "VIEW SOURCE")
                .style(ButtonStyle::Quiet)
                .on_click(move |world| {
                    world.desktop(&DesktopRequest::Open(
                        address.clone(),
                    ));
                })
                .leads_to(repository.clone())
                .boxed()
        };
        let what = || {
            said(
                "Its files go into your library, and every agent \
                 can use it."
                    .into(),
            )
        };
        let (actions, notes) = match mine {
            Some(Taking::Fetching) => (
                vec![],
                vec![said("Downloading its files...".into())],
            ),
            Some(Taking::Giving(files)) => (
                vec![],
                vec![said(format!(
                    "Adding {files} file{}...",
                    if *files == 1 { "" } else { "s" }
                ))],
            ),
            Some(Taking::Taken(name)) => (
                vec![owned("In your library")],
                vec![said(format!(
                    "Saved as `{name}`. Every agent can use it now."
                ))],
            ),
            _ if *shelved => (vec![owned("In your library")], vec![]),
            Some(Taking::Failed(why)) => (
                vec![take()],
                vec![
                    prose(format!(
                        "Couldn't take it into the library: {}. Try \
                         again in a moment.",
                        why.trim_end_matches('.')
                    ))
                    .tone(Tone::Critical)
                    .boxed(),
                ],
            ),
            None if *busy => (
                vec![],
                vec![said(
                    "Another skill is on its way to the library. \
                     Try again in a moment."
                        .into(),
                )],
            ),
            None => (vec![take()], vec![what()]),
        };
        let mut actions = actions;
        actions.push(source);
        action_box(Some(Cost::free()), actions, notes)
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
            aside("Fetching its SKILL.md from skills.sh...").boxed()
        }
        Some(Some(Err(why))) => aside(format!(
            "Its files could not be fetched: {why} Open its page \
             again to try once more."
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
    sections(vec![
        section("SKILL.MD", markdown(&shown, Tone::Body)),
        section("FILES", aside(told.join(". ")).boxed()),
    ])
}
