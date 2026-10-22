//! The Agents and Plugins stalls drawn: a listing's card, the chips
//! of its shelves, and its page with its maker, what chiefs say of it
//! and the box it is had from. Hiring and installing come with the
//! marketplace, so the box says so and nothing is had.

use agents_ui::{name, note};
use hamlet_engine_bevy::ui::{
    AnyView, Fill, FrameProps, Space, Tone, ViewExt, column,
    description, icon, label, row,
};

use crate::samples::agents::{Craft, Hireling};
use crate::samples::plugins::{
    Addition, Contraption, Kind, Part, Permission,
};
use crate::samples::{Price, SELLERS_SHARE, Seller, Standing};
use crate::view::shelf::{Shown, chips, part};
use crate::view::words::count;

impl Shown for Hireling {
    const BACK: &'static str = "AGENTS";
    const NOUN: &'static str = "agents";
    const FIND: &'static str = "Search agents";

    fn controls() -> Vec<AnyView> {
        vec![chips::<Hireling, _>(&Craft::ALL, Craft::name)]
    }

    fn title(&self) -> String {
        self.name.to_string()
    }

    fn row(&self) -> AnyView {
        card(
            (self.name, self.price, self.purpose),
            format!(
                "{}, by {}",
                rating(self.standing),
                self.maker.name
            ),
        )
    }

    fn page(&self) -> AnyView {
        let mut parts = vec![
            note(format!(
                "{}, by {}",
                rating(self.standing),
                seller_words(&self.maker)
            ))
            .boxed(),
            label(self.purpose).boxed(),
            deal(
                self.price,
                &self.maker,
                "Hiring comes with the marketplace: nobody moves in \
                 yet, and nothing is paid.",
            ),
            part("HOW IT WORKS", description(self.character).boxed()),
            part("SKILLS", note(self.skills.join(", ")).boxed()),
            part("TAGS", note(self.tags.join(", ")).boxed()),
        ];
        if let Some(review) = self.reviews.first() {
            parts.push(part(
                "WHAT USERS SAY",
                column((
                    label(format!("\"{}\"", review.words)),
                    note(format!(
                        "{} of 5, {}",
                        review.stars, review.by
                    )),
                ))
                .space(Space::None)
                .boxed(),
            ));
        }
        column(parts).boxed()
    }
}

impl Shown for Contraption {
    const BACK: &'static str = "PLUGINS";
    const NOUN: &'static str = "plugins";
    const FIND: &'static str = "Search plugins";

    fn controls() -> Vec<AnyView> {
        vec![chips::<Contraption, _>(&Kind::ALL, Kind::name)]
    }

    fn title(&self) -> String {
        self.name.to_string()
    }

    fn row(&self) -> AnyView {
        card(
            (self.name, self.price, self.summary),
            format!(
                "{}, {} installs",
                rating(self.standing),
                count(self.standing.owners.into())
            ),
        )
    }

    fn page(&self) -> AnyView {
        let mut parts = vec![
            note(format!(
                "{}, {} installs, by {}",
                rating(self.standing),
                count(self.standing.owners.into()),
                seller_words(&self.publisher)
            ))
            .boxed(),
            label(self.summary).boxed(),
            deal(
                self.price,
                &self.publisher,
                "Installing comes with the marketplace: Hamlet \
                 gains nothing yet, and nothing is paid.",
            ),
            description(self.description).boxed(),
            part(
                "ADDS",
                column(
                    self.adds
                        .iter()
                        .map(addition)
                        .collect::<Vec<_>>(),
                )
                .space(Space::Half)
                .boxed(),
            ),
        ];
        if !self.asks.is_empty() {
            let asks = self.asks.iter().map(|asks| permission(*asks));
            parts.push(part(
                "PERMISSIONS",
                column(asks.collect::<Vec<_>>())
                    .space(Space::Half)
                    .boxed(),
            ));
        }
        let (version, words) = self.latest;
        parts.push(part(
            "LATEST VERSION",
            description(format!("{version}: {words}")).boxed(),
        ));
        column(parts).boxed()
    }
}

/// A listing's card: its name with its price, what it is for, and
/// `foot`.
fn card(
    (name, price, purpose): (&'static str, Price, &'static str),
    foot: String,
) -> AnyView {
    column((
        row((label(name).bold(true), note(price.words())))
            .space(Space::Half),
        description(purpose),
        note(foot),
    ))
    .space(Space::None)
    .boxed()
}

/// Stars out of five and the reviews they come from.
fn rating(standing: Standing) -> String {
    format!(
        "{:.1} of 5 from {} reviews",
        standing.stars,
        count(standing.reviews.into())
    )
}

fn seller_words(seller: &Seller) -> String {
    match seller.verified {
        true => format!("{}, verified seller", seller.name),
        false => format!("{}, not verified", seller.name),
    }
}

/// The box a listing is had from: its price, who is paid, and what
/// having it is for now, `later`.
fn deal(
    price: Price,
    seller: &Seller,
    later: &'static str,
) -> AnyView {
    let (amount, per) = match price {
        Price::Free => ("Free".to_string(), ""),
        Price::Once(dollars) => (format!("${dollars:.2}"), "once"),
        Price::Monthly(dollars) => {
            (format!("${dollars:.2}"), "a month")
        }
    };
    let mut parts = vec![
        row((label(amount).bold(true), note(per)))
            .space(Space::Half)
            .boxed(),
    ];
    if price != Price::Free {
        parts.push(
            note(format!(
                "{} receives {SELLERS_SHARE}% of each sale.",
                seller.name
            ))
            .boxed(),
        );
    }
    parts.push(description(later).boxed());
    column(parts).fill(Fill::Card).space(Space::Half).boxed()
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
        column((
            row((
                label(addition.name).bold(true),
                name(addition.part.name()),
            ))
            .space(Space::Half),
            description(addition.words),
        ))
        .space(Space::None),
    ))
    .space(Space::Half)
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
    row((icon("shield").tone(tone), label(asks.words()).tone(tone)))
        .space(Space::Half)
        .boxed()
}
