//! The Skill Stall: the skills skills.sh lists, searched and ordered
//! by their installs, each on a page of its own with its SKILL.md,
//! and taken into the library for nothing.

use std::collections::BTreeSet;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, dropdown, label, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_core::core::Skills;
use hamlet_core::skill_market::{
    FetchScrolls, SeekScrolls, SkillMarket, TakeSkill, Taking, Unroll,
};
use hamlet_model::skills_sh::{
    Order, Scroll, Unrolled, front, shown,
};

use crate::icons::Glyph;
use crate::looks::{
    Fills, ICON, action, bare, buttons, good_action, note, page_head,
    plate, stated, title, well,
};
use crate::roll::{either, moved};
use crate::{AnyView, Cx, Me, View, markdown};

/// The rows shown at first, and the more each press of Show More
/// adds.
const SHOWN: usize = 30;

/// The most of a SKILL.md shown on a skill's page, in characters.
const READ_MOST: usize = 6000;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Perusing>()
        .add_systems(Update, read_words);
}

/// What the chief is looking at in the Skill Stall.
#[derive(Resource)]
pub struct Perusing {
    /// Words each of which a skill shown is named by.
    pub words: String,
    pub order: Order,
    /// The most rows shown.
    pub shown: usize,
    /// The skill open on a page of its own, by its id.
    pub open: Option<String>,
}

impl Default for Perusing {
    fn default() -> Self {
        Perusing {
            words: String::new(),
            order: Order::default(),
            shown: SHOWN,
            open: None,
        }
    }
}

/// The field the stall is searched in.
#[derive(Component)]
struct Seeking;

/// Follows what the stall is searched for as it is typed.
fn read_words(
    field: Query<&Children, With<Seeking>>,
    texts: Query<&EditableText>,
    mut perusing: ResMut<Perusing>,
) {
    let Some(text) = field
        .iter()
        .flat_map(|children| children.iter().copied())
        .find_map(|child| texts.get(child).ok())
    else {
        return;
    };
    if text.value() != perusing.words.as_str() {
        perusing.words = text.value().to_string();
        perusing.shown = SHOWN;
    }
}

/// What the stall sells: its skills, or the one open.
pub(super) fn page() -> AnyView {
    keyed(
        resource::<Perusing, _>(|perusing| perusing.open.clone()),
        |open| match open {
            Some(id) => scroll_page(id.clone()),
            None => shelf(),
        },
    )
    .within(bare())
    .boxed()
}

