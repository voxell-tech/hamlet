//! Whole hamlets for sale: a mayor, its agents and all they know.
//! Mock data for now.

use serde::{Deserialize, Serialize};

use super::{Price, Seller, Standing};

/// A hamlet for sale, as it would be founded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    /// Its name on disk and in commands.
    pub name: String,
    /// The name it is shown by.
    pub title: String,
    /// What kind of hamlet it is, in a few words.
    pub kind: String,
    pub description: String,
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
}

/// An agent of a hamlet for sale, with all it knows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settler {
    pub name: String,
    pub purpose: String,
    /// Its `CHARACTER.md`.
    pub character: String,
    /// Its `NOTES.md`.
    pub notes: String,
    pub skills: Vec<Skill>,
}

/// A skill an agent keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    /// The name of its folder and of its front matter.
    pub name: String,
    pub description: String,
    /// What follows the front matter.
    pub body: String,
}

impl Skill {
    /// Its `SKILL.md`.
    pub fn file(&self) -> String {
        format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}\n",
            self.name,
            self.description,
            self.body.trim()
        )
    }
}

impl Listing {
    /// Everyone who comes with it, the mayor first.
    pub fn everyone(&self) -> impl Iterator<Item = &Settler> {
        std::iter::once(&self.mayor).chain(&self.agents)
    }
}

/// The hamlet for sale named `name`.
pub fn find(name: &str) -> Option<Listing> {
    for_sale().into_iter().find(|listing| listing.name == name)
}

fn seller(name: &str, verified: bool) -> Seller {
    Seller {
        name: name.into(),
        verified,
    }
}

fn standing(stars: f32, reviews: u32, owners: u32) -> Standing {
    Standing {
        stars,
        reviews,
        owners,
    }
}

fn skill(name: &str, description: &str, body: &str) -> Skill {
    Skill {
        name: name.into(),
        description: description.into(),
        body: body.into(),
    }
}

fn settler(
    name: &str,
    purpose: &str,
    character: &str,
    notes: &str,
    skills: Vec<Skill>,
) -> Settler {
    Settler {
        name: name.into(),
        purpose: purpose.into(),
        character: character.into(),
        notes: notes.into(),
        skills,
    }
}

/// Every hamlet on the store's shelves.
pub fn for_sale() -> Vec<Listing> {
    vec![
        saas_launch(),
        lit_review(),
        money_desk(),
        content_studio(),
        oss_maintainer(),
        job_hunt(),
    ]
}

fn saas_launch() -> Listing {
    Listing {
        name: "saas-launch".into(),
        title: "Indie SaaS Launch Team".into(),
        kind: "Product launch".into(),
        description: "The team I used to launch two small SaaS \
                      products. It researches competitors, writes \
                      the landing page, sets pricing and plans \
                      launch week."
            .into(),
        purpose: "Help a solo founder get a small software product \
                  from finished to launched, with research, copy, \
                  pricing and a launch plan."
            .into(),
        seller: seller("Jonas Weber", true),
        price: Price::Once(19.0),
        standing: standing(4.7, 184, 1260),
        seed: 11,
        mayor: settler(
            "launch-lead",
            "Breaks the launch into tasks, hands them out and keeps \
             a checklist of what's left.",
            "Direct and organised. Gives you one decision at a time \
             and a recommended option for each.",
            "# Notes\n\n- Launch checklist lives in launch.md.\n- \
             Ask before spending money on ads or tools.\n- Weekly \
             summary every Friday.\n",
            vec![skill(
                "launch-checklist",
                "Keep a launch checklist with owners and dates.",
                "List every task from landing page to launch-day \
                 posts. Give each an owner and a date, and tick it \
                 off when done.",
            )],
        ),
        agents: vec![
            settler(
                "market-researcher",
                "Finds competitors, their pricing and what users \
                 complain about.",
                "Thorough and sceptical of marketing claims. Quotes \
                 real reviews.",
                "# Notes\n\n- Keep competitors in competitors.md as \
                 a table.\n- Sources: G2, Reddit, Product Hunt \
                 comments.\n",
                vec![skill(
                    "competitor-scan",
                    "Compare competitors on price, features and \
                     complaints.",
                    "Find five competitors. For each, record \
                     pricing tiers, key features and the three most \
                     common complaints, with links.",
                )],
            ),
            settler(
                "landing-copy",
                "Writes landing page copy, headlines and FAQs.",
                "Clear over clever. Writes for the customer's \
                 problem, not the product's features.",
                "# Notes\n\n- One headline, one subheading, three \
                 benefits.\n- Avoid jargon like 'leverage' and \
                 'seamless'.\n",
                vec![skill(
                    "landing-page",
                    "Draft a landing page from a product brief.",
                    "Write a headline, subheading, three benefit \
                     blocks, social proof placeholders and five \
                     FAQs. Keep it under 400 words.",
                )],
            ),
            settler(
                "pricing-analyst",
                "Suggests pricing tiers based on competitors and \
                 costs.",
                "Numbers first. Shows the maths behind every price.",
                "# Notes\n\n- Monthly and yearly, yearly at about 2 \
                 months free.\n- Check our hosting cost per user \
                 before pricing.\n",
                vec![skill(
                    "pricing-tiers",
                    "Propose pricing tiers with reasoning.",
                    "Use competitor prices and our cost per user to \
                     propose free, pro and team tiers. Explain each \
                     limit and price.",
                )],
            ),
            settler(
                "launch-scheduler",
                "Plans launch week posts for Product Hunt, Hacker \
                 News and X.",
                "Calm under deadlines. Prefers fewer, better posts.",
                "# Notes\n\n- Product Hunt launches go live at \
                 12:01 PT.\n- Draft posts in launch-posts.md for \
                 review.\n",
                vec![skill(
                    "launch-week",
                    "Plan a week of launch posts across channels.",
                    "Lay out day-by-day posts for each channel with \
                     the draft text, the time to post and who \
                     replies to comments.",
                )],
            ),
        ],
    }
}

