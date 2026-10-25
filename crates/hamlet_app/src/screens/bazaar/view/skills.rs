//! The Skills stall drawn: a skill's row, the search past the
//! leaderboard, and a skill's page with its SKILL.md and the way to
//! the library.

use bevy::ecs::world::World;
use bevy::ui::AlignItems;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, label, row};
use bevy_fynix::{ViewExt, derived, keyed};
use hamlet_client::realm::Skills;
use hamlet_ui::styles::{action, bare, good_action, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx, markdown};

use super::parts::{
    Line, deal, mark, nothing, page_head, problem, rows, section,
    sections, top,
};
use super::shelf::{Shown, summary};
use super::words::{count, grouped};
use crate::bazaar::BazaarSettings;
use crate::bazaar::shelf::{Listing, Shelf};
use crate::bazaar::skills::skills_sh::{Scroll, Unrolled};
use crate::bazaar::skills::{self, Desk, SkillId, Taking, shelved};
use crate::roll::{either, moved};

impl Shown for Scroll {
    const BACK: &'static str = "SKILLS";
    const ABOUT: &'static str =
        "Skills from skills.sh, for your library.";
    const NOUN: &'static str = "skills";
    const FIND: &'static str =
        "Search by a skill's name or its repository's.";
    const FROM: Option<&'static str> = Some("skills.sh");

    fn controls() -> Vec<AnyView> {
        vec![
            row((action(Glyph::Lens, "SEARCH SKILLS.SH", |world| {
                let words =
                    world.resource::<Shelf<Scroll>>().words.clone();
                skills::seek(world, &words);
            }),))
            .align(AlignItems::Center)
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
                note(files.description()).boxed()
            }
            _ => nothing(),
        })
        .within(bare());
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
        sections(vec![
            top(vec![head, taking(id.clone(), self.clone())]),
            preview(id),
        ])
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
            .when(moved::<Skills>()),
        |shelved| match shelved {
            true => mark("In your library"),
            false => nothing(),
        },
    )
    .within(bare())
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
        .when(either(moved::<Desk>(), moved::<Skills>()))
    };
    let pressed = keyed(now, move |(mine, busy, shelved)| {
        let said = |words: String| note(words).boxed();
        let take = || {
            let id = id.clone();
            good_action(Glyph::Book, "ADD TO LIBRARY", move |world| {
                skills::take(world, &id);
            })
        };
        let source = {
            let address = repository.clone();
            action(Glyph::Globe, "VIEW SOURCE", move |_| {
                hamlet_desktop::open::browse(&address);
            })
        };
        let what = || {
            said(
                "Its files go into your library, and every agent \
                 can use it."
                    .into(),
            )
        };
        let (mut pressed, notes) = match mine {
            Some(Taking::Fetching) => (
                vec![],
                vec![said("Downloading its files...".into())],
            ),
            Some(Taking::Giving(files, _)) => (
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
        pressed.push(source);
        deal(None, pressed, notes)
    })
    .within(bare());
    deal(Some("Free".into()), Vec::new(), vec![pressed.boxed()])
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
        Some(Some(Ok(files))) => unrolled(files.clone()),
    })
    .within(bare())
    .boxed()
}

fn unrolled(files: Unrolled) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let most = cx.world.resource::<BazaarSettings>().read_most;
        let body = files.body();
        let mut shown = body.chars().take(most).collect::<String>();
        if shown.len() < body.len() {
            shown.push_str("\n\n*The rest is in its source.*");
        }
        let mut listed = files
            .files
            .iter()
            .map(|(path, _)| label(path.clone()).size(13.0).boxed())
            .collect::<Vec<_>>();
        if !files.left.is_empty() {
            listed.push(
                note(format!("Left out: {}", files.left.join(", ")))
                    .boxed(),
            );
        }
        cx.build(sections(vec![
            section(
                "SKILL.MD",
                markdown::fixed(&shown, Tone::Body).boxed(),
            ),
            section("FILES", rows(listed)),
        ]))
    })
}
