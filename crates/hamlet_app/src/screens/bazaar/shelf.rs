//! One shelf for every stall: the listings it has, narrowed by words
//! and a filter, put in order and shown a page at a time, and the
//! listing open on a page of its own.

use std::cmp::Ordering;
use std::fmt::Debug;
use std::marker::PhantomData;

use bevy::app::{App, PreUpdate};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Res, ResMut};

use crate::PanelSystems;
use crate::bazaar::BazaarSettings;

/// Keeps the shelf of the listings `L`: what it has, what the chief
/// looks at, and the rows that come of both.
pub(crate) fn plugin<L: Listing>(app: &mut App) {
    app.init_resource::<Stock<L>>()
        .init_resource::<Shelf<L>>()
        .init_resource::<Arranged<L>>()
        .init_resource::<Said<L>>()
        .add_systems(
            PreUpdate,
            arrange::<L>
                .run_if(
                    resource_changed::<Stock<L>>
                        .or_eager(resource_changed::<Shelf<L>>)
                        .or_eager(resource_changed::<BazaarSettings>),
                )
                .in_set(PanelSystems::Bind),
        );
}

/// One thing a stall sells, as its shelf finds and orders it.
pub(crate) trait Listing:
    Clone + PartialEq + Send + Sync + 'static
{
    type Id: Clone + PartialEq + Debug + Send + Sync + 'static;
    /// The filter of the shelf besides the words typed.
    type Filter: Clone + PartialEq + Default + Send + Sync + 'static;
    type Order: Copy + PartialEq + Default + Send + Sync + 'static;

    /// The orders the chief chooses among, by name.
    const ORDERS: &'static [(Self::Order, &'static str)];

    fn id(&self) -> Self::Id;

    /// All it is found by.
    fn told(&self) -> String;

    fn admits(&self, _filter: &Self::Filter) -> bool {
        true
    }

    /// Its place against `other` in `order`.
    fn order(&self, other: &Self, order: Self::Order) -> Ordering;
}

/// The listings on a stall's shelf, and how asking for them went.
#[derive(Resource)]
pub(crate) struct Stock<L: Listing> {
    pub items: Vec<L>,
    /// The count of times it was stocked, so that a row is built
    /// again with what it holds now.
    pub batch: u32,
    /// They were asked for once at least.
    pub asked: bool,
    /// They are being asked for.
    pub asking: bool,
    /// Why they could not be asked for, last time.
    pub problem: Option<String>,
    /// When they last came, in milliseconds since 1970.
    pub fetched: u64,
}

impl<L: Listing> Default for Stock<L> {
    fn default() -> Self {
        Stock {
            items: Vec::new(),
            batch: 0,
            asked: false,
            asking: false,
            problem: None,
            fetched: 0,
        }
    }
}

impl<L: Listing> Stock<L> {
    pub fn get(&self, id: &L::Id) -> Option<&L> {
        self.items.iter().find(|item| item.id() == *id)
    }

    /// Puts `items` on the shelf in place of what was there.
    pub fn stock(&mut self, items: Vec<L>) {
        self.items = items;
        self.batch += 1;
    }

    /// Marks them asked for, from now until they come.
    pub fn ask(&mut self) {
        self.asked = true;
        self.asking = true;
    }

    /// Takes in `items`, or why they could not be had, as they came
    /// at `now`.
    pub fn came(&mut self, items: Result<Vec<L>, String>, now: u64) {
        self.asking = false;
        match items {
            Ok(items) => {
                self.problem = None;
                self.fetched = now;
                self.stock(items);
            }
            Err(why) => self.problem = Some(why),
        }
    }
}

/// The chief's view of a stall's shelf.
#[derive(Resource)]
pub(crate) struct Shelf<L: Listing> {
    pub words: String,
    pub filter: L::Filter,
    pub order: L::Order,
    /// The pages of rows shown.
    pub pages: usize,
    /// The listing open on a page of its own.
    pub open: Option<L::Id>,
}

impl<L: Listing> Default for Shelf<L> {
    fn default() -> Self {
        Shelf {
            words: String::new(),
            filter: L::Filter::default(),
            order: L::Order::default(),
            pages: 1,
            open: None,
        }
    }
}

