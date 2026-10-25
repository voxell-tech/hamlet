//! The fitting, a page of the LOOKS stall under the way back to its
//! shelf: every agent and hamlet with what each wears, listed as the
//! Inspector lists its hamlets, and one of them on a page of its own,
//! a row to each kind of look, its name with a dropdown of the owned
//! assets that fit at its end, as an agent's page has its settings,
//! and for an agent, its emote played on demand.

use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, dropdown, label, row,
    scroll,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, keyed, resource};
use hamlet_protocol::client as protocol;
use hamlet_realm::wardrobe::{Asset, Category, Target};
use hamlet_ui::AnyView;
use hamlet_ui::styles::{
    Fills, ICON, action, bare, filling, link_row, note, note_rules,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::DisplayText;

use super::file_words;
use crate::bazaar::shelf::Shelf;
use crate::bazaar::view::parts::{
    crumb, deal, outcome, page_head, rows, section, sections, top,
};
use crate::bazaar::view::shelf::{Shown, browse};
use crate::bazaar::wardrobe::closet::Closet;
use crate::bazaar::wardrobe::{
    self, Dressed, Fitting, Wardrobe, Who, worn_in,
};
use crate::roll::{either, moved};

/// The fitting: every agent and hamlet, or the one chosen.
pub(super) fn fitting() -> AnyView {
    keyed(
        resource::<Fitting, _>(|fitting| fitting.who.clone()),
        |who| match who {
            Some(who) => one(who.clone()),
            None => everyone(),
        },
    )
    .within(filling())
    .boxed()
}

/// Back from the fitting to the shelf.
fn to_shelf(world: &mut World) {
    let mut fitting = world.resource_mut::<Fitting>();
    fitting.on = false;
    fitting.who = None;
}

/// Back from one agent or hamlet to everyone.
fn to_everyone(world: &mut World) {
    world.resource_mut::<Fitting>().who = None;
    world.resource_mut::<Wardrobe>().said = None;
}

/// A page of the fitting under the way back to `front`, which `act`
/// takes, `here` its name, then `body` in a scroll.
fn page(
    front: &'static str,
    act: fn(&mut World),
    here: String,
    body: AnyView,
) -> AnyView {
    column((crumb(front, act, here), scroll((body,)).fills_height()))
        .gap(14.0)
        .fills_height()
        .boxed()
}

/// What `target` is called, as the meta of its row and page.
fn kind_of(target: Target) -> &'static str {
    match target {
        Target::Agent => "Agent",
        Target::Hamlet => "Hamlet",
    }
}

/// The titles of what `looks` wears, or that it wears its own.
fn worn_words(
    wardrobe: &Wardrobe,
    looks: &protocol::Looks,
) -> String {
    let titles = Category::ALL
        .into_iter()
        .filter_map(|category| worn_in(looks, category))
        .map(|worn| wardrobe.title(worn))
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
        world
            .resource::<Dressed>()
            .0
            .iter()
            .map(|one| {
                (
                    one.who.clone(),
                    one.words.clone(),
                    worn_words(wardrobe, &one.looks),
                )
            })
            .collect::<Vec<_>>()
    })
    .when(either(moved::<Wardrobe>(), moved::<Dressed>()));
    let body = keyed(all, |all| {
        let head = page_head(
            "Fitting",
            note("Choose one to change what it wears.").boxed(),
            String::new(),
            None,
        );
        if all.is_empty() {
            return sections(vec![
                head,
                note(
                    "There are no agents or hamlets to dress yet. \
                     Each wears what is bought here once there are.",
                )
                .boxed(),
            ]);
        }
        let list = all
            .iter()
            .map(|(who, words, worn)| {
                dresser(who.clone(), words, worn)
            })
            .collect::<Vec<_>>();
        sections(vec![head, rows(list)])
    })
    .within(bare());
    page(Asset::BACK, to_shelf, "FITTING".into(), body.boxed())
}

