//! Agents other chiefs made, ready to hire, as the Artisans' Stall
//! shows them.

use serde::{Deserialize, Serialize};

use super::lot::{Detail, Lot};
use super::{Listing, Price, Seller, Standing};

/// An agent a chief made by hand, for hire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hireling {
    /// Its listing's id at the bazaar.
    pub id: String,
    pub name: String,
    pub maker: Seller,
    /// What it is for, in a line.
    pub purpose: String,
    pub description: String,
    /// The path of its cover on the bazaar's server.
    pub cover: Option<String>,
    /// How it thinks and speaks.
    pub character: String,
    pub skills: Vec<String>,
    /// The model its maker made it on.
    pub model: Option<String>,
    pub craft: Option<Craft>,
    pub tags: Vec<String>,
    pub price: Price,
    pub standing: Standing,
    pub reviews: Vec<Review>,
    pub version: String,
    /// The day it last changed, as `YYYY-MM-DD`.
    pub updated: String,
}

impl Hireling {
    /// The agent `lot` sells.
    pub fn of(lot: &Lot) -> Hireling {
        let meta = lot
            .latest
            .as_ref()
            .and_then(|latest| latest.agent())
            .unwrap_or_default();
        Hireling {
            id: lot.id.clone(),
            name: lot.title.clone(),
            maker: lot.seller.seller(),
            purpose: lot.summary.clone(),
            description: lot.description.clone(),
            cover: lot.cover_url.clone(),
            character: meta.character,
            skills: meta.skills,
            model: meta.model,
            craft: Craft::named(&lot.category),
            tags: lot.tags.clone(),
            price: lot.price(),
            standing: lot.standing(),
            reviews: Vec::new(),
            version: lot.version().to_string(),
            updated: lot.updated().to_string(),
        }
    }

    /// The agent of `detail`, with what its owners said.
    pub fn of_detail(detail: &Detail) -> Hireling {
        Hireling {
            reviews: Review::all(detail),
            ..Hireling::of(&detail.listing)
        }
    }
}

/// The kind of work a hireling does.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub enum Craft {
    Writing,
    Code,
    Research,
    Trade,
    Household,
    Teaching,
}

impl Craft {
    pub const ALL: [Craft; 6] = [
        Craft::Writing,
        Craft::Code,
        Craft::Research,
        Craft::Trade,
        Craft::Household,
        Craft::Teaching,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Craft::Writing => "Writing",
            Craft::Code => "Code",
            Craft::Research => "Research",
            Craft::Trade => "Business",
            Craft::Household => "Personal",
            Craft::Teaching => "Learning",
        }
    }

    /// The craft a listing's category names.
    pub fn named(name: &str) -> Option<Craft> {
        Craft::ALL.into_iter().find(|craft| {
            craft.name().eq_ignore_ascii_case(name.trim())
        })
    }
}

/// What one chief who hired a listing said of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub by: String,
    /// Out of five.
    pub stars: u8,
    pub words: String,
}

impl Review {
    /// The reviews of `detail`, the newest first.
    pub fn all(detail: &Detail) -> Vec<Review> {
        detail
            .reviews
            .iter()
            .map(|said| Review {
                by: said.by.login.clone(),
                stars: said.stars,
                words: said.words.clone(),
            })
            .collect()
    }
}

impl Listing for Hireling {
    fn name(&self) -> &str {
        &self.name
    }

    fn told(&self) -> String {
        format!(
            "{} {} {} {} {} {} {}",
            self.name,
            self.maker.name,
            self.maker.login,
            self.purpose,
            self.craft.map_or("", Craft::name),
            self.tags.join(" "),
            self.skills.join(" ")
        )
    }

    fn price(&self) -> Price {
        self.price
    }

    fn standing(&self) -> Standing {
        self.standing
    }

    fn updated(&self) -> &str {
        &self.updated
    }
}
