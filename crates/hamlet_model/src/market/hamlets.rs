//! Whole hamlets for sale, a mayor and its agents with all they
//! know, as the Hamlet Store shows them.

use serde::{Deserialize, Serialize};

use super::lot::{AgentMeta, Lot};
use super::{Price, Seller, Standing};

/// A hamlet for sale, as it would be founded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    /// Its listing's id at the bazaar.
    pub id: String,
    /// Its name on disk and in commands.
    pub name: String,
    /// The name it is shown by.
    pub title: String,
    /// What kind of hamlet it is, in a few words.
    pub kind: String,
    pub summary: String,
    pub description: String,
    /// The path of its cover on the bazaar's server.
    pub cover: Option<String>,
    /// What it is for, as its mayor is told.
    pub purpose: String,
    pub seller: Seller,
    pub price: Price,
    pub standing: Standing,
    pub mayor: Settler,
    /// Its agents besides the mayor.
    pub agents: Vec<Settler>,
    /// The seed its town's looks come from.
    pub seed: u64,
    pub version: String,
}

/// An agent of a hamlet for sale.
#[derive(
    Clone, Debug, PartialEq, Default, Serialize, Deserialize,
)]
pub struct Settler {
    pub name: String,
    pub purpose: String,
    /// The names of its skills.
    pub skills: Vec<String>,
    /// It comes with its notes.
    pub notes: bool,
}

impl Settler {
    fn of(meta: AgentMeta) -> Settler {
        Settler {
            name: meta.name,
            purpose: meta.purpose,
            skills: meta.skills,
            notes: meta.notes,
        }
    }
}

impl Listing {
    /// The hamlet `lot` sells.
    pub fn of(lot: &Lot) -> Listing {
        let meta = lot
            .latest
            .as_ref()
            .and_then(|latest| latest.hamlet())
            .unwrap_or_default();
        let seed = meta.seed();
        Listing {
            id: lot.id.clone(),
            name: meta.name,
            title: lot.title.clone(),
            kind: match meta.kind.is_empty() {
                true => lot.category.clone(),
                false => meta.kind,
            },
            summary: lot.summary.clone(),
            description: lot.description.clone(),
            cover: lot.cover_url.clone(),
            purpose: meta.purpose,
            seller: lot.seller.seller(),
            price: lot.price(),
            standing: lot.standing(),
            mayor: Settler::of(meta.mayor),
            agents: meta
                .agents
                .into_iter()
                .map(Settler::of)
                .collect(),
            seed,
            version: lot.version().to_string(),
        }
    }

    /// Everyone who comes with it, the mayor first.
    pub fn everyone(&self) -> impl Iterator<Item = &Settler> {
        std::iter::once(&self.mayor).chain(&self.agents)
    }
}
