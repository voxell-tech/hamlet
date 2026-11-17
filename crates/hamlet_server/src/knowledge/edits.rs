//! A scholar's `main` and a lodge's files changed outside the jobs,
//! under the write lock: commits, a lodge's memory and purges.

use anyhow::Result;
use hamlet_git::{Author, Change as GitChange, MAIN, Repository};
use hamlet_store::ids::{Actor, AgentId, ScholarId};

use crate::core::{Core, UnpushedLodge};
use crate::knowledge::folder::Folder;
use crate::knowledge::lodging;
use crate::realm::change::Done;
use crate::realm::jobs::{self, Job};
use crate::society::refusal::Refusal;
use crate::society::{addresses, hires};

impl Core<'_, '_> {
    /// Commits `changes` to the `main` of `scholar` as `by`'s, and
    /// has its idle hires take them in.
    pub(crate) fn commit_main(
        &mut self,
        by: Actor,
        scholar: ScholarId,
        changes: &[GitChange],
        message: &str,
    ) -> Result<()> {
        let path = self.realm.layout().repository(scholar);
        let author = by.to_string();
        let writing = self.writing.clone();
        let held = writing.hold();
        let repo = Repository::open(&path)?;
        repo.commit(
            MAIN,
            changes,
            message,
            Author {
                name: &author,
                at: crate::app::clock::now(),
            },
        )?;
        drop(held);
        self.main_moved(by, scholar)
    }

    /// Has the idle hires of `scholar` take in its `main`, which `by`
    /// moved.
    pub(crate) fn main_moved(
        &mut self,
        by: Actor,
        scholar: ScholarId,
    ) -> Result<()> {
        self.change(by, |cx, _| {
            for hire in hires::living_of(cx.db, scholar)? {
                if !crate::conversations::inputs::is_agent_busy(
                    cx.db, hire.agent,
                )? {
                    jobs::add(cx, &Job::TakeIn { hire: hire.agent })?;
                }
            }
            cx.touched.scholars.insert(scholar);
            Ok(())
        })?;
        Ok(())
    }

    /// Writes `text` to the memory file `path` of the lodge of
    /// `agent`, as `by` asks, empty removing it, and has the lodge
    /// committed.
    pub(crate) fn write_lodge_memory(
        &mut self,
        by: Actor,
        agent: AgentId,
        path: &str,
        text: &str,
    ) -> Result<()> {
        let layout = self.realm.layout().clone();
        let lodging =
            lodging(self.realm.store().connection(), &layout, agent)?;
        {
            let writing = self.writing.clone();
            let _held = writing.hold();
            let folder = Folder::make(&lodging.path)?;
            if text.is_empty() {
                folder.remove(path)?;
            } else {
                folder.write(path, text.as_bytes(), false)?;
            }
        }
        self.remote.unpushed.write(UnpushedLodge(agent));
        self.change(by, |cx, _| {
            jobs::add(cx, &Job::Commit { hire: agent })?;
            Ok(())
        })
    }

    /// Takes `path` out of every commit, every branch and every lodge
    /// of the hires of the scholar `id`, as `by` asks.
    pub(crate) fn purge_path(
        &mut self,
        by: Actor,
        id: ScholarId,
        path: &str,
    ) -> Done<String> {
        let path = path.trim_matches('/');
        if path.is_empty()
            || path.split('/').any(|part| part == ".." || part == ".")
        {
            return Ok(Err(Refusal::NoFile { path: path.into() }));
        }
        let layout = self.realm.layout().clone();
        let writing = self.writing.clone();
        let held = writing.hold();
        let db = self.realm.store().connection();
        for hire in hires::living_of(db, id)? {
            let lodge = layout.lodge(&hire.hamlet, hire.agent);
            let Ok(folder) = Folder::open(&lodge) else {
                continue;
            };
            // The path is a file, or a folder of them.
            for entry in folder.list_under(path)? {
                folder.remove(&entry.path)?;
            }
            folder.remove(path)?;
        }
        let rewritten =
            hamlet_git::purge(&layout.repository(id), path)?;
        drop(held);
        let db = self.realm.store().connection();
        for hire in hires::living_of(db, id)? {
            self.remote.unpushed.write(UnpushedLodge(hire.agent));
        }
        self.main_moved(by, id)?;
        let label = addresses::scholar_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(Ok(format!(
            "Purged {path} from {label}: {} branches rewritten.",
            rewritten.len()
        )))
    }
}
