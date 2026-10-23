//! The Agents and Plugins stalls drawn: a listing's row, the kinds
//! it is narrowed to, and its page with its maker, what chiefs say of
//! it and the box it is had from. Hiring and installing come with the
//! marketplace, so the box says so and nothing is had.

use hamlet_engine_bevy::ui::{
    AnyView, FrameProps, Space, Tone, ViewExt, column, icon, row,
};

use crate::samples::agents::{Craft, Hireling};
use crate::samples::plugins::{
    Addition, Contraption, Kind, Part, Permission,
};
use crate::samples::{Price, SELLERS_SHARE, Seller, Standing};
use crate::view::shelf::{Shown, kinds};
use crate::view::style::{
    Cost, Line, action_box, aside, lines, meta, named, prose,
    section, sections,
};
use crate::view::words::count;

impl Shown for Hireling {
    const BACK: &'static str = "AGENTS";
    const NOUN: &'static str = "agents";
    const FIND: &'static str = "Search agents";

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

    fn byline(&self) -> String {
        format!(
            "{}, by {}",
            rating(self.standing),
            seller_words(&self.maker)
        )
    }

    fn row(&self) -> Line {
        Line {
            cover: craft_cover(self.craft),
            name: self.name.to_string(),
            purpose: self.purpose.to_string(),
            meta: format!(
                "{}, by {}",
                rating(self.standing),
                self.maker.name
            ),
            cost: self.price.cost(),
            mark: None,
        }
    }

    fn page(&self) -> AnyView {
        let mut parts = vec![
            deal(
                self.price,
                &self.maker,
                "Hiring comes with the marketplace. Nobody moves in \
                 yet, and nothing is paid.",
            ),
            prose(self.purpose).boxed(),
            section("HOW IT WORKS", aside(self.character).boxed()),
            section("SKILLS", aside(self.skills.join(", ")).boxed()),
            section("TAGS", aside(self.tags.join(", ")).boxed()),
        ];
        if let Some(review) = self.reviews.first() {
            parts.push(section(
                "WHAT USERS SAY",
                column((
                    prose(format!("\"{}\"", review.words)),
                    meta(format!(
                        "{} of 5, from {}",
                        review.stars, review.by
                    )),
                ))
                .space(Space::Half)
                .boxed(),
            ));
        }
        sections(parts)
    }
}

impl Shown for Contraption {
    const BACK: &'static str = "PLUGINS";
    const NOUN: &'static str = "plugins";
    const FIND: &'static str = "Search plugins";

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

    fn byline(&self) -> String {
        format!(
            "{}, {} installs, by {}",
            rating(self.standing),
            count(self.standing.owners.into()),
            seller_words(&self.publisher)
        )
    }

    fn row(&self) -> Line {
        Line {
            cover: kind_cover(self.kind),
            name: self.name.to_string(),
            purpose: self.summary.to_string(),
            meta: format!(
                "{}, {} installs",
                rating(self.standing),
                count(self.standing.owners.into())
            ),
            cost: self.price.cost(),
            mark: None,
        }
    }

    fn page(&self) -> AnyView {
        let mut parts = vec![
            deal(
                self.price,
                &self.publisher,
                "Installing comes with the marketplace. Hamlet \
                 gains nothing yet, and nothing is paid.",
            ),
            prose(self.summary).boxed(),
            aside(self.description).boxed(),
            section(
                "ADDS",
                lines(self.adds.iter().map(addition).collect()),
            ),
        ];
        if !self.asks.is_empty() {
            let asks = self.asks.iter().map(|asks| permission(*asks));
            parts.push(section("PERMISSIONS", lines(asks.collect())));
        }
        let (version, words) = self.latest;
        parts.push(section(
            "LATEST VERSION",
            aside(format!("{version}: {words}")).boxed(),
        ));
        sections(parts)
    }
}

impl Price {
    /// The price as a row or a page shows it.
    pub(crate) fn cost(self) -> Cost {
        match self {
            Price::Free => Cost::free(),
            Price::Once(dollars) => {
                Cost::new(format!("${dollars:.2}"), "once")
            }
            Price::Monthly(dollars) => {
                Cost::new(format!("${dollars:.2}"), "monthly")
            }
        }
    }
}

fn craft_cover(craft: Craft) -> &'static str {
    match craft {
        Craft::Writing => "pencil",
        Craft::Code => "page",
        Craft::Research => "lens",
        Craft::Trade => "coins",
        Craft::Household => "clock",
        Craft::Teaching => "book",
    }
}

fn kind_cover(kind: Kind) -> &'static str {
    match kind {
        Kind::Drawing => "pencil",
        Kind::Planning => "check_all",
        Kind::Maps => "signal",
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

/// The box a listing is had from: its price, who is paid, and what
/// having it is for now, `later`.
fn deal(
    price: Price,
    seller: &Seller,
    later: &'static str,
) -> AnyView {
    let mut notes = vec![meta(later).boxed()];
    if price != Price::Free {
        notes.push(meta(share(seller)).boxed());
    }
    action_box(Some(price.cost()), Vec::new(), notes)
}

/// One thing a plugin adds, behind the mark of its part.
fn addition(addition: &Addition) -> AnyView {
    let mark = match addition.part {
        Part::Panel => "menu",
        Part::Tool => "robot",
        Part::Command => "play",
        Part::InChat => "speech",
    };
    row((
        icon(mark).tone(Tone::Faint),
        named(
            addition.name,
            format!("{}: {}", addition.part.name(), addition.words),
        ),
    ))
    .space(Space::Gap)
    .boxed()
}

/// Something a plugin asks to do; what could change what is the
/// chief's in red.
fn permission(asks: Permission) -> AnyView {
    let tone = if asks.grave() {
        Tone::Critical
    } else {
        Tone::Body
    };
    row((icon("shield").tone(tone), prose(asks.words()).tone(tone)))
        .space(Space::Gap)
        .boxed()
}
