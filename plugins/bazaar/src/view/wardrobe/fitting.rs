//! The fitting: what each agent and hamlet wears, each chosen among
//! the owned assets that fit it, kind by kind, and for an agent, its
//! emote played on demand.

use agents_ui::{
    Rostered, breadcrumb, either, link_row, moved, note,
};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, Tone,
    UiExt, ViewExt, column, derived, description, dropdown,
    icon_button, keyed, label, list, resource, row,
};
use looks::Outfit;
use looks::wardrobe::{Asset, Category, Target};

use crate::view::shelf::browse;
use crate::view::wardrobe::{file_words, wearers};
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

/// Every agent and hamlet, with what each wears, each opened to
/// change it.
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
    keyed(all, |all| {
        if all.is_empty() {
            return description(
                "There are no agents or hamlets to dress yet. Each \
                 wears what is bought here once there are.",
            )
            .boxed();
        }
        let rows = all
            .iter()
            .map(|(target, name, words, worn)| {
                let (target, name) = (*target, name.clone());
                let kind = match target {
                    Target::Agent => "AGENT",
                    Target::Hamlet => "HAMLET",
                };
                link_row(
                    column((
                        row((
                            label(words.clone()).bold(true),
                            note(kind),
                        ))
                        .space(Space::Half),
                        description(worn.clone()),
                    ))
                    .space(Space::None)
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
        list(rows).boxed()
    })
    .boxed()
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
            choices,
            emote,
            files,
            said: wardrobe.said.clone(),
        }
    }
}

/// What the agent or hamlet `name` wears, to be changed.
fn one(target: Target, name: String) -> AnyView {
    let read = name.clone();
    let now =
        derived(move |world: &World| Now::of(world, target, &read))
            .when(either(
                either(moved::<Wardrobe>(), moved::<Rostered>()),
                moved::<crate::wardrobe::closet::Closet>(),
            ));
    keyed(now, move |now| {
        let back = |world: &mut World, _: usize| {
            world.resource_mut::<Fitting>().who = None;
        };
        let mut parts = vec![breadcrumb(
            vec!["FITTING".into()],
            now.shown.clone(),
            back,
        )];
        let shop = icon_button("tray", "SHOP")
            .style(ButtonStyle::Quiet)
            .on_click(move |world| {
                world.resource_mut::<Fitting>().on = false;
                browse::<Asset>(world, |shelf| {
                    shelf.filter = (Some(target), None);
                    shelf.open = None;
                });
            })
            .boxed();
        if now.choices.iter().all(|choice| choice.options.is_empty())
        {
            let who = match target {
                Target::Agent => "this agent",
                Target::Hamlet => "this hamlet",
            };
            parts.push(
                description(format!(
                    "You own nothing for {who} to wear yet. Skins, \
                     emotes and meshes are sold on the shelf."
                ))
                .boxed(),
            );
            parts.push(row((shop,)).boxed());
        } else {
            for choice in &now.choices {
                parts.push(choose(target, &name, choice));
            }
            let mut more = Vec::new();
            if now.emote {
                let name = name.clone();
                more.push(
                    icon_button("play", "PLAY EMOTE")
                        .on_click(move |world| {
                            wardrobe::play(world, &name)
                        })
                        .boxed(),
                );
            }
            more.push(shop);
            parts.push(row(more).space(Space::Half).boxed());
            for file in &now.files {
                parts.push(description(file.clone()).boxed());
            }
        }
        if let Some((words, wrong)) = &now.said {
            let tone =
                if *wrong { Tone::Critical } else { Tone::Accent };
            parts.push(label(words.clone()).tone(tone).boxed());
        }
        column(parts).space(Space::Half).boxed()
    })
    .boxed()
}

/// The choice of what is worn of one kind.
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
        note(category.one().to_uppercase()),
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
    .space(Space::Half)
    .boxed()
}
