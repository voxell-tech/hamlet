//! The shelf every stall is drawn as: what it is searched by, its
//! order and filters, a word of how many there are, its rows a page
//! at a time, and one listing on a page of its own.

use agents_ui::{breadcrumb, either, moved, short};
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, UiExt,
    ViewExt, column, derived, dropdown, each, field, icon_button,
    keyed, list, resource, row, scroll,
};

use crate::shelf::{Arranged, Listing, PAGES, Said, Shelf, Stock};
use crate::view::style::{
    Line, controls, meta, notice, page_head, prose, sections,
    status_line,
};
use crate::view::words::ago;

/// The most characters of a page's name on its breadcrumb.
const CRUMB_MOST: usize = 24;

/// A stall's listings as the panel draws them.
pub(crate) trait Shown: Listing {
    /// The stall's name on the way back to it.
    const BACK: &'static str;
    /// What the stall lists, for the words of how many there are:
    /// the plural, ending in `s`.
    const NOUN: &'static str;
    /// The words in the field while it is empty.
    const FIND: &'static str;
    /// Where the listings are asked for, where they are.
    const FROM: Option<&'static str> = None;

    /// Over the shelf.
    fn heading() -> AnyView {
        column(()).boxed()
    }

    /// The controls past the words and the order, laid after the
    /// order a few to a line.
    fn controls() -> Vec<AnyView> {
        Vec::new()
    }

    /// Asks for the listings again.
    fn again(_world: &mut World) {}

    /// The words of how many there are.
    fn summary(world: &World) -> String {
        summary::<Self>(world)
    }

    /// Its name, at the head of its page.
    fn title(&self) -> String;

    /// The meta line under its title on its page.
    fn byline(&self) -> String {
        String::new()
    }

    fn row(&self) -> Line;

    /// What its page shows under its head.
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

/// The way to narrow the shelf of `L` to one of `all`, or to show
/// `any` of them.
pub(crate) fn kinds<L, T>(
    any: &str,
    all: &'static [T],
    named: fn(T) -> &'static str,
) -> AnyView
where
    L: Listing<Filter = Option<T>>,
    T: Copy + PartialEq + Send + Sync + 'static,
{
    let mut options = vec![any.to_string()];
    options.extend(all.iter().map(|one| named(*one).to_string()));
    choice::<L>(
        options,
        move |shelf| {
            shelf
                .filter
                .and_then(|now| {
                    all.iter().position(|one| *one == now)
                })
                .map_or(0, |at| at + 1)
        },
        move |shelf, chosen| {
            shelf.filter = chosen.checked_sub(1).map(|at| all[at]);
        },
    )
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
    let mut narrow = vec![orders];
    narrow.extend(L::controls());
    let counted = derived(L::summary)
        .when(either(moved::<Stock<L>>(), moved::<Arranged<L>>()));
    let again = L::FROM.map(|_| {
        icon_button("restore", "LOOK AGAIN")
            .style(ButtonStyle::Quiet)
            .on_click(L::again)
            .boxed()
    });
    let rows = list((each(
        resource::<Arranged<L>, _>(|arranged| arranged.rows.clone()),
        |(listing, batch): &(L, u32)| (listing.id(), *batch),
        |(listing, _): &(L, u32)| shown::<L>(listing),
    ),));
    let more = keyed(
        resource::<Arranged<L>, _>(|arranged| arranged.more()),
        |more| more_rows::<L>(*more),
    );
    column((
        L::heading(),
        said::<L>(),
        find,
        controls(narrow),
        status_line(counted, again),
        scroll((rows, more)),
    ))
    .boxed()
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
            true => meta(format!(
                "{more} more. Type a few words to narrow them."
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
    let none = || {
        format!(
            "No {} match your search. Try fewer words, or another \
             filter.",
            L::NOUN
        )
    };
    match L::FROM {
        Some(from) if stock.asking => format!("Asking {from}..."),
        Some(from) if stock.items.is_empty() => {
            stock.problem.as_ref().map_or_else(
                || {
                    format!(
                        "No {} yet. Press Look again to ask {from}.",
                        L::NOUN
                    )
                },
                |why| format!("{why} Press Look again to try again."),
            )
        }
        Some(_) if found == 0 => none(),
        Some(from) => {
            let mut words = format!(
                "{} from {from}, fetched {}.",
                counted::<L>(found),
                ago(stock.fetched)
            );
            if let Some(why) = &stock.problem {
                words = format!("{words} Asking again failed: {why}");
            }
            words
        }
        None if found == 0 => none(),
        None => format!("{}.", counted::<L>(found)),
    }
}

/// `count` listings of `L`, in words.
pub(crate) fn counted<L: Shown>(count: usize) -> String {
    match count {
        1 => format!("1 {}", L::NOUN.trim_end_matches('s')),
        _ => format!("{count} {}", L::NOUN),
    }
}

/// The row of `listing`, which opens its page.
fn shown<L: Shown>(listing: &L) -> AnyView {
    let id = listing.id();
    listing.row().view(move |world| {
        browse::<L>(world, |shelf| shelf.open = Some(id.clone()));
        world.resource_mut::<Said<L>>().0 = None;
        L::opened(world, &id);
    })
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
            Some(listing) => (
                listing.title(),
                sections(vec![
                    page_head(&listing.title(), listing.byline()),
                    listing.page(),
                ]),
            ),
            None => (
                "...".to_string(),
                prose(format!(
                    "This {} is no longer listed. Go back to the {} \
                     to choose another.",
                    L::NOUN.trim_end_matches('s'),
                    L::NOUN
                ))
                .boxed(),
            ),
        };
        column((
            breadcrumb(
                vec![L::BACK.to_string()],
                short(&title, CRUMB_MOST),
                back,
            ),
            said::<L>(),
            // Room inside the scroll, as a wrapped paragraph can be
            // laid a little wider than the scroll is.
            scroll((column((body,)).padding(Space::Half),)),
        ))
        .boxed()
    })
    .boxed()
}

/// The outcome of the last thing done at the stall of `L`.
fn said<L: Listing>() -> AnyView {
    keyed(resource::<Said<L>, _>(|said| said.0.clone()), |said| {
        match said {
            Some((words, wrong)) => notice(words.clone(), *wrong),
            None => column(()).boxed(),
        }
    })
    .boxed()
}