/// The row of `who`, shown as `words`, wearing `worn`, which opens
/// its page.
fn dresser(who: Who, words: &str, worn: &str) -> AnyView {
    let target = who.target();
    button(
        row((
            column((
                row((
                    label(words.to_uppercase())
                        .size(13.0)
                        .bold(true)
                        .tone(Tone::Body)
                        .tagged(DisplayText),
                    label(kind_of(target))
                        .size(12.0)
                        .bold(true)
                        .tone(Tone::Faint),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
                label(worn.to_string()).bold(false).rules(note_rules),
            ))
            .gap(3.0)
            .fills_width(),
            glyph(Glyph::Into, ICON).tone(Tone::Dim),
        ))
        .gap(12.0)
        .grow(1.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(14.0), px(6.0)))
    .rules(link_row)
    .tagged(Variant::Default)
    .on_activate(move |world| {
        world.resource_mut::<Fitting>().who = Some(who.clone());
        world.resource_mut::<Wardrobe>().said = None;
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
    /// Its words, if it is still there.
    shown: Option<String>,
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
    fn of(world: &World, who: &Who) -> Now {
        let wardrobe = world.resource::<Wardrobe>();
        let dresser = world.resource::<Dressed>().find(who);
        let looks =
            dresser.map(|one| one.looks.clone()).unwrap_or_default();
        let target = who.target();
        let choices = Category::ALL
            .into_iter()
            .map(|category| {
                let options = wardrobe
                    .wearable(target, category)
                    .map(|asset| {
                        (asset.id.clone(), asset.title.clone())
                    })
                    .collect::<Vec<_>>();
                let worn =
                    worn_in(&looks, category).map(wardrobe::asset_of);
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
        let emote = target == Target::Agent && looks.emote.is_some();
        let files = Category::ALL
            .into_iter()
            .filter_map(|category| {
                let worn = worn_in(&looks, category)?;
                Some(format!(
                    "{}: {}",
                    wardrobe.title(worn),
                    file_words(world, worn)
                ))
            })
            .collect();
        Now {
            shown: dresser.map(|one| one.words.clone()),
            worn: worn_words(wardrobe, &looks),
            choices,
            emote,
            files,
            said: wardrobe.said.clone(),
        }
    }
}

/// What the agent or hamlet `who` wears, to be changed, under the way
/// back to everyone.
fn one(who: Who) -> AnyView {
    let read = who.clone();
    let now = derived(move |world: &World| Now::of(world, &read))
        .when(either(
            either(moved::<Wardrobe>(), moved::<Dressed>()),
            moved::<Closet>(),
        ));
    keyed(now, move |now| {
        let target = who.target();
        let Some(shown) = now.shown.clone() else {
            return page(
                "FITTING",
                to_everyone,
                "...".into(),
                note("It is not here any more.").boxed(),
            );
        };
        let shop =
            action(Glyph::Coins, "SHOP FOR LOOKS", move |world| {
                world.resource_mut::<Fitting>().on = false;
                browse::<Asset>(world, |shelf: &mut Shelf<Asset>| {
                    shelf.filter = (Some(target), None);
                    shelf.open = None;
                });
            });
        let head = page_head(
            &shown,
            note(now.worn.clone()).boxed(),
            kind_of(target).to_string(),
            None,
        );
        let mut buttons = Vec::new();
        let mut notes = Vec::new();
        let mut parts = Vec::new();
        if now.choices.iter().all(|choice| choice.options.is_empty())
        {
            let whose = match target {
                Target::Agent => "this agent",
                Target::Hamlet => "this hamlet",
            };
            notes.push(
                note(format!(
                    "You own nothing for {whose} to wear yet. \
                     Skins, emotes and meshes are sold on the shelf."
                ))
                .boxed(),
            );
        } else {
            let mut list = vec![rows(
                now.choices
                    .iter()
                    .map(|choice| choose(&who, choice))
                    .collect(),
            )];
            list.extend(
                now.files
                    .iter()
                    .map(|file| note(file.clone()).boxed()),
            );
            parts.push(section(
                "WEARS",
                column(list).gap(6.0).boxed(),
            ));
            if now.emote
                && let Who::Agent { name, .. } = &who
            {
                let name = name.clone();
                buttons.push(action(
                    Glyph::Play,
                    "PLAY THE EMOTE",
                    move |world| wardrobe::play(world, &name),
                ));
            }
        }
        buttons.push(shop);
        if let Some((words, wrong)) = &now.said {
            notes.push(outcome(words.clone(), *wrong));
        }
        parts.insert(0, top(vec![head, deal(None, buttons, notes)]));
        page(
            "FITTING",
            to_everyone,
            shown.to_uppercase(),
            sections(parts),
        )
    })
    .within(bare())
    .boxed()
}

/// The choice of what `who` wears of one kind: its plain name, and
/// the dropdown at its end.
fn choose(who: &Who, choice: &Choice) -> AnyView {
    let names = std::iter::once("None".to_string())
        .chain(choice.options.iter().map(|(_, title)| title.clone()))
        .collect::<Vec<_>>();
    let ids = choice
        .options
        .iter()
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let (who, category) = (who.clone(), choice.category);
    row((
        column((label(choice.category.one()).size(13.0),))
            .fills_width(),
        dropdown(
            names,
            choice.chosen,
            Glyph::Chevron.image(ICON),
            move |world, at| {
                let id = at
                    .checked_sub(1)
                    .and_then(|at| ids.get(at))
                    .cloned();
                wardrobe::wear(world, &who, category, id);
            },
        ),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
    .boxed()
}
