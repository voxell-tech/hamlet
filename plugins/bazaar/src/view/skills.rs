//! The Skills stall drawn: a skill's row, the search past the
//! leaderboard, and a skill's page with its SKILL.md and the way to
//! the library.

use agents_ui::{Shelves, either, markdown, moved};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Tone, UiExt, ViewExt, column, derived, description,
    icon_button, keyed, label, row,
};
use hamlet_engine_bevy::{DesktopRequest, HostExt};

use crate::shelf::{Listing, Shelf};
use crate::skills::skills_sh::{Scroll, Unrolled};
use crate::skills::{self, Desk, SkillId, Taking, shelved};
use crate::view::parts::{
    Line, deal, mark, note, page_head, problem, rows, section,
    sections, top,
};
use crate::view::shelf::{Shown, summary};
use crate::view::words::{count, grouped};

/// The most characters of a SKILL.md shown on its page.
const READ_MOST: usize = 6000;

impl Shown for Scroll {
    const BACK: &'static str = "SKILLS";
    const ABOUT: &'static str =
        "Skills from skills.sh, for your library.";
    const NOUN: &'static str = "skills";
    const FIND: &'static str = "Search skills";
    const FROM: Option<&'static str> = Some("skills.sh");

    fn controls() -> Vec<AnyView> {
        // In a row of its own, so it keeps its width in its share.
        vec![
            row((icon_button("lens", "SEARCH SKILLS.SH")
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
                ),))
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
            name: self.name.clone(),
            price: "Free".into(),
            purpose: self.source.clone(),
            meta: told.join(", "),
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
                description(files.description()).boxed()
            }
            _ => column(()).boxed(),
        });
        let mut told = vec![
            format!("From {}", self.source),
            format!("{} installs", grouped(self.installs)),
        ];
        if self.official {
            told.push("official".into());
        }
        let head = page_head(
            &self.name,
            about.boxed(),
            told.join(", "),
            Some(in_library(self)),
        );
        let mut parts =
            vec![top(vec![head, taking(id.clone(), self.clone())])];
        parts.push(preview(id));
        sections(parts)
    }

    fn opened(world: &mut World, id: &SkillId) {
        skills::unroll(world, id);
    }
}

/// The accent line of a skill the library has.
fn in_library(scroll: &Scroll) -> AnyView {
    let scroll = scroll.clone();
    keyed(
        derived(move |world: &World| shelved(world, &scroll))
            .when(moved::<Shelves>()),
        |shelved| match shelved {
            true => mark("In your library"),
            false => column(()).boxed(),
        },
    )
    .boxed()
}

/// The way a skill is had: free, the way to the library and to its
/// source, and how the skill is on its way.
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
    let buttons = keyed(now, move |(mine, busy, shelved)| {
        let said = |words: String| description(words).boxed();
        let take = || {
            let id = id.clone();
            icon_button("book", "ADD TO LIBRARY")
                .on_click(move |world| skills::take(world, &id))
                .boxed()
        };
        let source = {
            let address = repository.clone();
            icon_button("globe", "VIEW SOURCE")
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
        let (mut buttons, notes) = match mine {
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
                vec![],
                vec![said(format!(
                    "Saved as `{name}`. Every agent can use it now."
                ))],
            ),
            _ if *shelved => (vec![], vec![]),
            Some(Taking::Failed(why)) => (
                vec![take()],
                vec![problem(format!(
                    "Couldn't add it to the library: {}. Try again \
                     in a moment.",
                    why.trim_end_matches('.')
                ))],
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
        buttons.push(source);
        deal(None, buttons, notes)
    });
    deal(Some("Free".into()), Vec::new(), vec![buttons.boxed()])
}

/// The SKILL.md of the skill `id`, below its front matter, and its
/// other files.
fn preview(id: SkillId) -> AnyView {
    let files = derived(move |world: &World| {
        world.resource::<Desk>().unrolled.get(&id).cloned()
    })
    .when(moved::<Desk>());
    keyed(files, |files| match files {
        None | Some(None) => section(
            "SKILL.MD",
            note("Fetching its SKILL.md from skills.sh...").boxed(),
        ),
        Some(Some(Err(why))) => section(
            "SKILL.MD",
            problem(format!(
                "Its files could not be fetched: {why} Open its \
                 page again to try once more."
            )),
        ),
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
    let mut listed = files
        .files
        .iter()
        .map(|(path, _)| label(path.clone()).boxed())
        .collect::<Vec<_>>();
    if !files.left.is_empty() {
        listed.push(
            description(format!(
                "Left out: {}",
                files.left.join(", ")
            ))
            .boxed(),
        );
    }
    sections(vec![
        section("SKILL.MD", markdown(&shown, Tone::Body)),
        section("FILES", rows(listed)),
    ])
}
