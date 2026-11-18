//! The library, a repository of skills every agent shares (H33).

use anyhow::{Context, Result};
use hamlet_git::{Author, Change, CommitId, MAIN, Repository};
use hamlet_store::ids::Actor;

use super::folder::Folder;
use super::run::write_tree;
use crate::core::Core;
use crate::realm::change::Done;
use crate::realm::layout::Layout;
use crate::society::library::{self as shelf, Event, EventKind};
use crate::society::rejection::Rejection;

macro_rules! starter {
    ($name:literal, [$($file:literal),+]) => {
        ($name, &[$(($file, include_str!(concat!("../../assets/starters/", $name, "/", $file)))),+])
    };
}

/// A starter skill: its name and its files.
type Starter =
    (&'static str, &'static [(&'static str, &'static str)]);

/// The skills every realm's library starts with.
const STARTERS: &[Starter] = &[
    starter!("web-search", ["SKILL.md", "scripts/search.py"]),
    starter!("read-webpage", ["SKILL.md", "scripts/read.py"]),
    starter!("write-skill", ["SKILL.md"]),
    starter!("manage-memory", ["SKILL.md"]),
];

/// The most a skill's files may hold, together.
pub(crate) const MOST_BYTES: usize = 4 * 1024 * 1024;

/// The value of `key` in the front matter of `paper`.
pub(crate) fn field(paper: &str, key: &str) -> String {
    let head = paper.strip_prefix("---").unwrap_or_default();
    let head = head.split("\n---").next().unwrap_or_default();
    head.lines()
        .find_map(|line| line.trim().strip_prefix(key))
        .map(|v| v.trim().trim_matches('"').to_owned())
        .unwrap_or_default()
}

fn author(at: i64) -> Author<'static> {
    Author { name: "realm", at }
}

/// Makes the library with the starters when it has no `main`, and
/// checks its `main` out.
pub(crate) fn open(layout: &Layout, at: i64) -> Result<()> {
    let path = layout.library_repository();
    let repo = match Repository::open(&path) {
        Ok(repo) => repo,
        Err(_) => Repository::create(&path)?,
    };
    if repo.tip(MAIN)?.is_none() {
        let changes = STARTERS
            .iter()
            .flat_map(|(name, files)| {
                files.iter().map(move |(file, text)| Change::Write {
                    path: format!("{name}/{file}"),
                    bytes: text.as_bytes().to_vec(),
                })
            })
            .collect::<Vec<_>>();
        repo.commit(
            MAIN,
            &changes,
            "The starter skills",
            author(at),
        )?;
    }
    check_out(layout, &repo)
}

/// Makes `library/` hold the files of the library's `main`.
fn check_out(layout: &Layout, repo: &Repository) -> Result<()> {
    let tip = repo.tip(MAIN)?.context("the library has no main")?;
    write_tree(repo, tip, &Folder::make(&layout.library())?, &|_| {
        false
    })
}

/// Every shelved skill, by its folder, with its description.
pub(crate) fn skills(
    layout: &Layout,
) -> Result<Vec<(String, String)>> {
    let Ok(repo) = Repository::open(&layout.library_repository())
    else {
        return Ok(Vec::new());
    };
    let Some(tip) = repo.tip(MAIN)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in repo.entries(tip)? {
        if let Some((name, "SKILL.md")) = entry.path.split_once('/') {
            let paper =
                String::from_utf8_lossy(&repo.blob(entry.blob)?)
                    .into_owned();
            out.push((
                name.to_owned(),
                field(&paper, "description:"),
            ));
        }
    }
    Ok(out)
}

/// Whether the library holds a skill `name`.
pub(crate) fn has(layout: &Layout, name: &str) -> Result<bool> {
    Ok(skills(layout)?.iter().any(|(each, _)| each == name))
}