fn lit_review() -> Listing {
    Listing {
        name: "lit-review".into(),
        title: "Literature Review Crew".into(),
        kind: "Academic research".into(),
        description: "Built during my PhD to keep up with papers. \
                      It finds papers, summarises them, keeps a \
                      BibTeX library and points out gaps in the \
                      research."
            .into(),
        purpose: "Support a graduate student's literature review: \
                  find relevant papers, summarise them accurately, \
                  keep citations tidy and highlight open questions."
            .into(),
        seller: seller("Dr. Amara Osei", true),
        price: Price::Once(24.0),
        standing: standing(4.8, 212, 1430),
        seed: 23,
        mayor: settler(
            "review-lead",
            "Turns a research question into search tasks and \
             assembles the review outline.",
            "Careful and honest about uncertainty. Never cites a \
             paper nobody has read.",
            "# Notes\n\n- Research question is in question.md.\n- \
             Outline in review-outline.md, updated weekly.\n",
            vec![skill(
                "review-outline",
                "Build a literature review outline from summaries.",
                "Group summaries by theme, order themes from \
                 foundational to recent, and note which claims each \
                 paper supports.",
            )],
        ),
        agents: vec![
            settler(
                "paper-finder",
                "Searches Google Scholar, arXiv and Semantic \
                 Scholar for relevant papers.",
                "Casts a wide net, then ranks by relevance and \
                 citations.",
                "# Notes\n\n- Prefer peer-reviewed over preprints \
                 when both exist.\n- Log searches in searches.md so \
                 they can be repeated.\n",
                vec![skill(
                    "paper-search",
                    "Search academic databases for papers on a \
                     topic.",
                    "Search with three phrasings of the question. \
                     Keep the 15 most relevant papers with title, \
                     year, venue and link.",
                )],
            ),
            settler(
                "paper-reader",
                "Reads papers and writes structured summaries.",
                "Precise. Separates what the paper shows from what \
                 it claims.",
                "# Notes\n\n- Summary template: question, method, \
                 data, findings, limits.\n- Quote page numbers for \
                 key claims.\n",
                vec![skill(
                    "paper-summary",
                    "Summarise a paper with method, findings and \
                     limits.",
                    "Read the abstract, methods and results. Fill \
                     the summary template and quote the key result \
                     with its page.",
                )],
            ),
            settler(
                "citation-keeper",
                "Keeps the BibTeX library clean and consistent.",
                "Pedantic about formatting, in a helpful way.",
                "# Notes\n\n- Library is refs.bib.\n- Keys are \
                 firstauthorYEARword, e.g. smith2021graph.\n",
                vec![skill(
                    "bibtex-tidy",
                    "Add and clean BibTeX entries.",
                    "Fetch the BibTeX for each paper, normalise the \
                     key, fix capitalisation in titles and remove \
                     duplicates.",
                )],
            ),
            settler(
                "gap-spotter",
                "Looks across summaries for open questions and \
                 contradictions.",
                "Curious and constructive. Frames gaps as possible \
                 research questions.",
                "# Notes\n\n- Keep gaps in gaps.md with the papers \
                 that point to each.\n",
                vec![skill(
                    "find-gaps",
                    "Find contradictions and open questions across \
                     papers.",
                    "Compare findings across summaries. List where \
                     papers disagree, what nobody has tested, and \
                     which methods are missing.",
                )],
            ),
        ],
    }
}