/// A count, short: `950`, `57.2K`, `3.9M`.
fn count(n: u64) -> String {
    if n >= 999_950 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

/// The names of the library's skills, and of their folders, in
/// lower case.
fn shelved(world: &World) -> BTreeSet<String> {
    world
        .resource::<Skills>()
        .0
        .iter()
        .flat_map(|skill| {
            let folder = skill
                .path
                .parent()
                .and_then(|dir| dir.file_name())
                .map(|name| name.to_string_lossy().to_lowercase());
            std::iter::once(skill.name.to_lowercase()).chain(folder)
        })
        .collect()
}

/// True when a skill of the name of `scroll` is on the shelves.
fn in_library(shelved: &BTreeSet<String>, scroll: &Scroll) -> bool {
    shelved.contains(&scroll.skill.to_lowercase())
        || shelved.contains(&scroll.name.to_lowercase())
}

/// One row of the stall, as it is drawn.
#[derive(Clone, PartialEq)]
struct Shown {
    id: String,
    name: String,
    /// Its place, repository and whether it is official.
    told: String,
    installs: String,
    shelved: bool,
}

impl Shown {
    fn of(scroll: &Scroll, shelved: &BTreeSet<String>) -> Shown {
        let mut told = Vec::new();
        if let Some(rank) = scroll.rank {
            told.push(format!("#{rank}"));
        }
        told.push(scroll.source.clone());
        if scroll.official {
            told.push("official".into());
        }
        Shown {
            id: scroll.id(),
            name: scroll.name.clone(),
            told: told.join("  ·  "),
            installs: count(scroll.installs),
            shelved: in_library(shelved, scroll),
        }
    }
}

/// The rows the stall shows, and the count of those past them.
fn rows_now(world: &World) -> (Vec<Shown>, usize) {
    let perusing = world.resource::<Perusing>();
    let market = world.resource::<SkillMarket>();
    let shelved = shelved(world);
    let all = shown(
        &market.scrolls.scrolls,
        &perusing.words,
        perusing.order,
    );
    let rows = all
        .iter()
        .take(perusing.shown)
        .map(|scroll| Shown::of(scroll, &shelved))
        .collect::<Vec<_>>();
    let more = all.len() - rows.len();
    (rows, more)
}

/// A skill's row, which opens its page.
fn shown_row(shown: &Shown) -> AnyView {
    let id = shown.id.clone();
    let mut below =
        vec![row((note(shown.told.clone()),)).fills_width().boxed()];
    if shown.shelved {
        below.push(
            stated(Glyph::Check, "IN LIBRARY", Tone::Accent, 11.0)
                .boxed(),
        );
    }
    button(
        column((
            row((
                row((label(shown.name.clone()).bold(true),))
                    .fills_width(),
                label(format!("{} installs", shown.installs))
                    .size(12.0)
                    .tone(Tone::Faint)
                    .wrap(false),
            ))
            .gap(10.0)
            .align(AlignItems::Center),
            row(below).gap(10.0).align(AlignItems::Center),
        ))
        .gap(3.0)
        .grow(1.0)
        .min_width(px(0.0)),
    )
    .padding(UiRect::axes(px(10.0), px(8.0)))
    .rules(plate)
    .on_activate(move |world| look(world, &id))
    .boxed()
}

/// Opens the skill `id` on its page, and asks for its files.
fn look(world: &mut World, id: &str) {
    world.resource_mut::<Perusing>().open = Some(id.to_string());
    world.trigger(Unroll(id.to_string()));
}

/// Changes what the chief is looking at, from the first rows again.
fn peruse(world: &mut World, change: impl FnOnce(&mut Perusing)) {
    let mut perusing = world.resource_mut::<Perusing>();
    change(&mut perusing);
    perusing.shown = SHOWN;
}

/// Asks skills.sh for what the words typed name, past its
/// leaderboard.
fn ask_wider(world: &mut World) {
    let words = world.resource::<Perusing>().words.trim().to_string();
    if words.chars().count() >= 2 {
        world.trigger(SeekScrolls(words));
    }
}

fn controls() -> impl View {
    let search = row((text_field(
        resource::<Perusing, _>(|p| p.words.clone()),
        |world, text| peruse(world, |p| p.words = text),
    )
    .grow(1.0)
    .rules(well)
    .tagged(Seeking),))
    .gap(12.0)
    .align(AlignItems::Center);
    let orders = buttons(vec![
        dropdown(
            Order::ALL
                .iter()
                .map(|o| o.name().to_string())
                .collect::<Vec<_>>(),
            resource::<Perusing, _>(|p| {
                Order::ALL
                    .iter()
                    .position(|o| *o == p.order)
                    .unwrap_or(0)
            }),
            Glyph::Chevron.image(ICON),
            |world, chosen| {
                peruse(world, |p| p.order = Order::ALL[chosen])
            },
        )
        .min_width(px(130.0))
        .boxed(),
        action(Glyph::Lens, "SEARCH ALL", ask_wider),
        action(Glyph::Restore, "REFRESH", |world| {
            world.trigger(FetchScrolls)
        }),
    ]);
    column((search, orders)).gap(10.0)
}

/// How many skills are shown of how many, and how fresh they are.
fn summary(world: &World) -> String {
    let perusing = world.resource::<Perusing>();
    let market = world.resource::<SkillMarket>();
    let all = &market.scrolls.scrolls;
    if market.fetching && all.is_empty() {
        return "Loading skills from skills.sh...".into();
    }
    if all.is_empty() {
        return market.problem.as_ref().map_or_else(
            || "No skills loaded yet. Choose Refresh.".into(),
            |why| format!("Couldn't reach skills.sh: {why}"),
        );
    }
    let count = shown(all, &perusing.words, perusing.order).len();
    let hours = hamlet_model::log::now_millis()
        .saturating_sub(market.scrolls.fetched)
        / 3_600_000;
    let ago = match hours {
        0 => "less than an hour ago".to_string(),
        1 => "an hour ago".to_string(),
        hours => format!("{hours} hours ago"),
    };
    format!("{count} skills, updated {ago}.")
}

/// What skills.sh found for the words searched, besides what the
/// leaderboard shows: rows, or a word of why there are none.
fn found_now(world: &World) -> Option<Result<Vec<Shown>, String>> {
    let perusing = world.resource::<Perusing>();
    let market = world.resource::<SkillMarket>();
    let (words, found) = market.found.as_ref()?;
    if words.trim() != perusing.words.trim() {
        return None;
    }
    let shelved = shelved(world);
    let listed =
        shown(&market.scrolls.scrolls, words, perusing.order)
            .iter()
            .map(|scroll| scroll.id())
            .collect::<BTreeSet<_>>();
    Some(match found {
        None => Err("Asking skills.sh...".into()),
        Some(Err(why)) => {
            Err(format!("skills.sh could not be asked: {why}"))
        }
        Some(Ok(found)) => {
            let more = found
                .iter()
                .filter(|scroll| !listed.contains(&scroll.id()))
                .map(|scroll| Shown::of(scroll, &shelved))
                .collect::<Vec<_>>();
            match more.is_empty() {
                true => Err("skills.sh has nothing more for those \
                             words."
                    .into()),
                false => Ok(more),
            }
        }
    })
}

/// The stall's list: the controls, then the rows.
fn shelf() -> AnyView {
    let rows = derived(rows_now).when(either(
        moved::<Perusing>(),
        either(moved::<SkillMarket>(), moved::<Skills>()),
    ));
    let list = keyed(rows, |(rows, more)| {
        let mut shown =
            rows.iter().map(shown_row).collect::<Vec<_>>();
        if *more > 0 {
            shown.push(
                row((action(Glyph::More, "SHOW MORE", |world| {
                    world.resource_mut::<Perusing>().shown += SHOWN;
                }),))
                .padding(UiRect::top(px(6.0)))
                .boxed(),
            );
        }
        column(shown).gap(2.0).boxed()
    })
    .within(bare());
    let found = derived(found_now).when(either(
        moved::<Perusing>(),
        either(moved::<SkillMarket>(), moved::<Skills>()),
    ));
    let wider = keyed(found, |found| match found {
        None => bare().boxed(),
        Some(found) => {
            let rows = match found {
                Err(said) => note(said.clone()).boxed(),
                Ok(rows) => column(
                    rows.iter().map(shown_row).collect::<Vec<_>>(),
                )
                .gap(2.0)
                .boxed(),
            };
            column((
                label("More on skills.sh")
                    .size(12.0)
                    .bold(true)
                    .tone(Tone::Faint),
                rows,
            ))
            .gap(6.0)
            .boxed()
        }
    })
    .within(bare());
    let counted = derived(summary)
        .when(either(moved::<Perusing>(), moved::<SkillMarket>()));
    column((controls(), note(counted), list, wider))
        .gap(12.0)
        .boxed()
}

/// All that is known of the skill `id` on its page.
#[derive(Clone, PartialEq)]
struct Opened {
    scroll: Option<Scroll>,
    shelved: bool,
    files: Option<Result<Unrolled, String>>,
}

/// The skill `id` on a page of its own: what it is, where it comes
/// from, its SKILL.md and the way to the library.
fn scroll_page(id: String) -> AnyView {
    let opened = {
        let id = id.clone();
        derived(move |world: &World| {
            let market = world.resource::<SkillMarket>();
            let scroll = market.scroll(&id).cloned();
            Opened {
                shelved: scroll
                    .as_ref()
                    .is_some_and(|s| in_library(&shelved(world), s)),
                files: market.unrolled.get(&id).cloned().flatten(),
                scroll,
            }
        })
        .when(either(moved::<SkillMarket>(), moved::<Skills>()))
    };
    let body = keyed(opened, |opened| match &opened.scroll {
        None => note("This skill is no longer on the table.").boxed(),
        Some(scroll) => about(scroll, opened),
    })
    .within(bare());
    column((
        page_head(
            "SKILLS",
            |world| world.resource_mut::<Perusing>().open = None,
            Glyph::Book,
            "SKILL",
        ),
        body,
    ))
    .gap(16.0)
    .boxed()
}

fn about(scroll: &Scroll, opened: &Opened) -> AnyView {
    let mut parts = vec![
        column((
            title(scroll.name.clone()),
            note(scroll.source.clone()),
        ))
        .gap(4.0)
        .boxed(),
    ];
    let description = match &opened.files {
        Some(Ok(files)) => files.description(),
        _ => String::new(),
    };
    if !description.trim().is_empty() {
        parts.push(markdown::fixed(&description, Tone::Body).boxed());
    }
    parts.push(taking(scroll.id(), opened.shelved));
    parts.push(facts(scroll));
    parts.push(preview(&opened.files));
    column(parts).gap(14.0).boxed()
}

/// A fact of a skill: its name over what it is, which fits a narrow
/// Inspector as well as the wide panel.
fn fact(name: &'static str, value: String) -> AnyView {
    column((
        label(name).size(11.0).tone(Tone::Faint),
        label(value).size(13.0),
    ))
    .gap(2.0)
    .padding(UiRect::right(px(14.0)))
    .boxed()
}

/// A count in full, its thousands apart: `1,094,811`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut grouped = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

fn facts(scroll: &Scroll) -> AnyView {
    let repository = scroll.repository();
    buttons(vec![
        fact("Installs", grouped(scroll.installs)),
        action(Glyph::Globe, "VIEW SOURCE", move |_| {
            hamlet_desktop::open::browse(&repository);
        }),
    ])
    .boxed()
}

/// The way to the library, or how the skill is on its way there.
fn taking(id: String, shelved: bool) -> AnyView {
    let now = {
        let id = id.clone();
        derived(move |world: &World| {
            let market = world.resource::<SkillMarket>();
            let mine = market
                .taking
                .as_ref()
                .filter(|(taken, _)| *taken == id)
                .map(|(_, going)| going.clone());
            (mine, market.busy())
        })
        .when(moved::<SkillMarket>())
    };
    keyed(now, move |(mine, busy)| {
        let said = |words: String, tone: Tone| {
            label(words).size(12.0).tone(tone).boxed()
        };
        let take = {
            let id = id.clone();
            good_action(Glyph::Book, "ADD TO LIBRARY", move |world| {
                let by = world
                    .get_resource::<Me>()
                    .map_or("chief".into(), |me| {
                        format!("chief:{}", me.user)
                    });
                world.trigger(TakeSkill { id: id.clone(), by });
            })
        };
        let parts = match mine {
            Some(Taking::Fetching) => {
                vec![said("Downloading...".into(), Tone::Dim)]
            }
            Some(Taking::Shelving(files)) => vec![said(
                format!(
                    "Adding {files} file{}...",
                    if *files == 1 { "" } else { "s" }
                ),
                Tone::Dim,
            )],
            Some(Taking::Shelved(folder)) => vec![
                stated(
                    Glyph::Check,
                    "IN YOUR LIBRARY",
                    Tone::Accent,
                    12.0,
                )
                .boxed(),
                note(format!(
                    "Saved as `{folder}`. All agents can use it now."
                ))
                .boxed(),
            ],
            _ if shelved => vec![
                stated(
                    Glyph::Check,
                    "IN YOUR LIBRARY",
                    Tone::Accent,
                    12.0,
                )
                .boxed(),
            ],
            Some(Taking::Failed(why)) => vec![
                buttons(vec![take]).boxed(),
                said(
                    format!(
                        "Couldn't add it: {}.",
                        why.trim_end_matches('.')
                    ),
                    Tone::Critical,
                ),
            ],
            None if *busy => vec![
                note("Adding another skill. Try again in a moment.")
                    .boxed(),
            ],
            None => vec![buttons(vec![take]).boxed()],
        };
        column(parts).gap(6.0).boxed()
    })
    .within(bare())
    .boxed()
}

/// The SKILL.md of the skill, below its front matter, and its other
/// files.
fn preview(files: &Option<Result<Unrolled, String>>) -> AnyView {
    let files = match files {
        None => {
            return note("Unrolling its SKILL.md from skills.sh...")
                .boxed();
        }
        Some(Err(why)) => {
            return note(format!(
                "Its files could not be fetched: {why}"
            ))
            .boxed();
        }
        Some(Ok(files)) => files,
    };
    let text = files.text();
    let body = match front(text, "name") {
        Some(_) => text
            .strip_prefix("---")
            .and_then(|rest| rest.split_once("\n---"))
            .map_or(text, |(_, body)| body),
        None => text,
    }
    .trim();
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
    let told = told.join(". ");
    AnyView::new(move |cx: &mut Cx| {
        let palette = &cx.theme().palette;
        let page = column((markdown::fixed(&shown, Tone::Body),))
            .padding(UiRect::axes(px(14.0), px(10.0)))
            .border(1.0)
            .border_color(palette.brass_6)
            .radius(cx.theme().space.round)
            .fill(palette.card);
        cx.build(
            column((
                label("SKILL.md")
                    .size(12.0)
                    .bold(true)
                    .tone(Tone::Faint),
                page,
                label("Files")
                    .size(12.0)
                    .bold(true)
                    .tone(Tone::Faint),
                note(told.clone()),
            ))
            .gap(8.0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_are_told_shortly() {
        assert_eq!(count(950), "950");
        assert_eq!(count(57_196), "57.2K");
        assert_eq!(count(999_949), "999.9K");
        assert_eq!(count(999_950), "1.0M");
        assert_eq!(count(3_881_956), "3.9M");
        assert_eq!(grouped(1_094_811), "1,094,811");
        assert_eq!(grouped(94_811), "94,811");
        assert_eq!(grouped(811), "811");
    }
}
