//! The listings of the stalls that have no market behind them yet:
//! agents other chiefs made, plugins and whole hamlets. Made up, to
//! show the stalls as they will be.

pub(crate) mod agents;
pub(crate) mod hamlets;
pub(crate) mod plugins;

use std::cmp::Ordering;

use hamlet_engine_bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::samples::agents::{HIRELINGS, Hireling};
use crate::samples::hamlets::{FOR_SALE, Template};
use crate::samples::plugins::{CONTRAPTIONS, Contraption};
use crate::shelf::{self, Stock};

/// The part of each sale that goes to its seller, in percent.
pub(crate) const SELLERS_SHARE: u32 = 80;

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins((
        shelf::plugin::<Hireling>,
        shelf::plugin::<Contraption>,
        shelf::plugin::<Template>,
    ))
    .add_systems(Startup, stock);
}

fn stock(
    mut hirelings: ResMut<Stock<Hireling>>,
    mut contraptions: ResMut<Stock<Contraption>>,
    mut templates: ResMut<Stock<Template>>,
) {
    hirelings.stock(HIRELINGS.to_vec());
    contraptions.stock(CONTRAPTIONS.to_vec());
    templates.stock(FOR_SALE.to_vec());
}

/// The one who sells a listing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Seller {
    pub name: &'static str,
    /// The bazaar has checked who they are.
    pub verified: bool,
}

pub(crate) const fn seller(
    name: &'static str,
    verified: bool,
) -> Seller {
    Seller { name, verified }
}

/// The price of a listing, in dollars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Price {
    Free,
    Once(f64),
    Monthly(f64),
}

impl Price {
    /// What it costs over a year, to set a price paid once beside one
    /// paid each month.
    fn yearly(self) -> f64 {
        match self {
            Price::Free => 0.0,
            Price::Once(dollars) => dollars,
            Price::Monthly(dollars) => dollars * 12.0,
        }
    }
}

/// The standing of a listing with those who bought it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Standing {
    /// Out of five.
    pub stars: f32,
    pub reviews: u32,
    /// How many have it.
    pub owners: u32,
}

pub(crate) const fn standing(
    stars: f32,
    reviews: u32,
    owners: u32,
) -> Standing {
    Standing {
        stars,
        reviews,
        owners,
    }
}

/// A review of a listing by one chief who had it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Review {
    pub by: &'static str,
    /// Out of five.
    pub stars: u8,
    pub words: &'static str,
}

/// The order listings are shown in.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize,
)]
pub(crate) enum Order {
    #[default]
    Popular,
    Rated,
    Cheapest,
    Newest,
}

/// The orders of a stall of listings, by name.
pub(crate) const ORDERS: &[(Order, &str)] = &[
    (Order::Popular, "Most popular"),
    (Order::Rated, "Best rated"),
    (Order::Cheapest, "Cheapest"),
    (Order::Newest, "Newest"),
];

/// The keys a listing is ordered by: its price, its standing and the
/// day it last changed, as `YYYY-MM-DD`.
pub(crate) type Ranked = (Price, Standing, &'static str);

/// The place of a listing `one` against `other` in `order`.
pub(crate) fn rank(
    one: Ranked,
    other: Ranked,
    order: Order,
) -> Ordering {
    let ((price, mine, day), (their_price, theirs, their_day)) =
        (one, other);
    match order {
        Order::Popular => theirs.owners.cmp(&mine.owners),
        Order::Rated => theirs
            .stars
            .total_cmp(&mine.stars)
            .then(theirs.reviews.cmp(&mine.reviews)),
        Order::Cheapest => {
            price.yearly().total_cmp(&their_price.yearly())
        }
        Order::Newest => their_day.cmp(day),
    }
}
