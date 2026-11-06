//! The library (H33): a repository of skills every agent shares,
//! whose `main` is checked out read-only at `library/`. Its first
//! commit holds the starter skills built into the server.

use anyhow::{Context, Result};
use hamlet_git::{Author, Change, CommitId, MAIN, Repository, Step};

use super::folder::Folder;
use crate::layout::Layout;

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
    starter!("weather", ["SKILL.md", "scripts/weather.py"]),
    starter!("call-api", ["SKILL.md", "scripts/api.py"]),
    starter!("system-info", ["SKILL.md", "scripts/info.py"]),
    starter!("write-skill", ["SKILL.md"]),
    starter!("manage-memory", ["SKILL.md"]),
    starter!("write-typst", ["SKILL.md"]),
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
    let folder = Folder::make(&layout.library())?;
    let listing = folder.list(&|_| false)?;
    for step in repo.plan(tip, &listing, &|_| false)? {
        match step {
            Step::Write {
                path,
                blob,
                executable,
            } => {
                folder.write(&path, &repo.blob(blob)?, executable)?;
            }
            Step::Remove { path } => folder.remove(&path)?,
        }
    }
    Ok(())
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
/// library out. `None` when it was not shelved.
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
