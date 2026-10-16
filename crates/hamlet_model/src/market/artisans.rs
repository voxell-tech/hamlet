//! Agents other chiefs made, ready to hire. Mock data for now.

use serde::{Deserialize, Serialize};

use super::{Listing, Price, Seller, Standing};

/// An agent a chief made by hand, for hire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hireling {
    pub id: String,
    pub name: String,
    pub maker: Seller,
    /// What it is for, in a line.
    pub purpose: String,
    /// How it thinks and speaks.
    pub character: String,
    pub skills: Vec<String>,
    /// The model its maker made it on, as OpenRouter names it.
    pub model: String,
    pub craft: Craft,
    pub tags: Vec<String>,
    pub price: Price,
    pub standing: Standing,
    pub reviews: Vec<Review>,
    pub version: String,
    /// The day it last changed, as `YYYY-MM-DD`.
    pub updated: String,
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
            Craft::Trade => "Trade",
            Craft::Household => "Household",
            Craft::Teaching => "Teaching",
        }
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

impl Listing for Hireling {
    fn name(&self) -> &str {
        &self.name
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

fn seller(name: &str, verified: bool) -> Seller {
    Seller {
        name: name.into(),
        verified,
    }
}

fn all(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn standing(stars: f32, reviews: u32, owners: u32) -> Standing {
    Standing {
        stars,
        reviews,
        owners,
    }
}

fn review(by: &str, stars: u8, words: &str) -> Review {
    Review {
        by: by.into(),
        stars,
        words: words.into(),
    }
}

/// The hirelings on offer.
pub fn hirelings() -> Vec<Hireling> {
    vec![
        Hireling {
            id: "quillfeather".into(),
            name: "Quillfeather".into(),
            maker: seller("Hollis Reed", true),
            purpose: "Edits your drafts until they say what you \
                      meant."
                .into(),
            character: "A patient old editor who reads every line \
                        twice. Quillfeather cuts what can go, keeps \
                        your voice, and says why in the margin."
                .into(),
            skills: all(&[
                "line editing",
                "style guides",
                "outlines",
            ]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Writing,
            tags: all(&["essays", "blog", "letters"]),
            price: Price::Monthly(4.0),
            standing: standing(4.8, 212, 1904),
            reviews: vec![
                review(
                    "Juno of Fernhollow",
                    5,
                    "Took my rambling notes and made a newsletter \
                     of them. Still sounds like me.",
                ),
                review(
                    "Abel Marsh",
                    4,
                    "Firm with adverbs. I have learned to live with \
                     it.",
                ),
            ],
            version: "2.3.0".into(),
            updated: "2026-09-28".into(),
        },
        Hireling {
            id: "bramble".into(),
            name: "Bramble the Bookkeeper".into(),
            maker: seller("Odile Penhallow", true),
            purpose: "Keeps your receipts, invoices and monthly \
                      ledger in order."
                .into(),
            character: "Prim, exact and quietly delighted by a \
                        column that adds up. Bramble asks before it \
                        guesses at a number."
                .into(),
            skills: all(&["invoices", "receipts", "spreadsheets"]),
            model: "openai/gpt-5-mini".into(),
            craft: Craft::Trade,
            tags: all(&["accounts", "tax", "small business"]),
            price: Price::Once(12.0),
            standing: standing(4.6, 98, 860),
            reviews: vec![
                review(
                    "Merriweather Farm",
                    5,
                    "The quarter closed itself. I only signed.",
                ),
                review(
                    "Tamsin Hale",
                    4,
                    "Slow with blurry photos of receipts, but never \
                     wrong.",
                ),
            ],
            version: "1.6.2".into(),
            updated: "2026-08-14".into(),
        },
        Hireling {
            id: "lanternjaw".into(),
            name: "Old Tom Lanternjaw".into(),
            maker: seller("Cask & Crow Works", false),
            purpose: "Keeps the night watch over your servers and \
                      logs."
                .into(),
            character: "A gruff watchman who never sleeps and \
                        rarely speaks. When Tom rings the bell, \
                        something is truly on fire."
                .into(),
            skills: all(&["log reading", "alerts", "uptime checks"]),
            model: "google/gemini-2.5-flash".into(),
            craft: Craft::Code,
            tags: all(&["ops", "monitoring", "on call"]),
            price: Price::Monthly(6.0),
            standing: standing(4.4, 57, 412),
            reviews: vec![
                review(
                    "Ivo Brightwater",
                    5,
                    "Woke me once in three months, and it was the \
                     disk. Worth every coin.",
                ),
                review(
                    "Petra Lowe",
                    3,
                    "Good watchman, terrible at small talk.",
                ),
            ],
            version: "0.9.4".into(),
            updated: "2026-10-03".into(),
        },
        Hireling {
            id: "pip".into(),
            name: "Pip Rustwhistle".into(),
            maker: seller("Hearthforge Collective", true),
            purpose: "Reviews your code before anyone else has to."
                .into(),
            character: "Brisk, fair and fond of small diffs. Pip \
                        points at the line, says what could break, \
                        and offers the fix."
                .into(),
            skills: all(&["code review", "rust", "typescript"]),
            model: "qwen/qwen3-coder".into(),
            craft: Craft::Code,
            tags: all(&["pull requests", "lint", "refactor"]),
            price: Price::Free,
            standing: standing(4.7, 340, 5120),
            reviews: vec![
                review(
                    "Rook Ashdown",
                    5,
                    "Caught an off-by-one my whole guild missed.",
                ),
                review(
                    "Linnea Cole",
                    4,
                    "A little too fond of renaming things.",
                ),
            ],
            version: "3.1.0".into(),
            updated: "2026-09-19".into(),
        },
        Hireling {
            id: "marigold".into(),
            name: "Marigold Almanac".into(),
            maker: seller("The Thornfield Library", true),
            purpose: "Researches a question and cites where each \
                      answer came from."
                .into(),
            character: "A librarian with a lantern and a long \
                        memory. Marigold never states a thing she \
                        cannot point to."
                .into(),
            skills: all(&["web search", "citations", "summaries"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Research,
            tags: all(&["sources", "reports", "reading"]),
            price: Price::Once(8.0),
            standing: standing(4.9, 176, 2210),
            reviews: vec![
                review(
                    "Dr. Ambrose Vell",
                    5,
                    "Every claim footnoted. My students fear her.",
                ),
                review(
                    "Wynn Carrow",
                    5,
                    "Found the 1911 survey nobody else could.",
                ),
            ],
            version: "1.2.1".into(),
            updated: "2026-07-30".into(),
        },
        Hireling {
            id: "hester".into(),
            name: "Hester Thimble".into(),
            maker: seller("Nell Brackenbury", false),
            purpose: "Plans the week's meals, the shopping and the \
                      chores."
                .into(),
            character: "Warm, practical and impossible to argue \
                        with about vegetables. Hester remembers who \
                        dislikes mushrooms."
                .into(),
            skills: all(&["meal plans", "shopping lists", "rotas"]),
            model: "openai/gpt-5-mini".into(),
            craft: Craft::Household,
            tags: all(&["family", "groceries", "home"]),
            price: Price::Free,
            standing: standing(4.5, 421, 6340),
            reviews: vec![
                review(
                    "The Oakleys",
                    5,
                    "The children eat greens now. Witchcraft.",
                ),
                review(
                    "Corin Pell",
                    4,
                    "Plans for six when I said two, every time.",
                ),
            ],
            version: "2.0.5".into(),
            updated: "2026-09-02".into(),
        },
        Hireling {
            id: "ferrous".into(),
            name: "Ferrous Gale".into(),
            maker: seller("Hearthforge Collective", true),
            purpose: "Writes the tests your code has been missing, \
                      and hunts the bugs they find."
                .into(),
            character: "Restless and thorough, a terrier for edge \
                        cases. Ferrous is happiest when something \
                        fails."
                .into(),
            skills: all(&["unit tests", "fuzzing", "bisecting"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Code,
            tags: all(&["testing", "quality", "ci"]),
            price: Price::Monthly(9.0),
            standing: standing(4.6, 88, 730),
            reviews: vec![
                review(
                    "Saoirse Venn",
                    5,
                    "Coverage from 30 to 80 in a fortnight.",
                ),
                review(
                    "Mott Granger",
                    4,
                    "Found three bugs and one I made on purpose.",
                ),
            ],
            version: "1.4.0".into(),
            updated: "2026-10-09".into(),
        },
        Hireling {
            id: "cobble".into(),
            name: "Cobble".into(),
            maker: seller("Ledgerlight", false),
            purpose: "Wrangles messy spreadsheets into tidy tables \
                      and charts."
                .into(),
            character: "A cheerful sorter who hums while it works. \
                        Cobble shows you every change before it \
                        makes it."
                .into(),
            skills: all(&["csv", "pivot tables", "charts"]),
            model: "deepseek/deepseek-chat".into(),
            craft: Craft::Research,
            tags: all(&["data", "cleaning", "excel"]),
            price: Price::Once(5.0),
            standing: standing(4.2, 64, 590),
            reviews: vec![
                review(
                    "Fen Whitlock",
                    4,
                    "Twelve years of orchard yields, finally in one \
                     sheet.",
                ),
                review(
                    "Aster Quill",
                    4,
                    "Charts are plain, but correct.",
                ),
            ],
            version: "0.7.0".into(),
            updated: "2026-06-21".into(),
        },
        Hireling {
            id: "wick".into(),
            name: "Wick".into(),
            maker: seller("Schoolhouse on the Hill", true),
            purpose: "Tutors any subject by asking the right next \
                      question."
                .into(),
            character: "Gentle and curious, never gives the answer \
                        away. Wick waits for you to find it, then \
                        cheers."
                .into(),
            skills: all(&["tutoring", "quizzes", "explaining"]),
            model: "google/gemini-2.5-flash".into(),
            craft: Craft::Teaching,
            tags: all(&["school", "learning", "maths"]),
            price: Price::Free,
            standing: standing(4.9, 512, 8800),
            reviews: vec![
                review(
                    "Mira, aged 11",
                    5,
                    "Fractions make sense now. Wick is my friend.",
                ),
                review(
                    "Rowan Fairley",
                    5,
                    "Never once did my son's homework for him.",
                ),
            ],
            version: "4.0.1".into(),
            updated: "2026-09-25".into(),
        },
        Hireling {
            id: "saffron".into(),
            name: "Saffron Vane".into(),
            maker: seller("Market Cross Studio", false),
            purpose: "Writes the posts, signs and shop words for a \
                      small business."
                .into(),
            character: "Bright, quick and a touch theatrical. \
                        Saffron knows a good sign when she paints \
                        one."
                .into(),
            skills: all(&["copywriting", "social posts", "naming"]),
            model: "openai/gpt-5".into(),
            craft: Craft::Trade,
            tags: all(&["marketing", "shop", "brand"]),
            price: Price::Monthly(3.0),
            standing: standing(4.1, 73, 640),
            reviews: vec![
                review(
                    "The Copper Kettle",
                    4,
                    "Our chalkboard has never been funnier.",
                ),
                review(
                    "Gideon Thorne",
                    3,
                    "Too many exclamation marks for a funeral home.",
                ),
            ],
            version: "1.1.3".into(),
            updated: "2026-08-30".into(),
        },
        Hireling {
            id: "ondine".into(),
            name: "Ondine Brook".into(),
            maker: seller("Hollis Reed", true),
            purpose: "Translates letters and keeps the tone of the \
                      one who wrote them."
                .into(),
            character: "Soft-spoken and well travelled. Ondine asks \
                        who a letter is for before she chooses a \
                        word."
                .into(),
            skills: all(&["translation", "letters", "etiquette"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Writing,
            tags: all(&["languages", "french", "japanese"]),
            price: Price::Once(6.0),
            standing: standing(4.7, 129, 1150),
            reviews: vec![
                review(
                    "Haruto Ise",
                    5,
                    "My grandmother cried at the letter. Good tears.",
                ),
                review(
                    "Élodie March",
                    4,
                    "Formal by default. Ask her to relax.",
                ),
            ],
            version: "1.3.0".into(),
            updated: "2026-10-01".into(),
        },
        Hireling {
            id: "mossback".into(),
            name: "Mossback the Gardener".into(),
            maker: seller("Cask & Crow Works", false),
            purpose: "Tends your inbox: weeds, sorts and drafts the \
                      replies that matter."
                .into(),
            character: "Slow, steady and unbothered by a thousand \
                        unread letters. Mossback pulls weeds before \
                        it waters."
                .into(),
            skills: all(&["email triage", "drafting", "follow ups"]),
            model: "google/gemini-2.5-flash".into(),
            craft: Craft::Household,
            tags: all(&["inbox", "mail", "calm"]),
            price: Price::Monthly(2.5),
            standing: standing(4.3, 201, 2480),
            reviews: vec![
                review(
                    "Benedict Ware",
                    5,
                    "Inbox zero for the first time since the war.",
                ),
                review(
                    "Olwen Price",
                    4,
                    "Archived my mother once. She forgave it.",
                ),
            ],
            version: "2.2.0".into(),
            updated: "2026-05-17".into(),
        },
    ]
}
