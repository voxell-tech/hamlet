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
            Craft::Trade => "Business",
            Craft::Household => "Personal",
            Craft::Teaching => "Learning",
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
            id: "pr-reviewer".into(),
            name: "PR Reviewer".into(),
            maker: seller("Dev Tooling Collective", true),
            purpose: "Reviews pull requests for bugs, risky changes \
                      and missing tests."
                .into(),
            character: "Reads the diff and the code around it \
                        before commenting. Flags real bugs first, \
                        style last, and never rewrites your code \
                        unasked."
                .into(),
            skills: all(&["code-review", "git", "test-gaps"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Code,
            tags: all(&["github", "review"]),
            price: Price::Free,
            standing: standing(4.8, 412, 9300),
            reviews: vec![
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
            version: "2.1.0".into(),
            updated: "2026-10-02".into(),
        },
        Hireling {
            id: "meeting-notes".into(),
            name: "Meeting Notes".into(),
            maker: seller("Kofi Mensah", true),
            purpose: "Turns transcripts into decisions, action \
                      items and owners."
                .into(),
            character: "Short and structured. Puts decisions at the \
                        top, lists every action item with an owner \
                        and a date, and asks when something is \
                        unclear."
                .into(),
            skills: all(&["summarise", "action-items"]),
            model: "openai/gpt-5-mini".into(),
            craft: Craft::Trade,
            tags: all(&["meetings", "notes"]),
            price: Price::Monthly(2.0),
            standing: standing(4.7, 286, 6100),
            reviews: vec![
                review(
                    "Lena K.",
                    5,
                    "Our standups finally have written outcomes.",
                ),
                review(
                    "Ravi S.",
                    4,
                    "Needs speaker names to work best.",
                ),
            ],
            version: "1.6.3".into(),
            updated: "2026-09-21".into(),
        },
        Hireling {
            id: "sql-helper".into(),
            name: "SQL Helper".into(),
            maker: seller("Rowan Patel", true),
            purpose: "Writes and explains SQL for Postgres, MySQL \
                      and SQLite."
                .into(),
            character: "Asks for the schema before guessing. \
                        Explains each query in plain words and \
                        warns about full-table scans and missing \
                        indexes."
                .into(),
            skills: all(&["sql", "schema-reading"]),
            model: "deepseek/deepseek-chat-v3.1".into(),
            craft: Craft::Code,
            tags: all(&["sql", "data"]),
            price: Price::Free,
            standing: standing(4.6, 198, 4800),
            reviews: vec![review(
                "Ana G.",
                5,
                "Explains joins better than my old professor.",
            )],
            version: "1.3.0".into(),
            updated: "2026-08-15".into(),
        },
        Hireling {
            id: "cold-email".into(),
            name: "Cold Email Writer".into(),
            maker: seller("Growth Desk", false),
            purpose: "Drafts short, personal outreach emails from a \
                      prospect's website."
                .into(),
            character: "Researches the company first, writes three \
                        lines max, and never invents facts about \
                        the person. Gives you two variants to test."
                .into(),
            skills: all(&["web-research", "copywriting"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Trade,
            tags: all(&["sales", "email"]),
            price: Price::Once(12.0),
            standing: standing(4.3, 141, 2900),
            reviews: vec![review(
                "Marco D.",
                4,
                "Reply rate went up, but check every claim.",
            )],
            version: "1.1.2".into(),
            updated: "2026-09-09".into(),
        },
        Hireling {
            id: "recipe-planner".into(),
            name: "Meal Planner".into(),
            maker: seller("Hannah Brooks", false),
            purpose: "Plans a week of meals around what's in your \
                      fridge and your diet."
                .into(),
            character: "Practical and budget-aware. Builds a \
                        shopping list grouped by aisle and reuses \
                        ingredients across meals to cut waste."
                .into(),
            skills: all(&["meal-planning", "shopping-list"]),
            model: "google/gemini-2.5-flash".into(),
            craft: Craft::Household,
            tags: all(&["food", "planning"]),
            price: Price::Free,
            standing: standing(4.5, 322, 7700),
            reviews: vec![review(
                "Jess M.",
                5,
                "Saved me an hour every Sunday.",
            )],
            version: "1.4.0".into(),
            updated: "2026-07-30".into(),
        },
        Hireling {
            id: "study-buddy".into(),
            name: "Study Buddy".into(),
            maker: seller("Learnwell", true),
            purpose: "Quizzes you on your notes with spaced \
                      repetition."
                .into(),
            character: "Patient tutor. Asks one question at a time, \
                        explains wrong answers, and brings back the \
                        cards you struggle with."
                .into(),
            skills: all(&["flashcards", "spaced-repetition"]),
            model: "openai/gpt-5-mini".into(),
            craft: Craft::Teaching,
            tags: all(&["study", "quiz"]),
            price: Price::Monthly(3.0),
            standing: standing(4.7, 509, 11400),
            reviews: vec![
                review(
                    "Hiro T.",
                    5,
                    "Got me through organic chemistry.",
                ),
                review(
                    "Sam W.",
                    4,
                    "Wish it could read PDFs directly.",
                ),
            ],
            version: "2.0.1".into(),
            updated: "2026-10-05".into(),
        },
        Hireling {
            id: "bug-triager".into(),
            name: "Issue Triager".into(),
            maker: seller("Open Source Helpers", true),
            purpose: "Labels, deduplicates and asks for repro steps \
                      on new GitHub issues."
                .into(),
            character: "Polite and consistent. Finds duplicates, \
                        asks for missing versions and logs, and \
                        suggests a label and priority for you to \
                        confirm."
                .into(),
            skills: all(&["github-issues", "dedupe"]),
            model: "anthropic/claude-haiku-4.5".into(),
            craft: Craft::Code,
            tags: all(&["github", "maintainers"]),
            price: Price::Free,
            standing: standing(4.6, 230, 3600),
            reviews: vec![review(
                "Ines R.",
                5,
                "Our backlog is finally readable.",
            )],
            version: "1.2.4".into(),
            updated: "2026-09-18".into(),
        },
        Hireling {
            id: "docs-writer".into(),
            name: "Docs Writer".into(),
            maker: seller("Clear Docs", false),
            purpose: "Writes READMEs, API docs and changelogs from \
                      your code."
                .into(),
            character: "Reads the code and existing docs first, \
                        matches your tone, and keeps examples \
                        runnable. Marks anything it could not \
                        verify."
                .into(),
            skills: all(&["docs", "api-reference"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Writing,
            tags: all(&["docs", "readme"]),
            price: Price::Once(15.0),
            standing: standing(4.4, 120, 2200),
            reviews: vec![review(
                "Leo F.",
                4,
                "Great first drafts, light editing needed.",
            )],
            version: "1.0.5".into(),
            updated: "2026-08-27".into(),
        },
        Hireling {
            id: "inbox-zero".into(),
            name: "Inbox Sorter".into(),
            maker: seller("Calm Mail", true),
            purpose: "Sorts your inbox into reply, read later and \
                      archive, with draft replies."
                .into(),
            character: "Conservative: never sends anything, only \
                        drafts. Learns which senders matter to you \
                        from what you open."
                .into(),
            skills: all(&["email-triage", "drafting"]),
            model: "openai/gpt-5-mini".into(),
            craft: Craft::Household,
            tags: all(&["email", "productivity"]),
            price: Price::Monthly(4.0),
            standing: standing(4.2, 388, 8500),
            reviews: vec![review(
                "Nora P.",
                4,
                "Drafts are good, sorting needs a week to learn.",
            )],
            version: "1.7.0".into(),
            updated: "2026-09-30".into(),
        },
        Hireling {
            id: "research-assistant".into(),
            name: "Research Assistant".into(),
            maker: seller("Ada Okafor", true),
            purpose: "Finds sources, reads them and writes a cited \
                      summary."
                .into(),
            character: "Cites every claim with a link, separates \
                        what sources say from its own reading, and \
                        says when evidence is thin."
                .into(),
            skills: all(&["web-search", "citations"]),
            model: "google/gemini-2.5-pro".into(),
            craft: Craft::Research,
            tags: all(&["research", "citations"]),
            price: Price::Free,
            standing: standing(4.8, 640, 14900),
            reviews: vec![
                review("Dr. Kim", 5, "Citations actually check out."),
                review("Paul R.", 5, "Best free agent here."),
            ],
            version: "3.0.0".into(),
            updated: "2026-10-08".into(),
        },
        Hireling {
            id: "blog-editor".into(),
            name: "Blog Editor".into(),
            maker: seller("Wordsmith Studio", false),
            purpose: "Edits drafts for clarity and flow without \
                      changing your voice."
                .into(),
            character: "Cuts filler, fixes structure and explains \
                        each change in the margin. Leaves your \
                        opinions alone."
                .into(),
            skills: all(&["line-editing", "style-guide"]),
            model: "anthropic/claude-sonnet-4.5".into(),
            craft: Craft::Writing,
            tags: all(&["writing", "editing"]),
            price: Price::Monthly(2.5),
            standing: standing(4.6, 265, 5200),
            reviews: vec![review(
                "Mia L.",
                5,
                "Feels like a real editor.",
            )],
            version: "1.5.2".into(),
            updated: "2026-09-12".into(),
        },
        Hireling {
            id: "expense-tracker".into(),
            name: "Expense Tracker".into(),
            maker: seller("Ledgerly", true),
            purpose: "Reads receipts and bank exports and keeps a \
                      monthly budget sheet."
                .into(),
            character: "Careful with numbers. Categorises spending, \
                        flags unusual charges and keeps everything \
                        in a CSV you own."
                .into(),
            skills: all(&["receipts", "budgeting"]),
            model: "openai/gpt-5-mini".into(),
            craft: Craft::Household,
            tags: all(&["finance", "budget"]),
            price: Price::Once(6.0),
            standing: standing(4.4, 174, 3900),
            reviews: vec![review(
                "Ben C.",
                4,
                "Handles messy bank exports well.",
            )],
            version: "1.2.0".into(),
            updated: "2026-08-04".into(),
        },
    ]
}
