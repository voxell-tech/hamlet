//! The samples of the Agents and Plugins stalls drawn, where the
//! bazaar's server cannot be reached: a listing's row, the kinds it
//! is narrowed to, and its page with its maker, what it does and what
//! chiefs say of it. They are made up, so its page says so and
//! nothing is had.

use agents_ui::markdown;
use hamlet_engine_bevy::ui::{
    AnyView, Tone, ViewExt, description, label,
};

use crate::listings::SELLERS_SHARE;
use crate::samples::agents::{Craft, Hireling};
use crate::samples::plugins::{Addition, Contraption, Kind};
use crate::samples::{Price, Seller, Standing};
use crate::view::parts::{
    Line, deal, fact, page_head, rows, section, sections, told, top,
};
use crate::view::shelf::{Shown, kinds};
use crate::view::trade::samples_notice;
use crate::view::words::{count, grouped};

impl Shown for Hireling {
    const BACK: &'static str = "AGENTS";
    const ABOUT: &'static str = "Agents other chiefs made, to hire.";
    const NOUN: &'static str = "agents";
    const FIND: &'static str = "Search agents";

    fn notice() -> AnyView {
        samples_notice()
    }

    fn controls() -> Vec<AnyView> {
        vec![kinds::<Hireling, _>(
            "All kinds",
            &Craft::ALL,
            Craft::name,
        )]
    }

    fn title(&self) -> String {
        self.name.to_string()
    }

    fn row(&self) -> Line {
        Line {
            name: self.name.to_string(),
            price: self.price.words(),
            purpose: self.purpose.to_string(),
            meta: format!(
                "{}, by {}",
                rating(self.standing),
                self.maker.name
            ),
            mark: None,
        }
    }

    fn page(&self) -> AnyView {
        let head = page_head(
            self.name,
            description(self.purpose).boxed(),
            format!(
                "{}, by {}",
                rating(self.standing),
                seller_words(&self.maker)
            ),
            None,
        );
        let mut parts = vec![
            top(vec![
                head,
                later(
                    self.price,
                    &self.maker,
                    "A sample: nobody moves in, and nothing is \
                     paid. Agents for hire come from the bazaar's \
                     server.",
                ),
            ]),
            section(
                "HOW IT WORKS",
                markdown(self.character, Tone::Body),
            ),
            section(
                "DETAILS",
                rows(vec![
                    fact("Kind", self.craft.name()),
                    fact("Skills", self.skills.join(", ")),
                    fact("Tags", self.tags.join(", ")),
                    fact(
                        "Hired by",
                        grouped(self.standing.owners.into()),
                    ),
                    fact("Updated", self.updated),
                ]),
            ),
        ];
        if !self.reviews.is_empty() {
            let said = self.reviews.iter().map(|review| {
                told(
                    label(format!("\"{}\"", review.words)).boxed(),
                    format!(
                        "{} of 5, from {}",
                        review.stars, review.by
                    ),
                )
            });
            parts.push(section(
                "WHAT OWNERS SAY",
                rows(said.collect()),
            ));
        }
        sections(parts)
    }
}

impl Shown for Contraption {
    const BACK: &'static str = "PLUGINS";
    const ABOUT: &'static str = "What others made to add to Hamlet.";
    const NOUN: &'static str = "plugins";
    const FIND: &'static str = "Search plugins";

    fn notice() -> AnyView {
        samples_notice()
    }

    fn controls() -> Vec<AnyView> {
        vec![kinds::<Contraption, _>(
            "All kinds",
            &Kind::ALL,
            Kind::name,
        )]
    }

    fn title(&self) -> String {
        self.name.to_string()
    }

    fn row(&self) -> Line {
        Line {
            name: self.name.to_string(),
            price: self.price.words(),
            purpose: self.summary.to_string(),
            meta: format!(
                "{}, {} installs",
                rating(self.standing),
                count(self.standing.owners.into())
            ),
            mark: None,
        }
    }

    fn page(&self) -> AnyView {
        let head = page_head(
            self.name,
            description(self.summary).boxed(),
            format!(
                "{}, {} installs, by {}",
                rating(self.standing),
                count(self.standing.owners.into()),
                seller_words(&self.publisher)
            ),
            None,
        );
        let mut parts = vec![
            top(vec![
                head,
                later(
                    self.price,
                    &self.publisher,
                    "A sample: nothing is installed or paid. Tools \
                     for your agents come from the bazaar's server.",
                ),
            ]),
            section("ABOUT", markdown(self.description, Tone::Body)),
            section(
                "WHAT IT ADDS",
                rows(self.adds.iter().map(addition).collect()),
            ),
        ];
        if !self.asks.is_empty() {
            let asks = self.asks.iter().map(|asks| {
                let tone = if asks.grave() {
                    Tone::Critical
                } else {
                    Tone::Body
                };
                label(asks.words()).tone(tone).boxed()
            });
            parts.push(section(
                "WHAT IT MAY DO",
                rows(asks.collect()),
            ));
        }
        let (version, words) = self.latest;
        parts.push(section(
            "DETAILS",
            rows(vec![
                fact("Kind", self.kind.name()),
                told(fact("Version", version), words),
                fact("Updated", self.updated),
            ]),
        ));
        sections(parts)
    }
}

impl Price {
    /// The price in a few words.
    pub(crate) fn words(self) -> String {
        match self {
            Price::Free => "Free".into(),
            Price::Once(dollars) => format!("${dollars:.2} once"),
            Price::Monthly(dollars) => {
                format!("${dollars:.2} monthly")
            }
        }
    }
}

/// Stars out of five and the reviews they come from.
pub(crate) fn rating(standing: Standing) -> String {
    format!(
        "{:.1} from {} reviews",
        standing.stars,
        count(standing.reviews.into())
    )
}

pub(crate) fn seller_words(seller: &Seller) -> String {
    match seller.verified {
        true => format!("{}, verified seller", seller.name),
        false => format!("{}, not verified", seller.name),
    }
}

/// The share of each sale `seller` receives.
pub(crate) fn share(seller: &Seller) -> String {
    format!("{} receives {SELLERS_SHARE}% of each sale.", seller.name)
}

/// The price of a listing that comes with the marketplace, who is
/// paid, and what having it is for now, `later`.
fn later(
    price: Price,
    seller: &Seller,
    later: &'static str,
) -> AnyView {
    let mut notes = vec![description(later).boxed()];
    if price != Price::Free {
        notes.push(description(share(seller)).boxed());
    }
    deal(Some(price.words()), Vec::new(), notes)
}

/// One thing a plugin adds: its name, the part of Hamlet it is, and
/// what it does.
fn addition(addition: &Addition) -> AnyView {
    told(fact(addition.name, addition.part.name()), addition.words)
}