/// Keeps the files of an offered skill apart until it is judged, on
/// the ref `offers/<offer>`, returning that commit.
pub(crate) fn keep_offer(
    layout: &Layout,
    offer: i64,
    files: &[(String, Vec<u8>)],
    at: i64,
) -> Result<CommitId> {
    let repo = Repository::open(&layout.library_repository())?;
    let changes = files
        .iter()
        .map(|(path, bytes)| Change::Write {
            path: path.clone(),
            bytes: bytes.clone(),
        })
        .collect::<Vec<_>>();
    repo.commit(
        &format!("offers/{offer}"),
        &changes,
        "Offered",
        author(at),
    )
}

/// The files of the offer kept at `commit`.
pub(crate) fn offered(
    layout: &Layout,
    commit: &str,
) -> Result<Vec<(String, Vec<u8>)>> {
    let repo = Repository::open(&layout.library_repository())?;
    let commit =
        commit.parse::<CommitId>().map_err(anyhow::Error::msg)?;
    repo.files(commit)
}

/// Shelves `files` as the skill `name`, by `by`, and checks the
/// library out.
pub(crate) fn shelve(
    layout: &Layout,
    name: &str,
    files: &[(String, Vec<u8>)],
    message: &str,
    by: &str,
    at: i64,
) -> Result<CommitId> {
    let repo = Repository::open(&layout.library_repository())?;
    let changes = files
        .iter()
        .map(|(path, bytes)| Change::Write {
            path: format!("{name}/{path}"),
            bytes: bytes.clone(),
        })
        .collect::<Vec<_>>();
    let commit = repo.commit(
        MAIN,
        &changes,
        message,
        Author { name: by, at },
    )?;
    check_out(layout, &repo)?;
    Ok(commit)
}

/// Takes the skill `name` off the shelf, by `by`, and checks the
/// library out.
pub(crate) fn unshelve(
    layout: &Layout,
    name: &str,
    message: &str,
    by: &str,
    at: i64,
) -> Result<Option<CommitId>> {
    let repo = Repository::open(&layout.library_repository())?;
    let tip = repo.tip(MAIN)?.context("the library has no main")?;
    let prefix = format!("{name}/");
    let changes = repo
        .entries(tip)?
        .into_iter()
        .filter(|entry| entry.path.starts_with(&prefix))
        .map(|entry| Change::Remove { path: entry.path })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return Ok(None);
    }
    let commit = repo.commit(
        MAIN,
        &changes,
        message,
        Author { name: by, at },
    )?;
    check_out(layout, &repo)?;
    Ok(Some(commit))
}

impl Core<'_, '_> {
    /// Takes `skill` off the library's shelves, as `by` asks.
    pub(crate) fn unshelve_skill(
        &mut self,
        by: Actor,
        skill: &str,
        reason: &str,
    ) -> Done<String> {
        let layout = self.realm.layout().clone();
        let author = by.to_string();
        let at = crate::app::clock::now();
        let message = if reason.is_empty() {
            format!("Took {skill} off the shelves")
        } else {
            format!("Took {skill} off the shelves: {reason}")
        };
        let writing = self.writing.clone();
        let held = writing.hold();
        let unshelved =
            unshelve(&layout, skill, &message, &author, at)?;
        drop(held);
        let Some(commit) = unshelved else {
            return Ok(Err(Rejection::NoSkill {
                name: skill.into(),
            }));
        };
        let (agent, chief) = match by {
            Actor::Agent(agent) => (Some(agent), None),
            Actor::Chief(chief) => (None, Some(chief)),
            Actor::Realm => (None, None),
        };
        let db = self.realm.store().connection();
        shelf::event(
            db,
            at,
            Event {
                kind: EventKind::Unshelved,
                skill,
                agent,
                chief,
                reason: (!reason.is_empty()).then_some(reason),
                commit: Some(&commit.to_string()),
            },
        )?;
        Ok(Ok(format!("Took {skill} off the shelves.")))
    }
}