fn money_desk() -> Listing {
    Listing {
        name: "money-desk".into(),
        title: "Personal Finance Desk".into(),
        kind: "Personal finance".into(),
        description: "My household finance setup: it tracks \
                      spending from bank exports, watches for \
                      bills, prepares tax paperwork and nudges \
                      savings goals."
            .into(),
        purpose: "Keep a household's money organised: categorise \
                  spending, track bills and subscriptions, prepare \
                  tax documents and follow savings goals."
            .into(),
        seller: seller("Grace Liu", false),
        price: Price::Once(12.0),
        standing: standing(4.5, 96, 640),
        seed: 37,
        mayor: settler(
            "finance-lead",
            "Runs the monthly money review and assigns follow-ups.",
            "Calm and non-judgemental about spending. Focuses on \
             next steps.",
            "# Notes\n\n- Monthly review on the 1st.\n- Never move \
             money or log in to banks; only read exports.\n",
            vec![skill(
                "monthly-review",
                "Run a monthly money review.",
                "Summarise income, spending by category, bills paid \
                 and progress on goals. End with three suggested \
                 actions.",
            )],
        ),
        agents: vec![
            settler(
                "budget-keeper",
                "Categorises transactions from bank CSV exports.",
                "Accurate and consistent with categories.",
                "# Notes\n\n- Categories in categories.md.\n- Flag \
                 anything over $200 that isn't rent.\n",
                vec![skill(
                    "categorise-spending",
                    "Categorise bank transactions from a CSV.",
                    "Read the export, assign each transaction a \
                     category from categories.md and total spending \
                     per category.",
                )],
            ),
            settler(
                "bill-watcher",
                "Tracks recurring bills and subscriptions and their \
                 due dates.",
                "Watchful. Spots price increases and forgotten \
                 subscriptions.",
                "# Notes\n\n- Bills listed in bills.md with due day \
                 and amount.\n",
                vec![skill(
                    "subscription-audit",
                    "Find recurring charges and price changes.",
                    "Scan three months of transactions for repeats. \
                     List each subscription, its cost and any price \
                     change.",
                )],
            ),
            settler(
                "tax-prep",
                "Collects documents and drafts a checklist for tax \
                 season.",
                "Methodical. Never gives tax advice, only organises.",
                "# Notes\n\n- Documents go in tax/YEAR/.\n- Remind \
                 about deadlines a month ahead.\n",
                vec![skill(
                    "tax-checklist",
                    "Build a tax document checklist.",
                    "List the forms and receipts needed for the \
                     year, mark which are already in tax/YEAR/ and \
                     which are missing.",
                )],
            ),
            settler(
                "savings-coach",
                "Tracks savings goals and suggests realistic \
                 monthly amounts.",
                "Encouraging but realistic.",
                "# Notes\n\n- Goals in goals.md: emergency fund, \
                 holiday, new laptop.\n",
                vec![skill(
                    "savings-plan",
                    "Plan monthly savings towards goals.",
                    "Given income, spending and goals, suggest a \
                     monthly amount for each goal and the month it \
                     will be reached.",
                )],
            ),
        ],
    }
}

