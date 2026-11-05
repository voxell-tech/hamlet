//! Scholars: each one a repository of its own, its house the checkout
//! of its main branch.

pub(crate) mod repository;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use self::repository::{Author, Change, MAIN, Repository};
use crate::papers::NOTES;
use crate::schema::{record_scholar, record_scholar_seed};

/// A scholar's character, at the top of its house.
pub(crate) const CHARACTER: &str = "CHARACTER.md";

/// A scholar's number in the realm, never given twice.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[serde(transparent)]
pub struct ScholarId(pub u64);

impl std::fmt::Display for ScholarId {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A scholar as the record keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scholar {
    pub id: ScholarId,
    /// Its name while it lives; an archived scholar frees it.
    pub name: Option<String>,
    /// The chief it belongs to.
    pub chief: String,
    pub since: i64,
    pub left: Option<Left>,
}

/// When, by whom and why a scholar was archived, and its archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Left {
    pub at: i64,
    pub by: String,
    pub reason: String,
    pub archive: String,
}

type Row = (
    i64,
    Option<String>,
    String,
    i64,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

const COLUMNS: (
    record_scholar::id,
    record_scholar::name,
    record_scholar::chief,
    record_scholar::since,
    record_scholar::left_at,
    record_scholar::left_by,
    record_scholar::left_reason,
    record_scholar::archive,
) = (
    record_scholar::id,
    record_scholar::name,
    record_scholar::chief,
    record_scholar::since,
    record_scholar::left_at,
    record_scholar::left_by,
    record_scholar::left_reason,
    record_scholar::archive,
);

fn scholar(row: Row) -> Result<Scholar> {
    let (id, name, chief, since, at, by, reason, archive) = row;
    Ok(Scholar {
        id: ScholarId(u64::try_from(id)?),
        name,
        chief,
        since,
        left: match (at, by, reason, archive) {
            (Some(at), Some(by), Some(reason), Some(archive)) => {
                Some(Left {
                    at,
                    by,
                    reason,
                    archive,
                })
            }
            (None, ..) => None,
            _ => anyhow::bail!("scholar {id} left only in part"),
        },
    })
}

/// Makes the scholar `name` of `chief` in the transaction `db`, to
/// start its repository with `character`.
pub(crate) fn make(
    db: &mut SqliteConnection,
    name: &str,
    chief: &str,
    since: i64,
    character: &str,
) -> Result<ScholarId> {
    let id = diesel::insert_into(record_scholar::table)
        .values((
            record_scholar::name.eq(name),
            record_scholar::chief.eq(chief),
            record_scholar::since.eq(since),
        ))
        .returning(record_scholar::id)
        .get_result::<i64>(db)?;
    diesel::insert_into(record_scholar_seed::table)
        .values((
            record_scholar_seed::scholar.eq(id),
            record_scholar_seed::character.eq(character),
        ))
        .execute(db)?;
    Ok(ScholarId(u64::try_from(id)?))
}

/// The scholar `id`, living or archived.
pub(crate) fn find(
    db: &mut SqliteConnection,
    id: ScholarId,
) -> Result<Option<Scholar>> {
    record_scholar::table
        .find(i64::try_from(id.0)?)
        .select(COLUMNS)
        .first::<Row>(db)
        .optional()?
        .map(scholar)
        .transpose()
}

/// The living scholar named `name`.
pub(crate) fn named(
    db: &mut SqliteConnection,
    name: &str,
) -> Result<Option<Scholar>> {
    record_scholar::table
        .filter(record_scholar::name.eq(name))
        .select(COLUMNS)
        .first::<Row>(db)
        .optional()?
        .map(scholar)
        .transpose()
}

/// Every scholar, living and archived, oldest first.
pub(crate) fn all(db: &mut SqliteConnection) -> Result<Vec<Scholar>> {
    record_scholar::table
        .order(record_scholar::id)
        .select(COLUMNS)
        .load::<Row>(db)?
        .into_iter()
        .map(scholar)
        .collect()
}

/// The scholars whose repositories are not made yet, with the
/// character each starts with.
pub(crate) fn seeds(
    db: &mut SqliteConnection,
) -> Result<Vec<(ScholarId, String)>> {
    record_scholar_seed::table
        .order(record_scholar_seed::scholar)
        .select((
            record_scholar_seed::scholar,
            record_scholar_seed::character,
        ))
        .load::<(i64, String)>(db)?
        .into_iter()
        .map(|(id, character)| {
            Ok((ScholarId(u64::try_from(id)?), character))
        })
        .collect()
}

/// Forgets the seed of `id`, whose repository is made.
pub(crate) fn planted(
    db: &mut SqliteConnection,
    id: ScholarId,
) -> Result<()> {
    diesel::delete(
        record_scholar_seed::table.find(i64::try_from(id.0)?),
    )
    .execute(db)?;
    Ok(())
}

/// The repositories and houses of a realm's scholars, in its
/// `scholars` folder.
pub(crate) struct Scholars {
    root: PathBuf,
}

impl Scholars {
    /// The scholars of the realm whose folder is `realm`.
    pub fn new(realm: &Path) -> Self {
        Self {
            root: realm.join("scholars"),
        }
    }

    fn folder(&self, id: ScholarId) -> PathBuf {
        self.root.join(id.to_string())
    }

    /// The checkout of `id`'s main branch, which its hires read.
    pub fn house(&self, id: ScholarId) -> PathBuf {
        self.folder(id).join("house")
    }

    /// The repository of `id`.
    pub fn repository(&self, id: ScholarId) -> Result<Repository> {
        Repository::open(&self.folder(id).join("repo.git"))
    }

    /// Makes the repository of `id` with `character` and an empty
    /// notes file, and checks its house out. A repository that
    /// already has a main branch is kept as it is.
    pub fn initialize(
        &self,
        id: ScholarId,
        character: &str,
        by: Author<'_>,
    ) -> Result<()> {
        let path = self.folder(id).join("repo.git");
        let repository = match Repository::open(&path) {
            Ok(repository) if repository.tip(MAIN)?.is_some() => {
                return self.check_out(id, &repository);
            }
            // Left half made by a start that stopped.
            _ => {
                if path.exists() {
                    std::fs::remove_dir_all(&path)?;
                }
                Repository::create(&path)?
            }
        };
        repository.commit(
            MAIN,
            &[
                Change::Write {
                    path: CHARACTER.into(),
                    bytes: character.into(),
                },
                Change::Write {
                    path: NOTES.into(),
                    bytes: Vec::new(),
                },
            ],
            "Made the scholar",
            by,
        )?;
        self.check_out(id, &repository)
    }

    /// Makes the repositories still to be made from their seeds, and
    /// checks out the house of any living scholar that has none.
    pub fn plant(&self, db: &mut SqliteConnection) -> Result<()> {
        for (id, character) in seeds(db)? {
            let by = Author {
                name: "realm",
                at: crate::clock::now() / 1000,
            };
            self.initialize(id, &character, by)?;
            planted(db, id)?;
        }
        for scholar in all(db)? {
            if scholar.name.is_none()
                || self.house(scholar.id).exists()
            {
                continue;
            }
            let repository =
                self.repository(scholar.id).with_context(|| {
                    format!(
                        "scholar {} has no repository",
                        scholar.id
                    )
                })?;
            self.check_out(scholar.id, &repository)?;
        }
        Ok(())
    }

    /// The character of `id`, as its house holds it.
    pub fn character(&self, id: ScholarId) -> Result<String> {
        let path = self.house(id).join(CHARACTER);
        std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))
    }

    /// Makes `text` the character of `id`, a commit on its main
    /// branch by `by`.
    pub fn set_character(
        &self,
        id: ScholarId,
        text: &str,
        by: Author<'_>,
    ) -> Result<()> {
        let repository = self.repository(id)?;
        repository.commit(
            MAIN,
            &[Change::Write {
                path: CHARACTER.into(),
                bytes: text.into(),
            }],
            "Edited the character",
            by,
        )?;
        self.check_out(id, &repository)
    }

    /// Writes the files of `id`'s main branch over its house, which
    /// then holds them alone.
    pub fn check_out(
        &self,
        id: ScholarId,
        repository: &Repository,
    ) -> Result<()> {
        let tip = repository
            .tip(MAIN)?
            .with_context(|| format!("scholar {id} has no main"))?;
        let house = self.house(id);
        let fresh = house.with_extension("new");
        if fresh.exists() {
            std::fs::remove_dir_all(&fresh)?;
        }
        std::fs::create_dir_all(&fresh)?;
        for (path, bytes) in repository.files(tip)? {
            ensure!(
                Path::new(&path).components().all(|part| {
                    matches!(part, std::path::Component::Normal(_))
                }),
                "scholar {id} holds the path {path}, outside its \
                 house"
            );
            let to = fresh.join(&path);
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&to, bytes)?;
        }
        if house.exists() {
            std::fs::remove_dir_all(&house)?;
        }
        std::fs::rename(&fresh, &house)?;
        Ok(())
    }
}
