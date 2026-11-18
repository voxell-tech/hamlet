//! Changes to a profile's `main` and a worktree's files outside the
//! jobs, under the write lock.

use anyhow::Result;
use hamlet_git::{Author, Change as GitChange, MAIN, Repository};
use hamlet_store::ids::{Actor, AgentId, ProfileId};

use crate::core::{Core, UnpushedWorktree};
use crate::knowledge::folder::Folder;
use crate::knowledge::worktree;
use crate::realm::change::Done;
use crate::realm::jobs::{self, Job};
use crate::society::rejection::Rejection;
use crate::society::{addresses, hamlet_agents};

impl Core<'_, '_> {
    /// Commits `changes` to the `main` of `profile`.
    pub(crate) fn commit_main(
        &mut self,
        by: Actor,
        profile: ProfileId,
        changes: &[GitChange],
        message: &str,
    ) -> Result<()> {
        let path = self.realm.layout().repository(profile);
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
        self.main_moved(by, profile)
    }

    /// Has the idle agents of `profile` take in its `main`.
    pub(crate) fn main_moved(
        &mut self,
        by: Actor,
        profile: ProfileId,
    ) -> Result<()> {
        self.change(by, |cx, _| {
            for agent in hamlet_agents::living_of(cx.db, profile)? {
                if !crate::conversations::inputs::is_agent_busy(
                    cx.db,
                    agent.agent,
                )? {
                    jobs::add(
                        cx,
                        &Job::TakeIn { agent: agent.agent },
                    )?;
                }
            }
            cx.touched.profiles.insert(profile);
            Ok(())
        })?;
        Ok(())
    }

    /// Writes the memory file `path` of `agent`'s worktree; empty
    /// removes it.
    pub(crate) fn write_worktree_memory(
        &mut self,
        by: Actor,
        agent: AgentId,
        path: &str,
        text: &str,
    ) -> Result<()> {
        let layout = self.realm.layout().clone();
        let worktree = worktree(
            self.realm.store().connection(),
            &layout,
            agent,
        )?;
        {
            let writing = self.writing.clone();
            let _held = writing.hold();
            let folder = Folder::make(&worktree.path)?;
            if text.is_empty() {
                folder.remove(path)?;
            } else {
                folder.write(path, text.as_bytes(), false)?;
            }
        }
        self.remote.unpushed.write(UnpushedWorktree(agent));
        self.change(by, |cx, _| {
            jobs::add(cx, &Job::Commit { agent })?;
            Ok(())
        })
    }

    /// Takes `path` out of the history of the profile `id` and its
    /// agents' worktrees.
    pub(crate) fn purge_path(
        &mut self,
        by: Actor,
        id: ProfileId,
        path: &str,
    ) -> Done<String> {
        let path = path.trim_matches('/');
        if path.is_empty()
            || path.split('/').any(|part| part == ".." || part == ".")
        {
            return Ok(Err(Rejection::NoFile { path: path.into() }));
        }
        let layout = self.realm.layout().clone();
        let writing = self.writing.clone();
        let held = writing.hold();
        let db = self.realm.store().connection();
        for agent in hamlet_agents::living_of(db, id)? {
            let worktree =
                layout.worktree(&agent.hamlet, agent.agent);
            let Ok(folder) = Folder::open(&worktree) else {
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
        for agent in hamlet_agents::living_of(db, id)? {
            self.remote.unpushed.write(UnpushedWorktree(agent.agent));
        }
        self.main_moved(by, id)?;
        let label = addresses::profile_name_of(
            self.realm.store().connection(),
            id,
        )?;
        Ok(Ok(format!(
            "Purged {path} from {label}: {} branches rewritten.",
            rewritten.len()
        )))
    }
}