fn content_studio() -> Listing {
    Listing {
        name: "content-studio".into(),
        title: "Newsletter and Video Studio".into(),
        kind: "Content creation".into(),
        description: "The pipeline behind my weekly newsletter and \
                      YouTube channel. It finds topics, writes \
                      scripts, briefs thumbnails and turns each \
                      piece into social posts."
            .into(),
        purpose: "Help a solo creator publish a weekly newsletter \
                  and video: topic ideas, research, scripts, \
                  thumbnails and repurposed posts."
            .into(),
        seller: seller("Theo Marsh", true),
        price: Price::Monthly(5.0),
        standing: standing(4.6, 301, 2210),
        seed: 41,
        mayor: settler(
            "studio-lead",
            "Plans the weekly content calendar and keeps everyone \
             on schedule.",
            "Energetic and deadline-aware. Protects the creator's \
             voice.",
            "# Notes\n\n- Calendar in calendar.md.\n- Publish \
             newsletter Tuesday, video Thursday.\n",
            vec![skill(
                "content-calendar",
                "Plan a week of content.",
                "Pick one topic, assign research, script, thumbnail \
                 and repurposing tasks with deadlines before \
                 Tuesday and Thursday.",
            )],
        ),
        agents: vec![
            settler(
                "idea-scout",
                "Finds topic ideas from trends, comments and reader \
                 questions.",
                "Curious. Ranks ideas by audience interest, not \
                 novelty.",
                "# Notes\n\n- Keep ideas in ideas.md with a score \
                 out of 10.\n",
                vec![skill(
                    "topic-ideas",
                    "Find and score content ideas.",
                    "Collect ideas from comments, search trends and \
                     reader emails. Score each for interest and \
                     effort.",
                )],
            ),
            settler(
                "script-writer",
                "Writes newsletter drafts and video scripts in the \
                 creator's voice.",
                "Conversational and concise. Hooks in the first two \
                 lines.",
                "# Notes\n\n- Style guide in voice.md.\n- Videos \
                 run 8 to 12 minutes.\n",
                vec![skill(
                    "video-script",
                    "Write a video script from an outline.",
                    "Write a hook, three main points with examples \
                     and a call to action. Mark B-roll and \
                     on-screen text.",
                )],
            ),
            settler(
                "thumbnail-brief",
                "Writes thumbnail and title options for each video.",
                "Visual thinker. Tests several angles.",
                "# Notes\n\n- Max 4 words on a thumbnail.\n",
                vec![skill(
                    "thumbnail-ideas",
                    "Propose titles and thumbnail concepts.",
                    "Give five title options and three thumbnail \
                     concepts, each with the text and the image \
                     idea.",
                )],
            ),
            settler(
                "repurposer",
                "Turns each piece into posts for X, LinkedIn and \
                 Instagram.",
                "Adapts tone per platform without losing the point.",
                "# Notes\n\n- One thread, one LinkedIn post, three \
                 short clips per video.\n",
                vec![skill(
                    "repurpose-post",
                    "Turn long content into social posts.",
                    "Pull the strongest points and write a thread, \
                     a LinkedIn post and captions for short clips.",
                )],
            ),
        ],
    }
}

