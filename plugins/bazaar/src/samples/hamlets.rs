//! Whole hamlets for sale: a mayor, its agents and all they know.

use std::cmp::Ordering;

use hamlet_engine_bevy::api::agents::{
    Settler as Settled, SettlerSkill, Template as Founded,
};

use crate::samples::{
    ORDERS, Order, Price, Seller, Standing, rank, seller, standing,
};
use crate::shelf::Listing;

/// A hamlet for sale, as it would be founded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Template {
    /// Its name on disk and in commands.
    pub name: &'static str,
    /// The name it is shown by.
    pub title: &'static str,
    /// What kind of hamlet it is, in a few words.
    pub kind: &'static str,
    pub description: &'static str,
    /// What it is for, as its mayor is told.
    pub purpose: &'static str,
    pub seller: Seller,
    pub price: Price,
    pub standing: Standing,
    pub mayor: Settler,
    /// Its agents besides the mayor.
    pub agents: &'static [Settler],
}

/// An agent of a hamlet for sale, with all it knows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settler {
    pub name: &'static str,
    pub purpose: &'static str,
    /// Its `CHARACTER.md`.
    pub character: &'static str,
    /// Its `NOTES.md`.
    pub notes: &'static str,
    /// Its one skill: its name, what it does and how.
    pub skill: (&'static str, &'static str, &'static str),
}

impl Settler {
    fn settled(&self) -> Settled {
        let (name, description, body) = self.skill;
        Settled {
            name: self.name.into(),
            purpose: self.purpose.into(),
            character: self.character.into(),
            notes: self.notes.into(),
            skills: vec![SettlerSkill {
                name: name.into(),
                description: description.into(),
                body: body.into(),
            }],
        }
    }
}

impl Template {
    /// It as the agents service founds it.
    pub fn founded(&self) -> Founded {
        Founded {
            name: self.name.into(),
            title: self.title.into(),
            purpose: self.purpose.into(),
            mayor: self.mayor.settled(),
            agents: self
                .agents
                .iter()
                .map(Settler::settled)
                .collect(),
        }
    }
}

impl Listing for Template {
    type Id = String;
    type Filter = ();
    type Order = Order;

    // A hamlet has no day it last changed.
    const ORDERS: &'static [(Order, &'static str)] =
        ORDERS.split_at(3).0;

    fn id(&self) -> String {
        self.name.to_string()
    }

    fn told(&self) -> String {
        format!(
            "{} {} {} {}",
            self.title, self.kind, self.seller.name, self.description
        )
    }

    fn order(&self, other: &Template, order: Order) -> Ordering {
        rank(
            (self.price, self.standing, ""),
            (other.price, other.standing, ""),
            order,
        )
        .then_with(|| self.title.cmp(other.title))
    }
}

