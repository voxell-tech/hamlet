//! What chiefs sell one another at the bazaar's server, each a
//! listing on the shelf of its stall: agents at AGENTS, tools at
//! PLUGINS, and whole hamlets at HAMLETS.

use std::cmp::Ordering;
use std::fmt::Debug;
use std::marker::PhantomData;

use crate::bazaar::market::lot::{Kind, Lot};
use crate::bazaar::samples::agents::Craft;
use crate::bazaar::samples::plugins::Kind as Shelf;
use crate::bazaar::samples::{ORDERS, Order};
use crate::bazaar::shelf::Listing;

/// The part of each sale that goes to its seller, in percent: the
/// server's platform fee is ten.
pub(crate) const SELLERS_SHARE: u32 = 90;

/// A kind of listing the bazaar's server sells, and how its stall
/// names and narrows it.
pub(crate) trait Sells: Send + Sync + 'static {
    const KIND: Kind;
    /// The stall's name on the way back to it.
    const BACK: &'static str;
    /// What the stall sells, beside its name.
    const ABOUT: &'static str;
    /// What the stall lists, for the words of how many there are.
    const NOUN: &'static str;
    const FIND: &'static str;

    /// The category a listing is narrowed by.
    type Category: Copy + PartialEq + Debug + Send + Sync + 'static;

    /// The categories chosen among; none for a kind not narrowed.
    const CATEGORIES: &'static [Self::Category];

    fn category(category: Self::Category) -> &'static str;
}

pub(crate) struct Agents;
pub(crate) struct Tools;
pub(crate) struct Hamlets;

impl Sells for Agents {
    const KIND: Kind = Kind::Agent;
    const BACK: &'static str = "AGENTS";
    const ABOUT: &'static str = "Agents other chiefs made, to hire.";
    const NOUN: &'static str = "agents";
    const FIND: &'static str =
        "Search by an agent's name or what it does.";
    type Category = Craft;
    const CATEGORIES: &'static [Craft] = &Craft::ALL;

    fn category(craft: Craft) -> &'static str {
        craft.name()
    }
}

impl Sells for Tools {
    const KIND: Kind = Kind::Plugin;
    const BACK: &'static str = "PLUGINS";
    const ABOUT: &'static str = "What others made to add to Hamlet.";
    const NOUN: &'static str = "plugins";
    const FIND: &'static str =
        "Search by a plugin's name or what it adds.";
    type Category = Shelf;
    const CATEGORIES: &'static [Shelf] = &Shelf::ALL;

    fn category(shelf: Shelf) -> &'static str {
        shelf.name()
    }
}

impl Sells for Hamlets {
    const KIND: Kind = Kind::Hamlet;
    const BACK: &'static str = "HAMLETS";
    const ABOUT: &'static str = "Whole hamlets, to found at once.";
    const NOUN: &'static str = "hamlets";
    const FIND: &'static str =
        "Search by a hamlet's name or what it is for.";
    // A hamlet's kind is its seller's own words.
    type Category = Craft;
    const CATEGORIES: &'static [Craft] = &[];

    fn category(craft: Craft) -> &'static str {
        craft.name()
    }
}

/// A listing of the server's on the shelf of the stall of `K`.
pub(crate) struct Wares<K>(pub Lot, PhantomData<fn() -> K>);

impl<K> Wares<K> {
    pub fn new(lot: Lot) -> Self {
        Wares(lot, PhantomData)
    }
}

impl<K> Clone for Wares<K> {
    fn clone(&self) -> Self {
        Wares::new(self.0.clone())
    }
}

impl<K> PartialEq for Wares<K> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<K: Sells> Listing for Wares<K> {
    type Id = String;
    type Filter = Option<K::Category>;
    type Order = Order;

    const ORDERS: &'static [(Order, &'static str)] = ORDERS;

    fn id(&self) -> String {
        self.0.id.clone()
    }

    fn told(&self) -> String {
        let lot = &self.0;
        format!(
            "{} {} {} {} {} {} {}",
            lot.title,
            lot.summary,
            lot.category,
            lot.tags.join(" "),
            lot.seller.login,
            lot.seller.shown(),
            lot.description,
        )
    }

    fn admits(&self, category: &Option<K::Category>) -> bool {
        category.is_none_or(|category| {
            K::category(category)
                .eq_ignore_ascii_case(self.0.category.trim())
        })
    }

    fn order(&self, other: &Self, order: Order) -> Ordering {
        rank(&self.0, &other.0, order)
            .then_with(|| self.0.title.cmp(&other.0.title))
    }
}

/// The place of `one` against `other` in `order`, as the server puts
/// them.
pub(crate) fn rank(one: &Lot, other: &Lot, order: Order) -> Ordering {
    match order {
        Order::Popular => other.owners.cmp(&one.owners),
        Order::Rated => other
            .stars
            .total_cmp(&one.stars)
            .then(other.reviews.cmp(&one.reviews)),
        Order::Cheapest => one.price_cents.cmp(&other.price_cents),
        Order::Newest => other.updated_at.cmp(&one.updated_at),
    }
}
