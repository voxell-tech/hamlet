//! The fitting, a page of the LOOKS stall under the breadcrumb back
//! to its shelf: every agent and hamlet with what each wears, listed
//! as the Inspector lists its hamlets, and one of them on a page of
//! its own, a row to each kind of look, its name with a dropdown of
//! the owned assets that fit at its end, as an agent's page has its
//! settings, and for an agent, its emote played on demand.

use agents_ui::{
    Rostered, breadcrumb, either, link_row, link_rows, moved,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Face, FrameProps, Size, Space, Tone, UiExt, ViewExt,
    column, derived, description, dropdown, icon_button, keyed,
    label, resource, row, scroll,
};
use looks::Outfit;
use looks::wardrobe::{Asset, Category, Target};

use crate::view::Shown;
use crate::view::parts::{
    deal, outcome, page_head, rows, section, sections, top,
};
use crate::view::shelf::browse;
use crate::view::wardrobe::{file_words, wearers};
use crate::wardrobe::closet::Closet;
use crate::wardrobe::{self, Fitting, Wardrobe, worn_in};

/// The fitting: every agent and hamlet, or the one chosen.
pub(super) fn fitting() -> AnyView {
    keyed(
        resource::<Fitting, _>(|fitting| fitting.who.clone()),
        |who| match who {
            Some((target, name)) => one(*target, name.clone()),
            None => everyone(),
        },
    )
    .boxed()
}

/// A page of the fitting under the breadcrumb `trail` back up to the
/// shelf, `here` the page shown, then `body` in a scroll.
fn page(trail: Vec<String>, here: String, body: AnyView) -> AnyView {
    column((
        breadcrumb(trail, here, |world, at| {
            let mut fitting = world.resource_mut::<Fitting>();
            if at == 0 {
                fitting.on = false;
            }
            fitting.who = None;
        }),
        // Room inside the scroll, as a wrapped paragraph can be laid
        // a little wider than the scroll is.
        scroll((column((body,)).padding(Space::Half),)),
    ))
    .boxed()
}

/// What `target` is called, as the meta of its row and page.
fn kind_of(target: Target) -> &'static str {
    match target {
        Target::Agent => "Agent",
        Target::Hamlet => "Hamlet",
    }
}

/// The titles of what `outfit` wears, or that it wears its own.
fn worn_words(
    wardrobe: &Wardrobe,
    outfit: Option<&Outfit>,
) -> String {
    let titles = Category::ALL
        .into_iter()
        .filter_map(|category| worn_in(outfit?, category))
        .map(|id| {
            wardrobe.owned.get(id).map_or_else(
                || id.to_string(),
                |asset| asset.title.clone(),
            )
        })
        .collect::<Vec<_>>();
    match titles.is_empty() {
        true => "Wears its own looks.".into(),
        false => format!("Wears {}.", titles.join(", ")),
    }
}

/// Every agent and hamlet, with what each wears, a row each as the
/// Inspector's hamlets are, each opened to change it.
fn everyone() -> AnyView {
    let all = derived(|world: &World| {
        let wardrobe = world.resource::<Wardrobe>();
        let roster = world.resource::<Rostered>();
        Target::ALL
            .into_iter()
            .flat_map(|target| {
                wearers(roster, target)
                    .into_iter()
                    .map(move |(name, words)| (target, name, words))
            })
            .map(|(target, name, words)| {
                let worn = worn_words(
                    wardrobe,
                    wardrobe.wearing.of(target, &name),
                );
                (target, name, words, worn)
            })
            .collect::<Vec<_>>()
    })
    .when(either(moved::<Wardrobe>(), moved::<Rostered>()));
    let body = keyed(all, |all| {
        let head = page_head(
            "Fitting",
            description("Choose one to change what it wears.")
                .boxed(),
            String::new(),
            None,
        );
        if all.is_empty() {
            return sections(vec![
                head,
                description(
                    "There are no agents or hamlets to dress yet. \
                     Each wears what is bought here once there are.",
                )
                .boxed(),
            ]);
        }
        let rows = all
            .iter()
            .map(|(target, name, words, worn)| {
                let (target, name) = (*target, name.clone());
                link_row(
                    column((
                        row((
                            label(words.to_uppercase())
                                .bold(true)
                                .face(Face::Display),
                            label(kind_of(target))
                                .size(Size::Small)
                                .tone(Tone::Dim),
                        ))
                        .space(Space::Half),
                        label(worn.clone())
                            .size(Size::Small)
                            .tone(Tone::Dim),
                    ))
                    .space(Space::Half)
                    .boxed(),
                )
                .on_click(move |world| {
                    world.resource_mut::<Fitting>().who =
                        Some((target, name.clone()));
                    world.resource_mut::<Wardrobe>().said = None;
                })
                .boxed()
            })
            .collect::<Vec<_>>();
        sections(vec![head, link_rows(rows)])
    });
    page(
        vec![Asset::BACK.to_string()],
        "FITTING".into(),
        body.boxed(),
    )
}

/// One kind's choice, as it is drawn: the owned assets that fit, by
/// id and title, and the place of the one worn among them, 0 for
/// none.
#[derive(Clone, PartialEq, Debug)]
struct Choice {
    category: Category,
    options: Vec<(String, String)>,
    chosen: usize,
}

