//! The bazaar's listings besides models: agents chiefs made, plugins
//! and whole hamlets, each with who sells it and what it costs. They
//! are sold through the bazaar's server, hamlet-server.

pub mod account;
pub mod artisans;
pub mod client;
pub mod hamlets;
pub mod lot;
pub mod package;
pub mod plugins;
pub mod samples;

use serde::{Deserialize, Serialize};

/// The one who sells a listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seller {
    pub name: String,
    /// Their GitHub login.
    #[serde(default)]
    pub login: String,
    /// The bazaar has checked who they are.
    pub verified: bool,
}

/// What a listing costs, in dollars, paid once.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Price {
    Free,
    Once(f64),
}

impl Price {
    /// The price of `cents`, as the server gives it.
    pub fn of_cents(cents: u32) -> Price {
        match cents {
            0 => Price::Free,
            cents => Price::Once(f64::from(cents) / 100.0),
        }
    }

    pub fn cents(self) -> u32 {
        match self {
            Price::Free => 0,
            Price::Once(dollars) => (dollars * 100.0).round() as u32,
        }
    }

    /// The price in a few words.
    pub fn words(self) -> String {
        match self {
            Price::Free => "Free".into(),
            Price::Once(dollars) => format!("${dollars:.2}"),
        }
    }
}

/// What those who bought a listing thought of it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Standing {
    /// Out of five.
    pub stars: f32,
    pub reviews: u32,
    /// How many have it.
    pub owners: u32,
}

/// The part of each sale that goes to its seller, in percent.
pub const SELLERS_SHARE: u32 = 90;

/// A listing as it is found and ordered at its stall.
pub trait Listing {
    fn name(&self) -> &str;
    /// All it can be found by, its name too.
    fn told(&self) -> String;
    fn price(&self) -> Price;
    fn standing(&self) -> Standing;
    /// The day it last changed, as `YYYY-MM-DD`.
    fn updated(&self) -> &str;
}

/// The order listings are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Order {
    #[default]
    Popular,
    Rated,
    Cheapest,
    Newest,
}

impl Order {
    pub const ALL: [Order; 4] = [
        Order::Popular,
        Order::Rated,
        Order::Cheapest,
        Order::Newest,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Order::Popular => "Most popular",
            Order::Rated => "Best rated",
            Order::Cheapest => "Cheapest",
            Order::Newest => "Newest",
        }
    }
}

/// The listings of `all` that `keep` keeps and that tell of every one
/// of `words`, in `order`. Ties go by name.
pub fn shown<'a, T: Listing>(
    all: &'a [T],
    words: &str,
    keep: impl Fn(&T) -> bool,
    order: Order,
) -> Vec<&'a T> {
    let words = words.to_lowercase();
    let mut shown = all
        .iter()
        .filter(|listing| keep(listing))
        .filter(|listing| {
            let told = listing.told().to_lowercase();
            words.split_whitespace().all(|word| told.contains(word))
        })
        .collect::<Vec<_>>();
    shown.sort_by(|a, b| {
        let (one, other) = (a.standing(), b.standing());
        match order {
            Order::Popular => other.owners.cmp(&one.owners),
            Order::Rated => other
                .stars
                .total_cmp(&one.stars)
                .then(other.reviews.cmp(&one.reviews)),
            Order::Cheapest => {
                a.price().cents().cmp(&b.price().cents())
            }
            Order::Newest => b.updated().cmp(a.updated()),
        }
        .then_with(|| a.name().cmp(b.name()))
    });
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stocked(&'static str, Price, f32, u32, &'static str);

    impl Listing for Stocked {
        fn name(&self) -> &str {
            self.0
        }

        fn told(&self) -> String {
            format!("{} lantern", self.0)
        }

        fn price(&self) -> Price {
            self.1
        }

        fn standing(&self) -> Standing {
            Standing {
                stars: self.2,
                reviews: self.3,
                owners: self.3 * 10,
            }
        }

        fn updated(&self) -> &str {
            self.4
        }
    }

    fn names(shown: Vec<&Stocked>) -> Vec<&str> {
        shown.iter().map(|listing| listing.0).collect()
    }

    #[test]
    fn listings_are_found_by_every_word_and_ordered() {
        let all = [
            Stocked("Wick", Price::Once(12.0), 4.5, 30, "2026-09-01"),
            Stocked(
                "Quill",
                Price::Once(10.0),
                4.9,
                12,
                "2026-10-02",
            ),
            Stocked("Bellows", Price::Free, 4.9, 40, "2025-12-30"),
        ];
        let all_of = |order| names(shown(&all, "", |_| true, order));
        assert_eq!(
            all_of(Order::Cheapest),
            ["Bellows", "Quill", "Wick"]
        );
        assert_eq!(
            all_of(Order::Rated),
            ["Bellows", "Quill", "Wick"]
        );
        assert_eq!(
            all_of(Order::Popular),
            ["Bellows", "Wick", "Quill"]
        );
        assert_eq!(
            all_of(Order::Newest),
            ["Quill", "Wick", "Bellows"]
        );
        let found =
            shown(&all, "LANTERN qu", |_| true, Order::Popular);
        assert_eq!(names(found), ["Quill"]);
        let kept =
            shown(&all, "", |l| l.1 != Price::Free, Order::Popular);
        assert_eq!(names(kept), ["Wick", "Quill"]);
    }
}
