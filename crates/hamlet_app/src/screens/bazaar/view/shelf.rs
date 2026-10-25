//! The shelf every stall is drawn as, the way the Inspector shows its
//! hamlets: the stall's head, what it is searched by, its order and
//! filters, a word of how many there are, its rows a page at a time,
//! and one listing on a page of its own under the way back.

use std::marker::PhantomData;

use bevy::app::{App, Update};
use bevy::ecs::component::Component;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::any_with_component;
use bevy::ecs::system::{Query, ResMut};
use bevy::ecs::world::World;
use bevy::text::EditableText;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, dropdown, label, row, scroll,
    text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, derived, each, keyed, resource,
};
use hamlet_ui::styles::{
    Fills, ICON, action, bare, filling, note, text_input,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, Cx};

use super::parts::{
    Line, crumb, nothing, outcome, problem, shelf_head, short,
};
use super::words::ago;
use crate::bazaar::BazaarSettings;
use crate::bazaar::shelf::{Arranged, Listing, Said, Shelf, Stock};
use crate::roll::{either, moved};

/// The most characters of a page's name on the way back.
const CRUMB_MOST: usize = 24;

/// The controls laid on one line under the search.
const CONTROLS_A_LINE: usize = 2;

/// Follows what the stall of `L` is searched for as it is typed,
/// while its field is there.
pub(crate) fn typing<L: Listing>(app: &mut App) {
    app.add_systems(
        Update,
        read_words::<L>.run_if(any_with_component::<Finding<L>>),
    );
}

/// A stall's listings as the panel draws them.
pub(crate) trait Shown: Listing {
    /// The stall's name, at the head of its shelf and on the way back
    /// to it.
    const BACK: &'static str;
    /// What the stall sells, beside its name.
    const ABOUT: &'static str;
    /// What the stall lists, for the words of how many there are:
    /// the plural, ending in `s`.
    const NOUN: &'static str;
    /// The words under the search field.
    const FIND: &'static str;
    /// Where the listings are asked for, where they are.
    const FROM: Option<&'static str> = None;

    /// Under the head of the shelf, what waits for the chief.
    fn notice() -> AnyView {
        nothing()
    }

    /// The controls past the words: the order and the filters, laid
    /// under the search a few to a line.
    fn controls() -> Vec<AnyView> {
        Vec::new()
    }

    /// Asks for the listings again.
    fn again(_world: &mut World) {}

    /// What asking again does, beside the button that does it.
    fn again_words() -> String {
        Self::FROM.map_or_else(String::new, |from| {
            format!("Asks {from} for its {} again.", Self::NOUN)
        })
    }

    /// The words of how many there are.
    fn summary(world: &World) -> String {
        summary::<Self>(world)
    }

    /// Why the listings could not be asked for, last time.
    fn trouble(world: &World) -> Option<String> {
        world.resource::<Stock<Self>>().problem.clone()
    }

    /// Its name, at the head of its page.
    fn title(&self) -> String;

    fn row(&self) -> Line;

    /// Its page: its head, the way it is had, and its parts.
    fn page(&self) -> AnyView;

    /// Asks for what its page shows.
    fn opened(_world: &mut World, _id: &Self::Id) {}
}

/// The field a stall is searched in.
#[derive(Component)]
pub(crate) struct Finding<L: Listing>(PhantomData<fn() -> L>);

fn read_words<L: Listing>(
    field: Query<&Children, With<Finding<L>>>,
    texts: Query<&EditableText>,
    mut shelf: ResMut<Shelf<L>>,
) {
    let typed = field
        .iter()
        .flat_map(|children| children.iter())
        .find_map(|child| texts.get(*child).ok());
    if let Some(text) = typed
        && text.value() != shelf.words.as_str()
    {
        let words = text.value().to_string();
        shelf.browse(|shelf| shelf.words = words);
    }
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
    .within(filling())
    .boxed()
}

/// Changes what the chief looks at, from the first page again.
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
    dropdown(
        options,
        resource::<Shelf<L>, _>(at),
        Glyph::Chevron.image(ICON),
        move |world, chosen| {
            browse::<L>(world, |shelf| set(shelf, chosen));
        },
    )
    .width(percent(100.0))
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

/// The shelf of `L`, as the Inspector shows its hamlets: the head,
/// the search with the order and the filters under it, how many there
/// are, the rows, and the way to ask for them again at its foot.
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
    let find = column((
        text_field(
            resource::<Shelf<L>, _>(|shelf| shelf.words.clone()),
            |world, words| {
                browse::<L>(world, |shelf| shelf.words = words);
            },
        )
        .rules(text_input)
        .tagged(Finding::<L>(PhantomData)),
        label(L::FIND).size(11.0).tone(Tone::Dim),
    ))
    .gap(6.0);
    let mut narrow = vec![orders];
    narrow.extend(L::controls());
    let counted = derived(L::summary)
        .when(either(moved::<Stock<L>>(), moved::<Arranged<L>>()));
    let rows = each(
        resource::<Arranged<L>, _>(|arranged| arranged.rows.clone()),
        |(listing, batch): &(L, u32)| (listing.id(), *batch),
        |(listing, _): &(L, u32)| shown::<L>(listing),
    )
    .within(column(()).gap(2.0));
    let more =
        keyed(resource::<Arranged<L>, _>(Arranged::more), |more| {
            more_rows::<L>(*more)
        })
        .within(bare());
    column((
        shelf_head(L::BACK, L::ABOUT),
        L::notice(),
        find,
        controls(narrow),
        note(counted),
        said::<L>(),
        scroll((rows, more)).fills_height(),
        again::<L>(),
    ))
    .gap(10.0)
    .fills_height()
    .boxed()
}

