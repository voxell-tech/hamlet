//! Agents other chiefs made, ready to hire.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::samples::{
    ORDERS, Order, Price, Ranked, Review, Seller, Standing, rank,
    seller, standing,
};
use crate::shelf::Listing;

/// An agent a chief made by hand, for hire.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hireling {
    pub id: &'static str,
    pub name: &'static str,
    pub maker: Seller,
    /// What it is for, in a line.
    pub purpose: &'static str,
    /// How it thinks and speaks.
    pub character: &'static str,
    pub skills: &'static [&'static str],
    pub craft: Craft,
    pub tags: &'static [&'static str],
    pub price: Price,
    pub standing: Standing,
    pub reviews: &'static [Review],
    /// The day it last changed, as `YYYY-MM-DD`.
    pub updated: &'static str,
}

/// The kind of work a hireling does.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(crate) enum Craft {
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
}

impl Hireling {
    fn ranked(&self) -> Ranked {
        (self.price, self.standing, self.updated)
    }
}

impl Listing for Hireling {
    type Id = String;
    type Filter = Option<Craft>;
    type Order = Order;

    const ORDERS: &'static [(Order, &'static str)] = ORDERS;

    fn id(&self) -> String {
        self.id.to_string()
    }

    fn told(&self) -> String {
        format!(
            "{} {} {} {} {} {}",
            self.name,
            self.maker.name,
            self.purpose,
            self.craft.name(),
            self.tags.join(" "),
            self.skills.join(" ")
        )
    }

    fn admits(&self, craft: &Option<Craft>) -> bool {
        craft.is_none_or(|craft| self.craft == craft)
    }

    fn order(&self, other: &Hireling, order: Order) -> Ordering {
        rank(self.ranked(), other.ranked(), order)
            .then_with(|| self.name.cmp(other.name))
    }
}

const fn review(
    by: &'static str,
    stars: u8,
    words: &'static str,
) -> Review {
    Review { by, stars, words }
}

/// The hirelings on offer.
pub(crate) static HIRELINGS: [Hireling; 6] = [
    Hireling {
        id: "pr-reviewer",
        name: "PR Reviewer",
        maker: seller("Dev Tooling Collective", true),
        purpose: "Reviews pull requests for bugs, risky changes and \
                  missing tests.",
        character: "Reads the diff and the code around it before \
                    commenting. Flags real bugs first, style last, \
                    and never rewrites your code unasked.",
        skills: &["code-review", "git", "test-gaps"],
        craft: Craft::Code,
        tags: &["github", "review"],
        price: Price::Free,
        standing: standing(4.8, 412, 9300),
        reviews: &[
            review(
                "Priya N.",
                5,
                "Caught a race condition two humans missed.",
            ),
            review(
                "Tom B.",
                4,
                "Occasionally too picky about naming.",
            ),
        ],
        updated: "2026-10-02",
    },
    Hireling {
        id: "meeting-notes",
        name: "Meeting Notes",
        maker: seller("Kofi Mensah", true),
        purpose: "Turns transcripts into decisions, action items \
                  and owners.",
        character: "Short and structured. Puts decisions at the \
                    top, lists every action item with an owner and \
                    a date, and asks when something is unclear.",
        skills: &["summarise", "action-items"],
        craft: Craft::Trade,
        tags: &["meetings", "notes"],
        price: Price::Monthly(2.0),
        standing: standing(4.7, 286, 6100),
        reviews: &[review(
            "Lena K.",
            5,
            "Our standups finally have written outcomes.",
        )],
        updated: "2026-09-21",
    },
    Hireling {
        id: "research-assistant",
        name: "Research Assistant",
        maker: seller("Ada Okafor", true),
        purpose: "Finds sources, reads them and writes a cited \
                  summary.",
        character: "Cites every claim with a link, separates what \
                    sources say from its own reading, and says when \
                    evidence is thin.",
        skills: &["web-search", "citations"],
        craft: Craft::Research,
        tags: &["research", "citations"],
        price: Price::Free,
        standing: standing(4.8, 640, 14900),
        reviews: &[review(
            "Dr. Kim",
            5,
            "Citations actually check out.",
        )],
        updated: "2026-10-08",
    },
    Hireling {
        id: "study-buddy",
        name: "Study Buddy",
        maker: seller("Learnwell", true),
        purpose: "Quizzes you on your notes with spaced repetition.",
        character: "Patient tutor. Asks one question at a time, \
                    explains wrong answers, and brings back the \
                    cards you struggle with.",
        skills: &["flashcards", "spaced-repetition"],
        craft: Craft::Teaching,
        tags: &["study", "quiz"],
        price: Price::Monthly(3.0),
        standing: standing(4.7, 509, 11400),
        reviews: &[review(
            "Hiro T.",
            5,
            "Got me through organic chemistry.",
        )],
        updated: "2026-10-05",
    },
    Hireling {
        id: "blog-editor",
        name: "Blog Editor",
        maker: seller("Wordsmith Studio", false),
        purpose: "Edits drafts for clarity and flow without \
                  changing your voice.",
        character: "Cuts filler, fixes structure and explains each \
                    change in the margin. Leaves your opinions \
                    alone.",
        skills: &["line-editing", "style-guide"],
        craft: Craft::Writing,
        tags: &["writing", "editing"],
        price: Price::Monthly(2.5),
        standing: standing(4.6, 265, 5200),
        reviews: &[review("Mia L.", 5, "Feels like a real editor.")],
        updated: "2026-09-12",
    },
    Hireling {
        id: "expense-tracker",
        name: "Expense Tracker",
        maker: seller("Ledgerly", true),
        purpose: "Reads receipts and bank exports and keeps a \
                  monthly budget sheet.",
        character: "Careful with numbers. Categorises spending, \
                    flags unusual charges and keeps everything in a \
                    CSV you own.",
        skills: &["receipts", "budgeting"],
        craft: Craft::Household,
        tags: &["finance", "budget"],
        price: Price::Once(6.0),
        standing: standing(4.4, 174, 3900),
        reviews: &[review(
            "Ben C.",
            4,
            "Handles messy bank exports well.",
        )],
        updated: "2026-08-04",
    },
];
