//! The details of a draft as they are typed, checked and saved.

use bevy::ecs::world::World;

use super::Desk;
use crate::bazaar::covers::{Pick, Picked};
use crate::bazaar::market::drafts::{Drafts, Then, Which, save};
use crate::bazaar::market::versions::Details;

/// The longest a title, a summary and a description may be.
const TITLE: usize = 60;
const SUMMARY: usize = 140;
const DESCRIPTION: usize = 5000;

/// The most a paid listing may cost, in cents.
const MOST_CENTS: u32 = 50_000;

/// The least a paid listing may cost where the bazaar has not said.
const LEAST_CENTS: u32 = 99;

/// The details of a draft, as they are typed.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Form {
    pub which: Which,
    pub title: String,
    pub summary: String,
    pub description: String,
    /// In dollars, `0` for free.
    pub price: String,
    pub tags: Vec<String>,
    /// What is typed in the tags' field.
    pub typing: String,
    /// The tag offered that is lit.
    pub lit: usize,
    /// The tags' field takes focus again.
    pub refocus: bool,
    /// Why the details cannot be saved as they are.
    pub problem: Option<String>,
}

impl Form {
    pub fn of(which: Which, details: &Details) -> Form {
        let cents = details.price_cents;
        Form {
            which,
            title: details.title.clone(),
            summary: details.summary.clone(),
            description: details.description.clone(),
            price: format!("{}.{:02}", cents / 100, cents % 100),
            tags: details.tags.clone(),
            typing: String::new(),
            lit: 0,
            refocus: false,
            problem: None,
        }
    }

    /// The details typed, or why they will not do. A paid listing
    /// costs at least `least` cents.
    pub fn details(&self, least: u32) -> Result<Details, String> {
        let title = self.title.trim();
        if title.is_empty() || title.chars().count() > TITLE {
            return Err(format!(
                "Give it a title of at most {TITLE} letters."
            ));
        }
        if self.summary.trim().chars().count() > SUMMARY {
            return Err(format!(
                "Keep the summary to {SUMMARY} letters."
            ));
        }
        if self.description.chars().count() > DESCRIPTION {
            return Err(format!(
                "Shorten the description to {DESCRIPTION} letters."
            ));
        }
        Ok(Details {
            title: title.to_string(),
            summary: self.summary.trim().to_string(),
            description: self.description.trim().to_string(),
            price_cents: cents(&self.price, least)?,
            tags: self.tags.clone(),
            cover_url: None,
        })
    }
}

/// The price typed, in cents, or why it is not one.
pub(super) fn cents(price: &str, least: u32) -> Result<u32, String> {
    let price = price.trim().trim_start_matches('$');
    if price.is_empty() {
        return Ok(0);
    }
    let dollars = price
        .parse::<f64>()
        .ok()
        .filter(|dollars| dollars.is_finite() && *dollars >= 0.0)
        .ok_or("Write the price in dollars, such as 4.99.")?;
    match (dollars * 100.0).round() as u32 {
        0 => Ok(0),
        cents if (least..=MOST_CENTS).contains(&cents) => Ok(cents),
        _ => Err(format!(
            "At least {}, or 0 for free.",
            price_of(least)
        )),
    }
}

pub(super) fn price_of(cents: u32) -> String {
    format!("${}.{:02}", cents / 100, cents % 100)
}

/// The least a paid listing may cost, as the bazaar says.
pub(super) fn least(world: &World) -> u32 {
    world
        .resource::<Drafts>()
        .fees
        .as_ref()
        .map_or(LEAST_CENTS, |fees| fees.min_price_cents.max(1))
}

/// Saves the draft's details as typed, then does what `then` says.
/// False when they will not do, and the form says why.
pub(super) fn save_form(world: &mut World, then: Then) -> bool {
    let least = least(world);
    let Some(form) = world.resource::<Desk>().form.clone() else {
        return false;
    };
    let cover = match &world.resource::<Pick>().state {
        Picked::Nothing => Ok(None),
        Picked::Got(picture) => Ok(Some((
            picture.bytes.clone(),
            picture.media.to_string(),
        ))),
        Picked::Fetching => {
            Err("Wait for the cover to come.".to_string())
        }
        Picked::Failed(_) => Err("The cover could not be had: give \
                                  another."
            .to_string()),
    };
    let details = form
        .details(least)
        .and_then(|details| Ok((details, cover?)));
    match details {
        Ok((details, cover)) => {
            if let Some(form) = &mut world.resource_mut::<Desk>().form
            {
                form.problem = None;
            }
            save(world, form.which, details, cover, then);
            true
        }
        Err(why) => {
            if let Some(form) = &mut world.resource_mut::<Desk>().form
            {
                form.problem = Some(why);
            }
            false
        }
    }
}
