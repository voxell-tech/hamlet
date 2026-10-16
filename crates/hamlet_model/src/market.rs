//! The bazaar's listings besides models: agents chiefs made, plugins
//! and whole hamlets, each with who sells it and what it costs. Mock
//! data for now.

pub mod artisans;
pub mod hamlets;
pub mod plugins;

use serde::{Deserialize, Serialize};

/// The one who sells a listing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seller {
    pub name: String,
    /// The bazaar has checked who they are.
    pub verified: bool,
}

/// What a listing costs, in dollars.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Price {
    Free,
    Once(f64),
    Monthly(f64),
}

impl Price {
    /// The price in a few words.
    pub fn words(self) -> String {
        match self {
            Price::Free => "Free".into(),
            Price::Once(dollars) => format!("${dollars:.2}"),
            Price::Monthly(dollars) => {
                format!("${dollars:.2} a month")
            }
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