/// All the looks of one agent or hamlet are drawn from.
#[derive(Clone, PartialEq, Debug)]
struct Now {
    /// Its words.
    shown: String,
    /// What it wears, in words.
    worn: String,
    choices: Vec<Choice>,
    /// An emote is worn, to be played.
    emote: bool,
    /// How each file worn stands, in words.
    files: Vec<String>,
    said: Option<(String, bool)>,
}

impl Now {
    fn of(world: &World, target: Target, name: &str) -> Now {
        let wardrobe = world.resource::<Wardrobe>();
        let outfit = wardrobe.wearing.of(target, name);
        let choices = Category::ALL
            .into_iter()
            .map(|category| {
                let options = wardrobe
                    .wearable(target, category)
                    .map(|asset| {
                        (asset.id.clone(), asset.title.clone())
                    })
                    .collect::<Vec<_>>();
                let worn = outfit
                    .and_then(|outfit| worn_in(outfit, category));
                let chosen = worn
                    .and_then(|worn| {
                        options.iter().position(|(id, _)| id == worn)
                    })
                    .map_or(0, |at| at + 1);
                Choice {
                    category,
                    options,
                    chosen,
                }
            })
            .collect::<Vec<_>>();
        let emote = target == Target::Agent
            && choices.iter().any(|choice| {
                choice.category == Category::Emote
                    && choice.chosen > 0
            });
        let files = Category::ALL
            .into_iter()
            .filter_map(|category| {
                let id = worn_in(outfit?, category)?;
                let asset = wardrobe.owned.get(id)?;
                let file = wardrobe.file(id)?;
                Some(format!(
                    "{}: {}",
                    asset.title,
                    file_words(world, &file)
                ))
            })
            .collect();
        let shown = wearers(world.resource::<Rostered>(), target)
            .into_iter()
            .find(|(one, _)| one == name)
            .map_or_else(|| name.to_string(), |(_, words)| words);
        Now {
            shown,
            worn: worn_words(wardrobe, outfit),
            choices,
            emote,
            files,
            said: wardrobe.said.clone(),
        }
    }
}

/// What the agent or hamlet `name` wears, to be changed, under the
/// breadcrumb back to everyone.
fn one(target: Target, name: String) -> AnyView {
    let read = name.clone();
    let now =
        derived(move |world: &World| Now::of(world, target, &read))
            .when(either(
                either(moved::<Wardrobe>(), moved::<Rostered>()),
                moved::<Closet>(),
            ));
    keyed(now, move |now| {
        let shop = icon_button("tray", "SHOP FOR LOOKS")
            .on_click(move |world| {
                world.resource_mut::<Fitting>().on = false;
                browse::<Asset>(world, |shelf| {
                    shelf.filter = (Some(target), None);
                    shelf.open = None;
                });
            })
            .boxed();
        let head = page_head(
            &now.shown,
            description(now.worn.clone()).boxed(),
            kind_of(target).to_string(),
            None,
        );
        let mut buttons = Vec::new();
        let mut notes = Vec::new();
        let mut parts = Vec::new();
        if now.choices.iter().all(|choice| choice.options.is_empty())
        {
            let who = match target {
                Target::Agent => "this agent",
                Target::Hamlet => "this hamlet",
            };
            notes.push(
                description(format!(
                    "You own nothing for {who} to wear yet. Skins, \
                     emotes and meshes are sold on the shelf."
                ))
                .boxed(),
            );
        } else {
            let mut list = vec![rows(
                now.choices
                    .iter()
                    .map(|choice| choose(target, &name, choice))
                    .collect(),
            )];
            list.extend(
                now.files
                    .iter()
                    .map(|file| description(file.clone()).boxed()),
            );
            parts.push(section(
                "WEARS",
                column(list).space(Space::Half).boxed(),
            ));
            if now.emote {
                let name = name.clone();
                buttons.push(
                    icon_button("play", "PLAY THE EMOTE")
                        .on_click(move |world| {
                            wardrobe::play(world, &name)
                        })
                        .boxed(),
                );
            }
        }
        buttons.push(shop);
        if let Some((words, wrong)) = &now.said {
            notes.push(outcome(words.clone(), *wrong));
        }
        let mut first = vec![head];
        first.push(deal(None, buttons, notes));
        parts.insert(0, top(first));
        page(
            vec![Asset::BACK.to_string(), "FITTING".into()],
            now.shown.to_uppercase(),
            sections(parts),
        )
    })
    .boxed()
}

/// The choice of what is worn of one kind: its plain name, and the
/// dropdown at its end.
fn choose(target: Target, name: &str, choice: &Choice) -> AnyView {
    let names = std::iter::once("None".to_string())
        .chain(choice.options.iter().map(|(_, title)| title.clone()))
        .collect::<Vec<_>>();
    let ids = choice
        .options
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let (name, category) = (name.to_string(), choice.category);
    row((
        column((label(category.one()),)).grow(1.0),
        dropdown(names, choice.chosen as u32).on_pick(
            move |world, at| {
                let id = (at as usize)
                    .checked_sub(1)
                    .and_then(|at| ids.get(at))
                    .cloned();
                wardrobe::wear(world, (target, &name), category, id);
            },
        ),
    ))
    .space(Space::Gap)
    .boxed()
}