impl<L: Listing> Shelf<L> {
    /// Changes what is looked at, from the first page again.
    pub fn browse(&mut self, change: impl FnOnce(&mut Self)) {
        change(self);
        self.pages = 1;
    }
}

/// The rows a shelf shows, each a listing and the batch it came in,
/// and the count of listings found.
#[derive(Resource)]
pub(crate) struct Arranged<L: Listing> {
    pub rows: Vec<(L, u32)>,
    pub found: usize,
}

impl<L: Listing> Default for Arranged<L> {
    fn default() -> Self {
        Arranged {
            rows: Vec::new(),
            found: 0,
        }
    }
}

impl<L: Listing> Arranged<L> {
    /// The listings found past the rows shown.
    pub fn more(&self) -> usize {
        self.found - self.rows.len()
    }
}

/// The outcome of the last thing done at the stall of `L`, and
/// whether it went wrong.
#[derive(Resource)]
pub(crate) struct Said<L>(
    pub Option<(String, bool)>,
    PhantomData<fn() -> L>,
);

impl<L> Default for Said<L> {
    fn default() -> Self {
        Said(None, PhantomData)
    }
}

impl<L> Said<L> {
    pub fn say(&mut self, words: impl Into<String>, wrong: bool) {
        self.0 = Some((words.into(), wrong));
    }
}

/// The time now, in milliseconds since 1970.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// The listings of `items` the words and the filter keep, in order.
pub(crate) fn arranged_of<'a, L: Listing>(
    items: &'a [L],
    words: &str,
    filter: &L::Filter,
    order: L::Order,
) -> Vec<&'a L> {
    let words = words.to_lowercase();
    let words = words.split_whitespace().collect::<Vec<_>>();
    let mut shown = items
        .iter()
        .filter(|item| item.admits(filter))
        .filter(|item| {
            let told = item.told().to_lowercase();
            words.iter().all(|word| told.contains(word))
        })
        .collect::<Vec<_>>();
    shown.sort_by(|a, b| a.order(b, order));
    shown
}

fn arrange<L: Listing>(
    set: Res<BazaarSettings>,
    stock: Res<Stock<L>>,
    shelf: Res<Shelf<L>>,
    mut arranged: ResMut<Arranged<L>>,
) {
    let all = arranged_of(
        &stock.items,
        &shelf.words,
        &shelf.filter,
        shelf.order,
    );
    let rows = all
        .iter()
        .take(set.page * shelf.pages.min(set.pages))
        .map(|item| ((*item).clone(), stock.batch))
        .collect::<Vec<_>>();
    if arranged.rows != rows || arranged.found != all.len() {
        *arranged = Arranged {
            rows,
            found: all.len(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listing of a name and a number, for the shelf alone.
    #[derive(Clone, PartialEq)]
    struct Lantern(&'static str, u32);

    impl Listing for Lantern {
        type Id = String;
        type Filter = Option<u32>;
        type Order = bool;

        const ORDERS: &'static [(bool, &'static str)] = &[];

        fn id(&self) -> String {
            self.0.to_string()
        }

        fn told(&self) -> String {
            format!("{} lantern", self.0)
        }

        fn admits(&self, most: &Option<u32>) -> bool {
            most.is_none_or(|most| self.1 <= most)
        }

        fn order(&self, other: &Self, rising: bool) -> Ordering {
            match rising {
                true => self.1.cmp(&other.1),
                false => other.1.cmp(&self.1),
            }
        }
    }

    #[test]
    fn a_shelf_finds_by_every_word_and_keeps_its_filter() {
        let all = [
            Lantern("Wick", 3),
            Lantern("Quill", 1),
            Lantern("Bellows", 2),
        ];
        let names = |words: &str, most: Option<u32>, rising: bool| {
            arranged_of(&all, words, &most, rising)
                .iter()
                .map(|lantern| lantern.0)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names("", None, false),
            ["Wick", "Bellows", "Quill"]
        );
        assert_eq!(names("LANTERN qu", None, false), ["Quill"]);
        assert_eq!(names("", Some(2), true), ["Quill", "Bellows"]);
    }
}