/// `controls` laid a few to a line, each an even share of it.
fn controls(controls: Vec<AnyView>) -> AnyView {
    let mut lines = Vec::new();
    let mut controls = controls.into_iter().peekable();
    while controls.peek().is_some() {
        let line = controls
            .by_ref()
            .take(CONTROLS_A_LINE)
            .map(|control| column((control,)).fills_width().boxed())
            .collect::<Vec<_>>();
        lines.push(
            row(line)
                .gap(8.0)
                .align(AlignItems::Center)
                .width(percent(100.0))
                .boxed(),
        );
    }
    column(lines).gap(8.0).boxed()
}

/// LOOK AGAIN at the foot of a shelf that is asked for, with what it
/// does beside it, or why asking failed in red.
fn again<L: Shown>() -> AnyView {
    if L::FROM.is_none() {
        return nothing();
    }
    let trouble = derived(L::trouble).when(moved::<Stock<L>>());
    let beside = keyed(trouble, move |trouble| match trouble {
        Some(why) => problem(why.clone()),
        None => note(L::again_words()).boxed(),
    })
    .within(bare());
    row((
        action(Glyph::Restore, "LOOK AGAIN", L::again),
        column((beside,)).fills_width(),
    ))
    .gap(12.0)
    .align(AlignItems::Center)
    .boxed()
}

/// SHOW MORE while there are more rows and room for them, or how
/// many more there are.
fn more_rows<L: Shown>(more: usize) -> AnyView {
    if more == 0 {
        return nothing();
    }
    AnyView::new(move |cx: &mut Cx| {
        let set = cx.world.resource::<BazaarSettings>();
        let all = cx.world.resource::<Shelf<L>>().pages >= set.pages;
        cx.build(match all {
            true => note(format!(
                "{more} more. Type a few words to narrow them."
            ))
            .boxed(),
            false => {
                row((action(Glyph::More, "SHOW MORE", |world| {
                    world.resource_mut::<Shelf<L>>().pages += 1;
                }),))
                .padding(UiRect::top(px(6.0)))
                .boxed()
            }
        })
    })
}

/// The words of how many listings there are, and how fresh.
pub(crate) fn summary<L: Shown>(world: &World) -> String {
    let stock = world.resource::<Stock<L>>();
    let found = world.resource::<Arranged<L>>().found;
    match L::FROM {
        Some(from) if stock.asking => format!("Asking {from}..."),
        Some(_) if stock.items.is_empty() => {
            format!("No {} yet.", L::NOUN)
        }
        _ if found == 0 => format!(
            "No {} match your search. Try fewer words, or another \
             filter.",
            L::NOUN
        ),
        Some(from) => format!(
            "{} from {from}, fetched {}.",
            counted::<L>(found),
            ago(stock.fetched)
        ),
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

/// Closes the page open at the stall of `L`.
fn close<L: Listing>(world: &mut World) {
    browse::<L>(world, |shelf| shelf.open = None);
}

/// The listing `id` on a page of its own, under the way back to its
/// shelf.
fn page<L: Shown>(id: L::Id) -> AnyView {
    let known = derived(move |world: &World| {
        world.resource::<Stock<L>>().get(&id).cloned()
    })
    .when(moved::<Stock<L>>());
    keyed(known, |known| {
        let (title, body) = match known {
            Some(listing) => (listing.title(), listing.page()),
            None => (
                "...".to_string(),
                note(format!(
                    "This {} is no longer listed. Go back to the {} \
                     to choose another.",
                    L::NOUN.trim_end_matches('s'),
                    L::NOUN
                ))
                .boxed(),
            ),
        };
        column((
            crumb(
                L::BACK,
                close::<L>,
                short(&title, CRUMB_MOST).to_uppercase(),
            ),
            scroll((body,)).fills_height(),
        ))
        .gap(14.0)
        .fills_height()
        .boxed()
    })
    .within(filling())
    .boxed()
}

/// What came of the last thing done at the stall of `L`.
pub(crate) fn said<L: Listing>() -> AnyView {
    keyed(resource::<Said<L>, _>(|said| said.0.clone()), |said| {
        match said {
            Some((words, wrong)) => outcome(words.clone(), *wrong),
            None => nothing(),
        }
    })
    .within(bare())
    .boxed()
}
