//! A few listings as the bazaar's server would send them, for the
//! gallery's pictures and the tests. Nothing the app shows a chief
//! comes from here.

use serde_json::{Value, json};

use super::lot::{Kind, Lot, Maker, Release, Status};

fn maker(login: &str, name: &str, verified: bool) -> Maker {
    Maker {
        id: format!("u_{login}"),
        login: login.into(),
        name: Some(name.into()),
        avatar_url: None,
        verified,
    }
}

#[allow(clippy::too_many_arguments)]
fn lot(
    kind: Kind,
    id: &str,
    (title, summary, category): (&str, &str, &str),
    seller: Maker,
    price_cents: u32,
    (stars, reviews, owners): (f32, u32, u32),
    (version, day): (&str, &str),
    meta: Value,
) -> Lot {
    let at = format!("{day}T09:00:00Z");
    Lot {
        id: id.into(),
        kind,
        title: title.into(),
        summary: summary.into(),
        description: format!(
            "{summary} Made with care, and kept up."
        ),
        category: category.into(),
        tags: vec![category.to_lowercase()],
        price_cents,
        status: Status::Listed,
        seller,
        cover_url: None,
        latest: Some(Release {
            version: version.into(),
            changes: "First release.".into(),
            size: 4096,
            sha256: String::new(),
            created_at: at.clone(),
            meta,
        }),
        owners,
        stars,
        reviews,
        created_at: at.clone(),
        updated_at: at,
    }
}

fn agent(name: &str, purpose: &str, skills: &[&str]) -> Value {
    json!({
        "name": name,
        "purpose": purpose,
        "model": "anthropic/claude-sonnet-4",
        "character": format!("{purpose} Speaks plainly, checks its work."),
        "skills": skills,
        "notes": true,
    })
}

/// The sample listings of `kind`.
pub fn lots(kind: Kind) -> Vec<Lot> {
    let guild = maker("dev-guild", "Dev Tooling Collective", true);
    let wren = maker("wren", "Wren Halloway", false);
    let ash = maker("ashgrove", "Lady Ashgrove", true);
    match kind {
        Kind::Agent => vec![
            lot(
                kind,
                "l_pr_reviewer",
                (
                    "PR Reviewer",
                    "Reviews pull requests for bugs, risky changes \
                     and missing tests.",
                    "Code",
                ),
                guild,
                900,
                (4.7, 128, 1940),
                ("1.4.0", "2026-10-02"),
                agent(
                    "pr-reviewer",
                    "Reviews pull requests.",
                    &["read-diff", "comment"],
                ),
            ),
            lot(
                kind,
                "l_ledger",
                (
                    "Ledger Keeper",
                    "Keeps the household accounts and says where \
                     the money went.",
                    "Personal",
                ),
                wren.clone(),
                0,
                (4.2, 31, 610),
                ("0.3.1", "2026-09-14"),
                agent("ledger", "Keeps accounts.", &["sum-up"]),
            ),
            lot(
                kind,
                "l_tutor",
                (
                    "Patient Tutor",
                    "Teaches one topic at a time, and checks you \
                     followed.",
                    "Learning",
                ),
                wren,
                450,
                (4.9, 12, 88),
                ("1.0.0", "2026-08-30"),
                agent("tutor", "Teaches.", &["quiz"]),
            ),
        ],
        Kind::Plugin => vec![
            lot(
                kind,
                "l_calendar",
                (
                    "Calendar",
                    "Lets agents read and add events to a calendar \
                     file.",
                    "Productivity",
                ),
                guild,
                0,
                (4.5, 40, 2300),
                ("2.1.0", "2026-10-05"),
                json!({"tools": [{
                    "name": "calendar",
                    "description": "Reads and adds events.",
                    "permissions": ["run_program", "write_files"],
                }]}),
            ),
            lot(
                kind,
                "l_charts",
                (
                    "Charts",
                    "Draws a chart from a table and saves it as a \
                     picture.",
                    "Data",
                ),
                wren,
                300,
                (4.1, 9, 140),
                ("0.9.2", "2026-09-21"),
                json!({"tools": [{
                    "name": "chart",
                    "description": "Draws a chart.",
                    "permissions": ["run_program", "read_files"],
                }, {
                    "name": "fetch-table",
                    "description": "Fetches a table from the web.",
                    "permissions": ["run_program", "network"],
                }]}),
            ),
        ],
        Kind::Hamlet => vec![
            lot(
                kind,
                "l_lamplight",
                (
                    "Lamplight Press",
                    "A small press that edits, sets and publishes \
                     your book.",
                    "Writing",
                ),
                ash.clone(),
                1900,
                (4.8, 64, 900),
                ("1.2.0", "2026-10-01"),
                json!({
                    "name": "lamplight",
                    "title": "Lamplight Press",
                    "kind": "Publishing house",
                    "purpose": "Turns a manuscript into a book.",
                    "seed": "7341",
                    "mayor": agent("publisher", "Runs the press.", &["plan"]),
                    "agents": [
                        agent("line-editor", "Edits line by line.", &["edit"]),
                        agent("typesetter", "Sets the pages.", &["typst"]),
                    ],
                }),
            ),
            lot(
                kind,
                "l_bramble",
                (
                    "Bramble Lab",
                    "A research hamlet that reads papers and writes \
                     reviews.",
                    "Research",
                ),
                ash,
                0,
                (4.4, 22, 310),
                ("0.5.0", "2026-09-10"),
                json!({
                    "name": "bramble",
                    "title": "Bramble Lab",
                    "kind": "Research lab",
                    "purpose": "Reviews the literature.",
                    "seed": "90210",
                    "mayor": agent("head", "Runs the lab.", &[]),
                    "agents": [agent("reader", "Reads papers.", &["cite"])],
                }),
            ),
        ],
    }
}
