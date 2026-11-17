//! A scholar of the city made of what a hire has become (H38): a
//! blank hire's own scholar let into the city, or a new scholar
//! forked from a city scholar's hire.

use anyhow::Result;
use hamlet_git::{MAIN, Repository};
use hamlet_store::ids::{Actor, AgentId};

use super::{Answer, Line};
use crate::core::Core;
use crate::knowledge::{self, CHARACTER};
use crate::society::addresses::Reach;
use crate::society::city::{self, Arrival};
use crate::society::hires;

/// What a scholar made of a hire starts from.
pub(crate) struct Source {
    pub name: String,
    /// The tip of the hire's branch, which a new scholar is forked
    /// from; none for a blank hire, whose own scholar joins the
    /// city.
    pub commit: Option<String>,
    pub character: String,
}

/// `/make-scholar <address> [name]`.
pub(super) fn make_scholar(
    core: &mut Core<'_, '_>,
    line: &Line,
) -> Answer {
    let Some(text) = line.words().first().copied() else {
        return Ok(Err(core.deeds.usage(&line.name)));
    };
    let hire =
        match core.resolve(text, line.here.as_ref(), Reach::Kept)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
    if let Err(refusal) = core.leads(hire)? {
        return Ok(Err(refusal));
    }
    let source = core.scholar_source(hire, &line.tail(1))?;
    let chief = core.realm.chief();
    let made = core.attempt(Actor::Chief(chief), |cx, _| {
        city::make_of_hire(
            cx,
            hire,
            &source.name,
            chief,
            source.commit,
            "",
        )
    })?;
    let db = core.realm.store().connection();
    Ok(match made {
        Ok(id) => Ok(format!(
            "{} lives in the city, made of what `{text}` learned.",
            city::get(db, id)?.label()
        )),
        Err(refusal) => Err(refusal),
    })
}

impl Core<'_, '_> {
    /// What the scholar made of `hire` starts from, named `name`,
    /// else as the hire is called. A city scholar's hire has its
    /// lodge committed first, so its branch's tip holds all it
    /// learned.
    pub(crate) fn scholar_source(
        &mut self,
        hire: AgentId,
        name: &str,
    ) -> Result<Source> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let found = hires::get(db, hire)?;
        let scholar = city::get(db, found.scholar)?;
        let name = match name.trim() {
            "" => found.nickname.unwrap_or(scholar.name),
            given => given.to_owned(),
        };
        let writing = self.writing.clone();
        let held = writing.hold();
        let db = self.realm.store().connection();
        // A scholar whose repository is not made yet has nothing to
        // fork, and a blank hire's joins with what its branch holds
        // once it is.
        let Ok(repository) =
            Repository::open(&layout.repository(found.scholar))
        else {
            return Ok(Source {
                name,
                commit: None,
                character: String::new(),
            });
        };
        let commit = if scholar.arrival == Arrival::Apart {
            None
        } else {
            knowledge::run::commit(
                db,
                &layout,
                hire,
                crate::app::clock::now(),
            )?;
            repository.tip(&knowledge::branch(hire))?
        };
        let character = match commit.or(repository.tip(MAIN)?) {
            Some(at) => {
                repository.read(at, CHARACTER)?.unwrap_or_default()
            }
            None => Vec::new(),
        };
        drop(held);
        Ok(Source {
            name,
            commit: commit.map(|commit| commit.to_string()),
            character: String::from_utf8_lossy(&character)
                .into_owned(),
        })
    }
}
