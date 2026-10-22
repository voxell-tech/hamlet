//! The shelf every stall is drawn as: what it is searched by, its
//! order and filters, a word of how many there are, its rows a page
//! at a time, and one listing on a page of its own.

use agents_ui::{breadcrumb, either, link_row, moved, name};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Button, ButtonProps, ButtonStyle, FrameProps, Space,
    Tone, UiExt, ViewExt, button, column, derived, description,
    dropdown, each, field, icon_button, keyed, label, list, resource,
    row, scroll,
};

use crate::shelf::{Arranged, Listing, PAGES, Said, Shelf, Stock};
use crate::view::words::ago;

/// A stall's listings as the panel draws them.
pub(crate) trait Shown: Listing {
    /// The stall's name on the way back to it.
    const BACK: &'static str;
    /// What the stall lists, for the words of how many there are.
    const NOUN: &'static str;
    /// The words in the field while it is empty.
    const FIND: &'static str;
    /// Where the listings are asked for, where they are.
    const FROM: Option<&'static str> = None;

    /// Over the shelf.
    fn heading() -> AnyView {
        column(()).boxed()
    }

    /// The controls past the words and the order.
    fn controls() -> Vec<AnyView> {
        Vec::new()
    }

    /// Asks for the listings again.
    fn again(_world: &mut World) {}

    /// The words of how many there are.
    fn summary(world: &World) -> String {
        summary::<Self>(world)
    }

    /// Its name, on its page's breadcrumb.
    fn title(&self) -> String;

    fn row(&self) -> AnyView;

    /// What its page shows under the breadcrumb.
    fn page(&self) -> AnyView;

    /// Asks for what its page shows.
    fn opened(_world: &mut World, _id: &Self::Id) {}
}

/// The stall of `L`: its shelf, or one listing on its page.
pub(crate) fn stall<L: Shown>() -> AnyView {
    keyed(
        resource::<Shelf<L>, _>(|shelf| shelf.open.clone()),
        |open| match open {
            Some(id) => page::<L>(id.clone()),
            None => shelf::<L>(),
        },
    )
    .boxed()
}

/// Changes what the person looks at, from the first page again.
pub(crate) fn browse<L: Listing>(
    world: &mut World,
    change: impl FnOnce(&mut Shelf<L>),
) {
    world.resource_mut::<Shelf<L>>().browse(change);
}

/// The way to choose among `options`, the one at `at` shown.
pub(crate) fn choice<L: Listing>(
    options: Vec<String>,
    at: impl Fn(&Shelf<L>) -> usize + Send + Sync + 'static,
    set: impl Fn(&mut Shelf<L>, usize) + Send + Sync + 'static,
) -> AnyView {
    dropdown(options, resource::<Shelf<L>, _>(move |s| at(s) as u32))
        .on_pick(move |world, chosen| {
            browse::<L>(world, |shelf| set(shelf, chosen as usize))
        })
        .boxed()
}

/// A word to press for each of `all`, and ALL, the one chosen lit,
/// narrowing the shelf of `L` to it.
pub(crate) fn chips<L, T>(
    all: &'static [T],
    named: fn(T) -> &'static str,
) -> AnyView
where
    L: Listing<Filter = Option<T>>,
    T: Copy + PartialEq + Send + Sync + 'static,
{
    keyed(resource::<Shelf<L>, _>(|shelf| shelf.filter), move |now| {
        let chip = |text: &str, on: bool, then: Option<T>| {
            button_of(text, on)
                .on_click(move |world| {
                    browse::<L>(world, |shelf| shelf.filter = then)
                })
                .boxed()
        };
        let mut chips = vec![chip("ALL", now.is_none(), None)];
        chips.extend(all.iter().map(|one| {
            chip(
                &named(*one).to_uppercase(),
                *now == Some(*one),
                Some(*one),
            )
        }));
        row(chips).space(Space::Half).boxed()
    })
    .boxed()
}

/// A button of a row of choices, lit when it is the one chosen.
fn button_of(text: &str, on: bool) -> Button {
    button(text.to_string()).style(if on {
        ButtonStyle::Primary
    } else {
        ButtonStyle::Quiet
    })
}