fn oss_maintainer() -> Listing {
    Listing {
        name: "oss-maintainer".into(),
        title: "Open Source Maintainer Kit".into(),
        kind: "Software maintenance".into(),
        description: "What I run on my open source library: it \
                      triages issues, checks pull requests, drafts \
                      release notes and fixes small docs problems."
            .into(),
        purpose: "Lighten an open source maintainer's load: triage \
                  issues, pre-review pull requests, write release \
                  notes and keep docs accurate."
            .into(),
        seller: seller("Lukas Brandt", true),
        price: Price::Free,
        standing: standing(4.9, 422, 3870),
        seed: 53,
        mayor: settler(
            "maintainer-lead",
            "Decides what needs a human and routes everything else.",
            "Respectful to contributors. Explains decisions briefly.",
            "# Notes\n\n- Never merge or close anything without \
             approval.\n- Weekly digest of new issues and PRs.\n",
            vec![skill(
                "weekly-digest",
                "Write a weekly digest of project activity.",
                "Summarise new issues, open PRs, releases and \
                 anything waiting on the maintainer.",
            )],
        ),
        agents: vec![
            settler(
                "issue-sorter",
                "Labels new issues, finds duplicates and asks for \
                 reproduction steps.",
                "Polite and patient with first-time reporters.",
                "# Notes\n\n- Labels: bug, feature, docs, question, \
                 needs-repro.\n",
                vec![skill(
                    "triage-issue",
                    "Triage a new issue.",
                    "Check for duplicates, suggest labels, and ask \
                     for version, OS and steps if missing.",
                )],
            ),
            settler(
                "pr-checker",
                "Does a first review of pull requests for tests, \
                 style and breaking changes.",
                "Encouraging but thorough.",
                "# Notes\n\n- Every PR needs a test and a changelog \
                 entry.\n",
                vec![skill(
                    "review-pr",
                    "Do a first-pass PR review.",
                    "Check for tests, breaking API changes and \
                     failing CI. Leave a friendly summary comment \
                     for the maintainer.",
                )],
            ),
            settler(
                "release-notes",
                "Drafts release notes from merged PRs.",
                "Plain language. Groups changes by what users \
                 notice.",
                "# Notes\n\n- Sections: Added, Changed, Fixed, \
                 Removed.\n",
                vec![skill(
                    "draft-release",
                    "Draft release notes from merged PRs.",
                    "List merged PRs since the last tag, group them \
                     by section and thank contributors by handle.",
                )],
            ),
            settler(
                "docs-fixer",
                "Fixes broken links, outdated examples and typos in \
                 the docs.",
                "Small, safe changes only.",
                "# Notes\n\n- Run examples before changing them.\n",
                vec![skill(
                    "fix-docs",
                    "Find and fix small documentation problems.",
                    "Check links and code examples, fix typos, and \
                     open one small PR per change.",
                )],
            ),
        ],
    }
}

fn job_hunt() -> Listing {
    Listing {
        name: "job-hunt".into(),
        title: "Job Hunt Helpers".into(),
        kind: "Career".into(),
        description: "I put this together while job hunting last \
                      year. It finds roles that match, tailors your \
                      CV, drafts cover letters and runs mock \
                      interviews."
            .into(),
        purpose: "Help someone find and apply for jobs: search \
                  listings, tailor applications, track progress and \
                  prepare for interviews."
            .into(),
        seller: seller("Sofia Romero", false),
        price: Price::Free,
        standing: standing(4.6, 158, 2050),
        seed: 67,
        mayor: settler(
            "career-lead",
            "Keeps the application tracker and plans each week's \
             applications.",
            "Supportive and practical. Keeps momentum without \
             pressure.",
            "# Notes\n\n- Tracker in applications.md.\n- Aim for \
             five strong applications a week.\n",
            vec![skill(
                "application-tracker",
                "Track job applications and next steps.",
                "Record each application with company, role, date, \
                 status and next step. Remind about follow-ups \
                 after a week.",
            )],
        ),
        agents: vec![
            settler(
                "job-scout",
                "Finds roles matching your skills, location and \
                 salary range.",
                "Selective. Explains why each role fits.",
                "# Notes\n\n- Preferences in preferences.md.\n",
                vec![skill(
                    "job-search",
                    "Find matching job listings.",
                    "Search job boards with the preferences. Keep \
                     ten roles with link, salary range and why it \
                     fits.",
                )],
            ),
            settler(
                "resume-tailor",
                "Tailors your CV to each job description.",
                "Honest. Highlights real experience, never invents \
                 it.",
                "# Notes\n\n- Master CV in cv.md.\n",
                vec![skill(
                    "tailor-cv",
                    "Tailor a CV to a job description.",
                    "Match the CV's wording to the job's key \
                     requirements and reorder bullet points by \
                     relevance.",
                )],
            ),
            settler(
                "cover-letter",
                "Drafts short cover letters for each application.",
                "Warm and specific. Under 250 words.",
                "# Notes\n\n- Mention one concrete thing about the \
                 company.\n",
                vec![skill(
                    "write-cover-letter",
                    "Write a cover letter for a role.",
                    "Open with why this company, give two relevant \
                     achievements, and close with a clear ask.",
                )],
            ),
            settler(
                "interview-coach",
                "Runs mock interviews and gives feedback.",
                "Friendly but honest about weak answers.",
                "# Notes\n\n- Use the STAR format for behavioural \
                 questions.\n",
                vec![skill(
                    "mock-interview",
                    "Run a mock interview for a role.",
                    "Ask five likely questions one at a time, then \
                     give feedback on structure, examples and \
                     clarity.",
                )],
            ),
        ],
    }
}