/// Every hamlet on the store's shelves.
pub(crate) static FOR_SALE: [Template; 3] = [
    Template {
        name: "saas-launch",
        title: "Indie SaaS Launch Team",
        kind: "Product launch",
        description: "The team I used to launch two small SaaS \
                      products. It researches competitors, writes \
                      the landing page, sets pricing and plans \
                      launch week.",
        purpose: "Help a solo founder get a small software product \
                  from finished to launched, with research, copy, \
                  pricing and a launch plan.",
        seller: seller("Jonas Weber", true),
        price: Price::Once(19.0),
        standing: standing(4.7, 184, 1260),
        mayor: Settler {
            name: "launch-lead",
            purpose: "Breaks the launch into tasks, hands them out \
                      and keeps a checklist of what's left.",
            character: "Direct and organised. Gives you one \
                        decision at a time and a recommended option \
                        for each.",
            notes: "# Notes\n\n- Launch checklist lives in \
                    launch.md.\n- Ask before spending money on ads \
                    or tools.\n",
            skill: (
                "launch-checklist",
                "Keep a launch checklist with owners and dates.",
                "List every task from landing page to launch-day \
                 posts. Give each an owner and a date, and tick it \
                 off when done.",
            ),
        },
        agents: &[
            Settler {
                name: "market-researcher",
                purpose: "Finds competitors, their pricing and what \
                          users complain about.",
                character: "Thorough and sceptical of marketing \
                            claims. Quotes real reviews.",
                notes: "# Notes\n\n- Keep competitors in \
                        competitors.md as a table.\n",
                skill: (
                    "competitor-scan",
                    "Compare competitors on price, features and \
                     complaints.",
                    "Find five competitors. For each, record \
                     pricing tiers, key features and the three most \
                     common complaints, with links.",
                ),
            },
            Settler {
                name: "landing-copy",
                purpose: "Writes landing page copy, headlines and \
                          FAQs.",
                character: "Clear over clever. Writes for the \
                            customer's problem, not the product's \
                            features.",
                notes: "# Notes\n\n- One headline, one subheading, \
                        three benefits.\n",
                skill: (
                    "landing-page",
                    "Draft a landing page from a product brief.",
                    "Write a headline, subheading, three benefit \
                     blocks and five FAQs. Keep it under 400 words.",
                ),
            },
            Settler {
                name: "pricing-analyst",
                purpose: "Suggests pricing tiers based on \
                          competitors and costs.",
                character: "Numbers first. Shows the maths behind \
                            every price.",
                notes: "# Notes\n\n- Monthly and yearly, yearly at \
                        about 2 months free.\n",
                skill: (
                    "pricing-tiers",
                    "Propose pricing tiers with reasoning.",
                    "Use competitor prices and our cost per user to \
                     propose free, pro and team tiers.",
                ),
            },
        ],
    },
    Template {
        name: "lit-review",
        title: "Literature Review Crew",
        kind: "Academic research",
        description: "Built during my PhD to keep up with papers. \
                      It finds papers, summarises them, keeps a \
                      BibTeX library and points out gaps in the \
                      research.",
        purpose: "Support a graduate student's literature review: \
                  find relevant papers, summarise them accurately, \
                  keep citations tidy and highlight open questions.",
        seller: seller("Dr. Amara Osei", true),
        price: Price::Once(24.0),
        standing: standing(4.8, 212, 1430),
        mayor: Settler {
            name: "review-lead",
            purpose: "Turns a research question into search tasks \
                      and assembles the review outline.",
            character: "Careful and honest about uncertainty. Never \
                        cites a paper nobody has read.",
            notes: "# Notes\n\n- Research question is in \
                    question.md.\n",
            skill: (
                "review-outline",
                "Build a literature review outline from summaries.",
                "Group summaries by theme, order themes from \
                 foundational to recent, and note which claims each \
                 paper supports.",
            ),
        },
        agents: &[
            Settler {
                name: "paper-finder",
                purpose: "Searches Google Scholar, arXiv and \
                          Semantic Scholar for relevant papers.",
                character: "Casts a wide net, then ranks by \
                            relevance and citations.",
                notes: "# Notes\n\n- Log searches in searches.md so \
                        they can be repeated.\n",
                skill: (
                    "paper-search",
                    "Search academic databases for papers on a \
                     topic.",
                    "Search with three phrasings of the question. \
                     Keep the 15 most relevant papers with title, \
                     year, venue and link.",
                ),
            },
            Settler {
                name: "paper-reader",
                purpose: "Reads papers and writes structured \
                          summaries.",
                character: "Precise. Separates what the paper shows \
                            from what it claims.",
                notes: "# Notes\n\n- Summary template: question, \
                        method, data, findings, limits.\n",
                skill: (
                    "paper-summary",
                    "Summarise a paper with method, findings and \
                     limits.",
                    "Read the abstract, methods and results. Fill \
                     the summary template and quote the key result \
                     with its page.",
                ),
            },
            Settler {
                name: "citation-keeper",
                purpose: "Keeps the BibTeX library clean and \
                          consistent.",
                character: "Pedantic about formatting, in a helpful \
                            way.",
                notes: "# Notes\n\n- Library is refs.bib.\n",
                skill: (
                    "bibtex-tidy",
                    "Add and clean BibTeX entries.",
                    "Fetch the BibTeX for each paper, normalise the \
                     key and remove duplicates.",
                ),
            },
        ],
    },
    Template {
        name: "oss-maintainer",
        title: "Open Source Maintainer Kit",
        kind: "Software maintenance",
        description: "What I run on my open source library: it \
                      triages issues, checks pull requests and \
                      drafts release notes.",
        purpose: "Lighten an open source maintainer's load: triage \
                  issues, pre-review pull requests and write \
                  release notes.",
        seller: seller("Lukas Brandt", true),
        price: Price::Free,
        standing: standing(4.9, 422, 3870),
        mayor: Settler {
            name: "maintainer-lead",
            purpose: "Decides what needs a human and routes \
                      everything else.",
            character: "Respectful to contributors. Explains \
                        decisions briefly.",
            notes: "# Notes\n\n- Never merge or close anything \
                    without approval.\n",
            skill: (
                "weekly-digest",
                "Write a weekly digest of project activity.",
                "Summarise new issues, open PRs, releases and \
                 anything waiting on the maintainer.",
            ),
        },
        agents: &[
            Settler {
                name: "issue-sorter",
                purpose: "Labels new issues, finds duplicates and \
                          asks for reproduction steps.",
                character: "Polite and patient with first-time \
                            reporters.",
                notes: "# Notes\n\n- Labels: bug, feature, docs, \
                        question, needs-repro.\n",
                skill: (
                    "triage-issue",
                    "Triage a new issue.",
                    "Check for duplicates, suggest labels, and ask \
                     for version, OS and steps if missing.",
                ),
            },
            Settler {
                name: "release-notes",
                purpose: "Drafts release notes from merged PRs.",
                character: "Plain language. Groups changes by what \
                            users notice.",
                notes: "# Notes\n\n- Sections: Added, Changed, \
                        Fixed, Removed.\n",
                skill: (
                    "draft-release",
                    "Draft release notes from merged PRs.",
                    "List merged PRs since the last tag, group them \
                     by section and thank contributors by handle.",
                ),
            },
        ],
    },
];