fn shelf<L: Shown>() -> AnyView {
    let orders = choice::<L>(
        L::ORDERS.iter().map(|(_, name)| name.to_string()).collect(),
        |shelf| {
            L::ORDERS
                .iter()
                .position(|(order, _)| *order == shelf.order)
                .unwrap_or(0)
        },
        |shelf, chosen| shelf.order = L::ORDERS[chosen].0,
    );
    // The field has a line of its own, so it has the panel's width
    // however narrow the panel is.
    let find =
        field(resource::<Shelf<L>, _>(|shelf| shelf.words.clone()))
            .placeholder(L::FIND)
            .on_commit(|world, words| {
                browse::<L>(world, |shelf| shelf.words = words)
            });
    let counted = derived(L::summary)
        .when(either(moved::<Stock<L>>(), moved::<Arranged<L>>()));
    let mut count = vec![description(counted).boxed()];
    if L::FROM.is_some() {
        count.push(
            icon_button("restore", "LOOK AGAIN")
                .style(ButtonStyle::Quiet)
                .on_click(L::again)
                .boxed(),
        );
    }
    let rows = list((each(
        resource::<Arranged<L>, _>(|arranged| arranged.rows.clone()),
        |(listing, batch): &(L, u32)| (listing.id(), *batch),
        |(listing, _): &(L, u32)| shown::<L>(listing),
    ),));
    let more = keyed(
        resource::<Arranged<L>, _>(|arranged| arranged.more()),
        |more| more_rows::<L>(*more),
    );
    let mut parts = vec![L::heading(), said::<L>(), find.boxed()];
    let mut controls = vec![orders.boxed()];
    controls.extend(L::controls());
    parts.push(column(controls).space(Space::Half).boxed());
    parts.push(row(count).boxed());
    parts.push(scroll((rows, more)).boxed());
    column(parts).boxed()
}

/// SHOW MORE while there are more rows and room for them, or how
/// many more there are.
fn more_rows<L: Shown>(more: usize) -> AnyView {
    if more == 0 {
        return column(()).boxed();
    }
    keyed(
        resource::<Shelf<L>, _>(|shelf| shelf.pages >= PAGES),
        move |all| match all {
            true => description(format!(
                "{more} more. Type words to narrow them."
            ))
            .boxed(),
            false => row((icon_button("more", "SHOW MORE")
                .style(ButtonStyle::Quiet)
                .on_click(|world| {
                    world.resource_mut::<Shelf<L>>().pages += 1;
                }),))
            .boxed(),
        },
    )
    .boxed()
}

/// The words of how many listings there are, and how fresh, or why
/// there are none.
pub(crate) fn summary<L: Shown>(world: &World) -> String {
    let stock = world.resource::<Stock<L>>();
    let found = world.resource::<Arranged<L>>().found;
    match L::FROM {
        Some(from) if stock.asking => format!("Asking {from}..."),
        Some(_) if stock.items.is_empty() => {
            stock.problem.as_ref().map_or_else(
                || format!("No {} yet. Choose Look Again.", L::NOUN),
                |why| why.clone(),
            )
        }
        Some(_) if found == 0 => {
            format!("No {} match your search.", L::NOUN)
        }
        Some(from) => {
            let mut words = format!(
                "{found} {} from {from}, asked {}.",
                L::NOUN,
                ago(stock.fetched)
            );
            if let Some(why) = &stock.problem {
                words = format!("{words} Asking again failed: {why}");
            }
            words
        }
        None if found == 0 => {
            format!("No {} match your search.", L::NOUN)
        }
        None => format!("{found} {}.", L::NOUN),
    }
}

/// The row of `listing`, which opens its page.
fn shown<L: Shown>(listing: &L) -> AnyView {
    let id = listing.id();
    link_row(listing.row())
        .on_click(move |world| {
            browse::<L>(world, |shelf| shelf.open = Some(id.clone()));
            world.resource_mut::<Said<L>>().0 = None;
            L::opened(world, &id);
        })
        .boxed()
}

/// The listing `id` on a page of its own, under the breadcrumb back
/// to its shelf.
fn page<L: Shown>(id: L::Id) -> AnyView {
    let known = derived(move |world: &World| {
        world.resource::<Stock<L>>().get(&id).cloned()
    })
    .when(moved::<Stock<L>>());
    keyed(known, |known| {
        let back = |world: &mut World, _: usize| {
            browse::<L>(world, |shelf| shelf.open = None);
        };
        let (title, body) = match known {
            Some(listing) => (listing.title(), listing.page()),
            None => (
                "...".to_string(),
                description(format!(
                    "This is not among the {} now.",
                    L::NOUN
                ))
                .boxed(),
            ),
        };
        column((
            breadcrumb(vec![L::BACK.to_string()], title, back),
            said::<L>(),
            scroll((body,)),
        ))
        .boxed()
    })
    .boxed()
}

/// The outcome of the last thing done at the stall of `L`.
fn said<L: Listing>() -> AnyView {
    keyed(resource::<Said<L>, _>(|said| said.0.clone()), |said| {
        match said {
            Some((words, wrong)) => label(words.clone())
                .tone(if *wrong {
                    Tone::Critical
                } else {
                    Tone::Accent
                })
                .boxed(),
            None => column(()).boxed(),
        }
    })
    .boxed()
}

/// The name of a part of a page, over it.
pub(crate) fn part(text: &str, body: AnyView) -> AnyView {
    column((name(text.to_string()), body))
        .space(Space::Half)
        .boxed()
}
