//! Agents leaving the hamlet and coming back (H5, H7, H17, H20):
//! removing, merging, restoring and erasing. A change appends one
//! line to the record and then moves files; `Hamlet::mend` moves the
//! files a crash left unmoved.

mod archive;
mod history;
mod notes;

use anyhow::{Result, anyhow, bail};
use hamlet_model::log;
use serde_json::{Value, json};

use crate::files::size_of;
use crate::hamlet::{
    Alone, Archived, Census, Hamlet, MAYOR, Resident, check_name,
};
use crate::lifecycle::notes::{accept, join, substance};
use crate::realm::Realm;
use crate::record::when;

/// The values the words about leaving and returning are tuned by.
/// Those a chief decides are in the limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Most agents that left a prompt names.
    pub gone_shown: usize,
    /// Most characters of a chief's task a refusal quotes.
    pub quoted: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            gone_shown: 5,
            quoted: 80,
        }
    }
}

/// The one who decides a change.
#[derive(Clone, Copy, Debug)]
pub enum Who<'a> {
    Mayor,
    /// The mayor, for an act the chief reserved and then allowed.
    Allowed,
    Chief(&'a str),
}

impl Who<'_> {
    fn name(&self) -> &str {
        match self {
            Who::Mayor | Who::Allowed => MAYOR,
            Who::Chief(name) => name,
        }
    }
}

/// The outcome of a merge.
#[derive(Clone, Debug, PartialEq)]
pub struct Merged {
    /// The agent that stays, with its new purpose.
    pub kept: Resident,
    pub gone: Resident,
    /// The absorbed agent's folder under `archive/`.
    pub archive: String,
    /// The two notes were joined end to end and want tidying,
    /// because no rewritten notes were given or they were refused.
    pub joined: bool,
    /// Skills and files of the absorbed agent that differed from
    /// the kept one's of the same name, and where each went.
    pub clashes: Vec<String>,
}

impl Merged {
    /// The outcome in words, for whoever decided it.
    pub fn report(&self) -> String {
        let (kept, gone) = (&self.kept.name, &self.gone.name);
        let mut text = format!(
            "Merged `{gone}` into `{kept}`. `{kept}` has the notes, \
             skills, files and roads of both, and its purpose is \
             now: {} `{gone}` is archived as {}.",
            self.kept.purpose, self.archive
        );
        if self.joined {
            text.push_str(&format!(
                " The two notes were joined, not rewritten: ask \
                 `{kept}` to tidy its NOTES.md."
            ));
        }
        if !self.clashes.is_empty() {
            text.push_str(&format!(
                " Kept apart because they differed: {}.",
                self.clashes.join("; ")
            ));
        }
        text
    }
}

/// The outcome of a restore.
#[derive(Clone, Debug, PartialEq)]
pub struct Restored {
    pub resident: Resident,
    /// The agent it was split from, when it had been merged.
    pub split_from: Option<String>,
    /// The agents its roads lead to after the restore.
    pub roads: Vec<String>,
    /// The roads the mayor has built and the most the chief allows,
    /// when the restore brought the hamlet past it.
    pub over_roads: Option<(usize, usize)>,
}

impl Restored {
    pub fn report(&self) -> String {
        let name = &self.resident.name;
        let roads = self.roads.join(", ");
        let over = match self.over_roads {
            Some((built, most)) => format!(
                " The hamlet now has {built} roads between agents, \
                 more than the {most} your limits allow. They all \
                 work; the mayor can build no more until you raise \
                 `most_roads` in limits.toml."
            ),
            None => String::new(),
        };
        let done = match &self.split_from {
            None => format!(
                "Restored `{name}` with its house as it was. Its \
                 roads lead to: {roads}."
            ),
            Some(kept) => format!(
                "Split `{name}` from `{kept}`. `{name}` has its \
                 house as it was before the merge, and `{kept}` its \
                 old purpose and whatever it has learned since. \
                 `{name}`'s roads lead to: {roads}."
            ),
        };
        done + &over
    }
}

impl Hamlet {
    /// The living agent `name`, or why there is none.
    pub(crate) fn living(
        &self,
        census: &Census,
        name: &str,
    ) -> Result<Resident> {
        if name == MAYOR || name == census.mayor.name {
            bail!(
                "the mayor cannot be removed or merged; replacing a \
                 mayor takes a chief or a vote"
            );
        }
        census
            .residents
            .iter()
            .find(|r| r.name == name)
            .cloned()
            .ok_or_else(|| match census.gone(name) {
                Some(a) => anyhow!(a.farewell()),
                None => anyhow!("no agent named `{name}`"),
            })
    }

    /// Refuses a departure with no reason given, or one the mayor
    /// decides while the agent has a chief's task (H7).
    fn may_part(
        &self,
        census: &Census,
        resident: &Resident,
        reason: &str,
        by: Who,
    ) -> Result<()> {
        if reason.trim().is_empty() {
            bail!("give a `reason`; it is kept in the record");
        }
        if census.at_root(&resident.name) {
            bail!(
                "the house of `{}` is the hamlet's own folder, so \
                 it cannot be removed or merged",
                resident.name
            );
        }
        // A chief's leave for a reserved act does not lift this.
        if let Who::Chief(_) = by {
            return Ok(());
        }
        let hold_ms = self
            .limits()?
            .leaving
            .protected_minutes
            .saturating_mul(60_000);
        if let Some(task) =
            census.owed(resident.id, log::now_millis(), hold_ms)
        {
            let quote = task
                .text
                .chars()
                .take(self.lifecycle.quoted)
                .collect::<String>();
            bail!(
                "`{}` has a task from the chief {} (\"{quote}\") \
                 that {}. No agent may be removed or merged to \
                 avoid a chief's task. Wait until the chief has had \
                 the answer, or ask the chief to do this.",
                resident.name,
                task.user,
                if task.done.is_none() {
                    "is not finished"
                } else {
                    "has only just ended"
                }
            );
        }
        Ok(())
    }

    /// The words for someone who tries to reach `name`, when it has
    /// left the hamlet.
    pub fn whereabouts(&self, name: &str) -> Option<String> {
        self.census().gone(name).map(Archived::farewell)
    }

    /// The agent `id` as it is now. `Err` says how it left.
    pub(crate) fn still_here(
        &self,
        id: u64,
        name: &str,
    ) -> Result<Resident> {
        let census = self.census();
        census
            .residents
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| {
                let left = census.archive.iter().rev().find(|a| {
                    a.resident.id == id && a.back.is_none()
                });
                match left {
                    Some(a) => anyhow!(a.farewell()),
                    None => anyhow!("`{name}` is not in the hamlet"),
                }
            })
    }

    /// Agents that left and have not come back, oldest first.
    pub fn archived(&self) -> Vec<Archived> {
        self.census()
            .archive
            .into_iter()
            .filter(|a| a.back.is_none())
            .collect()
    }

    /// Removes an agent at once (H5). Its house is archived and its
    /// roads go with it. The `reason` is kept in the record (H17).
    pub fn remove(
        &self,
        name: &str,
        reason: &str,
        by: Who,
    ) -> Result<Archived> {
        self.mend();
        let census = self.census();
        let resident = self.living(&census, name)?;
        if let Who::Mayor = by {
            self.mayor_may(Alone::RemoveAgents)?;
        }
        self.may_part(&census, &resident, reason, by)?;
        let (id, _) = self.new_shelf(name)?;
        self.append(json!({
            "v": 1,
            "type": "agent_removed",
            "ts": log::now_millis(),
            "name": name,
            "reason": reason.trim(),
            "by": by.name(),
            "archive": id,
        }))?;
        self.mend();
        self.census()
            .archive
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| {
                // Another session's line came first.
                let _ = std::fs::remove_dir(self.shelf(&id));
                anyhow!("`{name}` had left the hamlet already")
            })
    }

    /// Refuses a merge that may not happen, before anything is
    /// spent on it.
    pub fn can_merge(
        &self,
        keep: &str,
        absorb: &str,
        purpose: &str,
        reason: &str,
        by: Who,
    ) -> Result<()> {
        self.merging(
            &self.census(),
            keep,
            absorb,
            purpose,
            reason,
            by,
        )
        .map(|_| ())
    }

    fn merging(
        &self,
        census: &Census,
        keep: &str,
        absorb: &str,
        purpose: &str,
        reason: &str,
        by: Who,
    ) -> Result<(Resident, Resident)> {
        if keep == absorb {
            bail!("an agent cannot be merged with itself");
        }
        let kept = self.living(census, keep)?;
        let gone = self.living(census, absorb)?;
        if purpose.trim().is_empty() {
            bail!("say what the merged agent is for in `purpose`");
        }
        if let Who::Mayor = by {
            self.mayor_may(Alone::MergeAgents)?;
        }
        self.may_part(census, &gone, reason, by)?;
        self.may_part(census, &kept, reason, by)?;
        Ok((kept, gone))
    }

    /// Merges `absorb` into `keep` at once (H5): `keep` takes
    /// `purpose` and the contents and roads of both, and `absorb` is
    /// archived. Without usable `notes` the two NOTES.md are joined
    /// end to end.
    pub fn merge(
        &self,
        keep: &str,
        absorb: &str,
        purpose: &str,
        reason: &str,
        by: Who,
        notes: Option<&str>,
    ) -> Result<Merged> {
        self.mend();
        let census = self.census();
        let (kept, gone) =
            self.merging(&census, keep, absorb, purpose, reason, by)?;
        let (mine, theirs) = (
            self.notes_of(&census, keep),
            self.notes_of(&census, absorb),
        );
        let both = !substance(&mine).is_empty()
            && !substance(&theirs).is_empty();
        let limit = self.limits()?.leaving.merged_notes;
        let written = notes.and_then(|n| accept(n, limit));
        let joined = both && written.is_none();
        let body =
            written.unwrap_or_else(|| join(&mine, absorb, &theirs));
        let text = format!(
            "{}\n\n## Merged\n- {}: `{absorb}` was merged into me. \
             Its purpose was: {} Its notes as they were are in \
             merged/{absorb}/NOTES.md and mine in \
             merged/{absorb}/NOTES.{keep}.md. Files of its that \
             differed from mine of the same name are under \
             merged/{absorb}/.\n",
            body.trim_end(),
            when(log::now_millis()),
            gone.purpose
        );
        let (id, dir) = self.new_shelf(absorb)?;
        std::fs::write(dir.join("NOTES.before.md"), &mine)?;
        std::fs::write(dir.join("NOTES.md"), text)?;
        self.append(json!({
            "v": 1,
            "type": "agents_merged",
            "ts": log::now_millis(),
            "kept": keep,
            "gone": absorb,
            "purpose": purpose.trim(),
            "reason": reason.trim(),
            "by": by.name(),
            "archive": id,
            "notes": if joined { "joined" } else if both { "rewritten" } else { "plain" },
        }))?;
        self.mend();
        if !self.census().archive.iter().any(|a| a.id == id) {
            bail!(
                "`{keep}` or `{absorb}` left the hamlet while this \
                 was decided; nothing was merged"
            );
        }
        let manifest =
            std::fs::read_to_string(dir.join("manifest.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .unwrap_or_default();
        Ok(Merged {
            kept: Resident {
                purpose: purpose.trim().to_string(),
                ..kept
            },
            gone,
            archive: id,
            joined,
            clashes: manifest["clashes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(String::from))
                .collect(),
        })
    }

    /// A chief deletes what is archived of a removed or merged agent
    /// for good. Gives the bytes it freed.
    pub fn erase(&self, which: &str, by: Who) -> Result<u64> {
        let Who::Chief(chief) = by else {
            bail!("only a chief can delete an agent for good");
        };
        self.mend();
        let census = self.census();
        let entry = census
            .archive
            .iter()
            .rfind(|a| {
                a.back.is_none()
                    && (a.id == which || a.resident.name == which)
            })
            .ok_or_else(|| {
                anyhow!(
                    "nothing is archived under `{which}`. Only an \
                     agent that was removed or merged can be \
                     deleted for good; /remove it first"
                )
            })?;
        let dir = self.shelf(&entry.id);
        let freed = size_of(&dir);
        // The record first: a folder left behind by a crash is only
        // wasted room, and one deleted with the record still naming
        // it would be an agent that cannot be restored.
        self.append(json!({
            "v": 1,
            "type": "agent_erased",
            "ts": log::now_millis(),
            "archive": entry.id,
            "name": entry.resident.name,
            "by": chief,
        }))?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(freed)
    }

    /// Brings back a removed agent, or splits a merged one out of the
    /// agent that absorbed it (H20). `which` is its name or its
    /// archive id, and `new_name` is for a name taken since. Only a
    /// chief may.
    pub fn restore(
        &self,
        which: &str,
        new_name: Option<&str>,
        reason: &str,
        by: Who,
    ) -> Result<Restored> {
        let Who::Chief(_) = by else {
            bail!("only a chief can restore an agent");
        };
        self.mend();
        let census = self.census();
        let at = census
            .archive
            .iter()
            .rposition(|a| {
                a.back.is_none()
                    && (a.id == which || a.resident.name == which)
            })
            .ok_or_else(|| {
                anyhow!(
                    "nothing is archived under `{which}`; /record \
                     lists what is"
                )
            })?;
        let entry = &census.archive[at];
        let was = &entry.resident.name;
        let name = new_name.unwrap_or(was);
        check_name(name)?;
        if Realm::new(self.home().to_path_buf())
            .hamlet_of(name)
            .is_some()
        {
            bail!(
                "an agent named `{name}` lives in the hamlet now. \
                 Restore this one under another name: /restore \
                 {which} as <new-name>"
            );
        }
        let limits = self.limits()?;
        let (most, most_roads) =
            (limits.most_agents, limits.most_roads);
        if census.residents.len() >= most {
            bail!(
                "the hamlet is full ({most} agents); remove one \
                 first, or raise `most_agents` in limits.toml"
            );
        }
        let mut split_from = None;
        if let Some(merge) = &entry.merge {
            let Some(kept) = census
                .residents
                .iter()
                .find(|r| r.id == merge.kept.id)
            else {
                bail!(
                    "`{}`, which `{was}` was merged into, has left \
                     the hamlet itself; restore that first",
                    merge.kept.name
                );
            };
            // Merges into one agent come apart latest first, or
            // its purpose and roads would not add up.
            if let Some(later) =
                census.archive[at + 1..].iter().find(|a| {
                    a.back.is_none()
                        && a.merge
                            .as_ref()
                            .is_some_and(|m| m.kept.id == kept.id)
                })
            {
                bail!(
                    "`{}` was merged into `{}` after `{was}`; split \
                     that one first",
                    later.resident.name,
                    kept.name
                );
            }
            split_from = Some(kept.name.clone());
        }
        self.set_aside(&census, name)?;
        let reason = match reason.trim() {
            "" => "a chief asked",
            given => given,
        };
        self.append(json!({
            "v": 1,
            "type": "agent_restored",
            "ts": log::now_millis(),
            "archive": entry.id,
            "name": name,
            "reason": reason,
            "by": by.name(),
        }))?;
        self.mend();
        let after = self.census();
        let resident = after
            .residents
            .iter()
            .find(|r| r.name == name)
            .cloned()
            .ok_or_else(|| anyhow!("the record was not written"))?;
        // Roads that come back were paid for when they were built,
        // so they cost nothing now. They count against the chief's
        // limit like any other, which a restore may pass.
        let built =
            after.roads.iter().filter(|r| !r.has(MAYOR)).count();
        Ok(Restored {
            roads: after
                .roads
                .iter()
                .filter(|r| r.has(name))
                .map(|r| r.other(name).to_string())
                .collect(),
            resident,
            split_from,
            over_roads: Some((built, most_roads))
                .filter(|(built, most)| built > most),
        })
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;
    use crate::files::files_under;

    const CHIEF: Who = Who::Chief("alice");

    fn hamlet(tag: &str) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("lifecycle-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        hamlet
            .create("librarian", "Keeps the list.", "test")
            .unwrap();
        hamlet.create("scout", "Looks things up.", "test").unwrap();
        (dir, hamlet)
    }

    /// Writes the chief's limits.
    fn limits(hamlet: &Hamlet, text: &str) {
        std::fs::write(
            hamlet_home::limits::path(hamlet.root()),
            text,
        )
        .unwrap();
    }

    fn write(hamlet: &Hamlet, name: &str, rel: &str, text: &str) {
        let path =
            hamlet.house_root(name).join("workspace").join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(
        hamlet: &Hamlet,
        name: &str,
        rel: &str,
    ) -> Option<String> {
        std::fs::read_to_string(
            hamlet.house_root(name).join("workspace").join(rel),
        )
        .ok()
    }

    fn skill(name: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: x\n---\n{body}\n")
    }

    fn peers(hamlet: &Hamlet, name: &str) -> Vec<(String, u32)> {
        let mut peers = hamlet
            .roads()
            .iter()
            .filter(|r| r.has(name))
            .map(|r| (r.other(name).to_string(), r.messages))
            .collect::<Vec<_>>();
        peers.sort();
        peers
    }

    fn road(to: &str, messages: u32) -> (String, u32) {
        (to.to_string(), messages)
    }

    #[test]
    fn an_erased_agent_leaves_nothing_and_cannot_come_back() {
        let (_dir, hamlet) = hamlet("erase");
        write(&hamlet, "scout", "found.txt", "Dune");
        // One that lives is not deleted for good.
        assert!(hamlet.erase("scout", Who::Chief("alice")).is_err());
        let gone = hamlet
            .remove("scout", "Nobody asks it.", Who::Mayor)
            .unwrap();
        let shelf = hamlet.shelf(&gone.id);
        assert!(shelf.exists());
        // Only a chief may.
        assert!(hamlet.erase("scout", Who::Mayor).is_err());

        let done =
            hamlet.decree("alice", "/erase scout").unwrap().unwrap();
        assert!(done.lines[0].contains("for good"));
        assert!(!shelf.exists());
        assert!(hamlet.census().archive.is_empty());
        assert!(
            hamlet
                .restore("scout", None, "", Who::Chief("alice"))
                .is_err()
        );
        assert!(hamlet.history().iter().any(|l| {
            l.contains("alice deleted `scout` for good")
        }));
        // The name is free for a new agent.
        hamlet.create("scout", "Scouts again.", "test").unwrap();
    }

    #[test]
    fn a_removed_agent_is_archived_with_its_roads_and_its_reason() {
        let (_dir, hamlet) = hamlet("remove");
        write(&hamlet, "scout", "found.txt", "Dune");
        hamlet.build_road("librarian", "scout", "test", "").unwrap();
        let gone = hamlet
            .remove("scout", "Nobody asks it.", Who::Mayor)
            .unwrap();

        assert!(!hamlet.house_root("scout").exists());
        let kept =
            hamlet.shelf(&gone.id).join("house/workspace/found.txt");
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "Dune");
        assert_eq!(hamlet.residents().len(), 1);
        assert!(hamlet.roads().iter().all(|r| !r.has("scout")));
        assert!(hamlet.house("scout").is_err());

        let told = hamlet.whereabouts("scout").unwrap();
        assert!(told.contains("removed"));
        assert!(told.contains("Nobody asks it."));
        assert!(hamlet.history().iter().any(|l| {
            l.contains("the mayor removed `scout`: Nobody asks it.")
        }));
        let librarian = hamlet.residents().remove(0);
        assert!(
            hamlet
                .identity(&librarian, "")
                .contains("- scout: removed")
        );
        assert!(hamlet.prompt("").contains("- scout: removed"));
    }

    #[test]
    fn what_may_not_be_removed_or_merged_is_refused() {
        let (_dir, hamlet) = hamlet("refuse");
        let no = |r: Result<Archived>| r.unwrap_err().to_string();
        assert!(
            no(hamlet.remove(MAYOR, "x", Who::Mayor))
                .contains("mayor")
        );
        assert!(
            no(hamlet.remove("nobody", "x", Who::Mayor))
                .contains("no agent")
        );
        assert!(
            no(hamlet.remove("scout", " ", Who::Mayor))
                .contains("reason")
        );
        let merge = |keep: &str, absorb: &str| {
            hamlet
                .merge(keep, absorb, "Both.", "x", Who::Mayor, None)
                .unwrap_err()
                .to_string()
        };
        assert!(merge("scout", "scout").contains("itself"));
        assert!(merge(MAYOR, "scout").contains("mayor"));
        assert!(merge("scout", MAYOR).contains("mayor"));
        assert!(merge("scout", "nobody").contains("no agent"));
        assert!(
            hamlet
                .merge(
                    "scout",
                    "librarian",
                    " ",
                    "x",
                    Who::Mayor,
                    None
                )
                .is_err()
        );
        assert_eq!(hamlet.residents().len(), 2);
        assert!(!hamlet.root().join("archive").exists());

        hamlet.remove("scout", "x", Who::Mayor).unwrap();
        assert!(
            no(hamlet.remove("scout", "x", Who::Mayor))
                .contains("was removed")
        );
    }

    #[test]
    fn a_chiefs_task_protects_an_agent_from_the_mayor() {
        let (_dir, hamlet) = hamlet("task");
        let task = hamlet
            .instructed("scout", "alice", "Find Dune.")
            .unwrap();
        let refused = hamlet
            .remove("scout", "Not needed.", Who::Mayor)
            .unwrap_err()
            .to_string();
        assert!(refused.contains("Find Dune."));
        assert!(refused.contains("alice"));
        for (keep, absorb) in
            [("scout", "librarian"), ("librarian", "scout")]
        {
            assert!(
                hamlet
                    .merge(
                        keep,
                        absorb,
                        "Both.",
                        "x",
                        Who::Mayor,
                        None
                    )
                    .is_err()
            );
        }
        // Ended, even by a turn that failed, it is held a while for
        // the chief to read, if the chief asks for that.
        limits(&hamlet, "[leaving]\nprotected_minutes = 10\n");
        hamlet.answered("scout", task, false).unwrap();
        assert!(
            hamlet
                .remove("scout", "x", Who::Mayor)
                .unwrap_err()
                .to_string()
                .contains("only just ended")
        );
        limits(&hamlet, "[leaving]\nprotected_minutes = 0\n");
        hamlet.remove("scout", "x", Who::Mayor).unwrap();

        // A chief is not held by a task.
        hamlet.instructed("librarian", "alice", "Sort it.").unwrap();
        hamlet.remove("librarian", "x", CHIEF).unwrap();
    }

    #[test]
    fn a_merge_makes_one_house_of_two() {
        let (_dir, hamlet) = hamlet("merge");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        hamlet.build_road("scout", "poet", "test", "").unwrap();
        for _ in 0..3 {
            hamlet.carried(MAYOR, "scout", "").unwrap();
        }
        hamlet.carried(MAYOR, "librarian", "").unwrap();
        hamlet.carried("poet", "scout", "").unwrap();
        let mine = "# Notes\nAlice likes tea.\n";
        let theirs = "# Notes\nThe wiki is slow.\n";
        write(&hamlet, "librarian", "NOTES.md", mine);
        write(&hamlet, "scout", "NOTES.md", theirs);
        write(
            &hamlet,
            "scout",
            "skills/lookup/SKILL.md",
            &skill("lookup", "Run skills/lookup/run.sh"),
        );
        write(
            &hamlet,
            "scout",
            "skills/sort/SKILL.md",
            &skill("sort", "scout's way: skills/sort/run.sh"),
        );
        write(
            &hamlet,
            "librarian",
            "skills/sort/SKILL.md",
            &skill("sort", "librarian's way"),
        );
        write(&hamlet, "scout", "found.txt", "Dune");
        write(&hamlet, "scout", "list.txt", "scout's");
        write(&hamlet, "librarian", "list.txt", "librarian's");
        for name in ["librarian", "scout"] {
            hamlet.house(name).unwrap().set_character(name).unwrap();
        }

        let merged = hamlet
            .merge(
                "librarian",
                "scout",
                "Keeps the list and looks things up.",
                "They overlap.",
                Who::Mayor,
                None,
            )
            .unwrap();
        assert!(merged.joined);
        assert_eq!(merged.clashes.len(), 2, "{:?}", merged.clashes);
        // Each character stays with its house.
        let character = |house: std::path::PathBuf| {
            std::fs::read_to_string(house.join("CHARACTER.md"))
                .unwrap()
        };
        assert_eq!(
            character(hamlet.house_root("librarian")),
            "librarian\n"
        );
        assert_eq!(
            character(hamlet.shelf(&merged.archive).join("house")),
            "scout\n"
        );

        let residents = hamlet.residents();
        assert_eq!(residents.len(), 2);
        assert_eq!(
            residents[0].purpose,
            "Keeps the list and looks things up."
        );
        assert!(!hamlet.house_root("scout").exists());
        assert!(
            hamlet
                .shelf(&merged.archive)
                .join("house/workspace/found.txt")
                .exists()
        );

        let got = |rel: &str| read(&hamlet, "librarian", rel);
        let notes = got("NOTES.md").unwrap();
        assert!(notes.contains("Alice likes tea."));
        assert!(notes.contains("## From scout\nThe wiki is slow."));
        assert!(notes.contains("merged/scout/NOTES.md"));
        assert_eq!(got("merged/scout/NOTES.md").unwrap(), theirs);
        assert_eq!(
            got("merged/scout/NOTES.librarian.md").unwrap(),
            mine
        );
        assert!(got("skills/lookup/SKILL.md").is_some());
        assert!(
            got("skills/sort/SKILL.md")
                .unwrap()
                .contains("librarian's way")
        );
        let theirs = got("skills/sort-from-scout/SKILL.md").unwrap();
        assert!(theirs.contains("name: sort-from-scout"));
        assert!(
            theirs.contains(
                "scout's way: skills/sort-from-scout/run.sh"
            )
        );
        assert_eq!(got("found.txt").unwrap(), "Dune");
        assert_eq!(got("list.txt").unwrap(), "librarian's");
        assert_eq!(got("merged/scout/list.txt").unwrap(), "scout's");
        // A starter skill both had is there once.
        assert!(
            got("skills/web-search-from-scout/SKILL.md").is_none()
        );
        let index = hamlet.house("librarian").unwrap().skill_index();
        assert!(index.iter().any(|s| s.name == "sort-from-scout"));

        // The roads of both, with the use of both.
        assert_eq!(
            peers(&hamlet, "librarian"),
            [road(MAYOR, 4), road("poet", 1)]
        );
        assert!(
            hamlet
                .whereabouts("scout")
                .unwrap()
                .contains("talk to `librarian`")
        );
        let librarian = hamlet.residents().remove(0);
        assert!(
            hamlet
                .identity(&librarian, "")
                .contains("scout: merged into you")
        );
        assert!(hamlet.history().iter().any(|l| {
            l.contains("merged `scout` into `librarian`")
                && l.contains("They overlap.")
        }));
    }

    #[test]
    fn rewritten_notes_are_used_only_when_they_can_be() {
        let (_dir, hamlet) = hamlet("notes");
        // Nothing to rewrite while one of the two has no notes.
        assert!(
            hamlet
                .merge_request("librarian", "scout", "Both.")
                .is_none()
        );
        write(
            &hamlet,
            "librarian",
            "NOTES.md",
            "# Notes\nAlice likes tea.\n",
        );
        write(
            &hamlet,
            "scout",
            "NOTES.md",
            "# Notes\nThe wiki is slow.\n",
        );
        let request = hamlet
            .merge_request("librarian", "scout", "Both.")
            .unwrap();
        let asked = request[1]["content"].as_str().unwrap();
        assert!(asked.contains("Alice likes tea."));
        assert!(asked.contains("The wiki is slow."));

        assert_eq!(
            accept("```markdown\n# Notes\nA.\n```", 100).unwrap(),
            "# Notes\nA."
        );
        assert_eq!(
            accept("<think>hm</think>\nHere:\n# Notes\nA.", 100)
                .unwrap(),
            "# Notes\nA."
        );
        assert!(accept("# Notes\n", 100).is_none());
        assert!(accept(&"x".repeat(101), 100).is_none());

        let merged = hamlet
            .merge(
                "librarian",
                "scout",
                "Both.",
                "x",
                Who::Mayor,
                Some("# Notes\nAlice likes tea; the wiki is slow.\n"),
            )
            .unwrap();
        assert!(!merged.joined);
        let notes = read(&hamlet, "librarian", "NOTES.md").unwrap();
        assert!(notes.starts_with(
            "# Notes\nAlice likes tea; the wiki is slow.\n\n## \
             Merged\n"
        ));
    }

    #[test]
    fn a_split_undoes_a_merge_the_agent_has_not_built_on() {
        let (_dir, hamlet) = hamlet("split");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        hamlet.build_road("scout", "poet", "test", "").unwrap();
        hamlet.build_road("scout", "librarian", "test", "").unwrap();
        hamlet.carried("poet", "scout", "").unwrap();
        let mine = "# Notes\nAlice likes tea.\n";
        let theirs = "# Notes\nThe wiki is slow.\n";
        write(&hamlet, "librarian", "NOTES.md", mine);
        write(&hamlet, "scout", "NOTES.md", theirs);
        write(
            &hamlet,
            "scout",
            "skills/lookup/SKILL.md",
            &skill("lookup", "x"),
        );
        write(&hamlet, "scout", "found.txt", "Dune");
        let before = files_under(&hamlet.house_root("librarian"));
        hamlet
            .merge(
                "librarian",
                "scout",
                "Both.",
                "x",
                Who::Mayor,
                None,
            )
            .unwrap();

        // The mayor may not.
        assert!(
            hamlet.restore("scout", None, "", Who::Mayor).is_err()
        );
        let back = hamlet
            .restore("scout", None, "It was a mistake.", CHIEF)
            .unwrap();
        assert_eq!(back.split_from.as_deref(), Some("librarian"));
        assert_eq!(
            files_under(&hamlet.house_root("librarian")),
            before
        );
        assert_eq!(
            read(&hamlet, "librarian", "NOTES.md").unwrap(),
            mine
        );
        assert_eq!(
            read(&hamlet, "scout", "NOTES.md").unwrap(),
            theirs
        );
        assert_eq!(
            read(&hamlet, "scout", "found.txt").unwrap(),
            "Dune"
        );
        let residents = hamlet.residents();
        assert_eq!(residents[0].purpose, "Keeps the list.");
        assert_eq!(residents[2].purpose, "Looks things up.");
        // The road the librarian only had through the scout is the
        // scout's again.
        assert_eq!(
            peers(&hamlet, "librarian"),
            [road(MAYOR, 0), road("scout", 0)]
        );
        assert_eq!(
            peers(&hamlet, "scout"),
            [road("librarian", 0), road(MAYOR, 0), road("poet", 1)]
        );
        assert!(hamlet.whereabouts("scout").is_none());
        assert!(
            hamlet
                .history()
                .last()
                .unwrap()
                .contains("the chief alice restored `scout`")
        );
    }

    #[test]
    fn a_split_keeps_what_the_merged_agent_learned_since() {
        let (_dir, hamlet) = hamlet("learned");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        hamlet.build_road("scout", "poet", "test", "").unwrap();
        write(
            &hamlet,
            "librarian",
            "NOTES.md",
            "# Notes\nAlice likes tea.\n",
        );
        write(
            &hamlet,
            "scout",
            "NOTES.md",
            "# Notes\nThe wiki is slow.\n",
        );
        write(&hamlet, "scout", "found.txt", "Dune");
        write(&hamlet, "scout", "old.txt", "old");
        hamlet
            .merge(
                "librarian",
                "scout",
                "Both.",
                "x",
                Who::Mayor,
                None,
            )
            .unwrap();
        // The merged agent works on: new notes, a changed file, a
        // road it now uses.
        let notes = read(&hamlet, "librarian", "NOTES.md").unwrap();
        write(
            &hamlet,
            "librarian",
            "NOTES.md",
            &format!("{notes}Bob likes coffee.\n"),
        );
        write(&hamlet, "librarian", "found.txt", "Dune, Emma");
        hamlet.carried("librarian", "poet", "").unwrap();

        hamlet.restore("scout", None, "", CHIEF).unwrap();
        let notes = read(&hamlet, "librarian", "NOTES.md").unwrap();
        assert!(notes.contains("Bob likes coffee."));
        assert!(notes.contains("The wiki is slow."));
        assert!(notes.contains("## Split"));
        assert!(notes.contains("merged/scout/NOTES.librarian.md"));
        let got = |rel: &str| read(&hamlet, "librarian", rel);
        assert!(got("merged/scout/NOTES.librarian.md").is_some());
        assert_eq!(got("found.txt").unwrap(), "Dune, Emma");
        assert!(got("old.txt").is_none());
        // The scout has what it had at the merge.
        assert_eq!(
            read(&hamlet, "scout", "found.txt").unwrap(),
            "Dune"
        );
        // Both keep the road to the poet: one used it since, the
        // other had it before.
        assert_eq!(
            peers(&hamlet, "librarian"),
            [road(MAYOR, 0), road("poet", 1)]
        );
        assert!(peers(&hamlet, "scout").contains(&road("poet", 0)));
    }

    #[test]
    fn merges_into_one_agent_come_apart_latest_first() {
        let (_dir, hamlet) = hamlet("order");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        let merge = |absorb: &str, purpose: &str| {
            hamlet
                .merge(
                    "librarian",
                    absorb,
                    purpose,
                    "x",
                    Who::Mayor,
                    None,
                )
                .unwrap()
        };
        merge("scout", "Two.");
        merge("poet", "Three.");
        let refused =
            hamlet.restore("scout", None, "", CHIEF).unwrap_err();
        assert!(refused.to_string().contains("split that one first"));
        hamlet.restore("poet", None, "", CHIEF).unwrap();
        assert_eq!(hamlet.residents()[0].purpose, "Two.");
        hamlet.restore("scout", None, "", CHIEF).unwrap();
        assert_eq!(hamlet.residents()[0].purpose, "Keeps the list.");

        // An agent whose host has left cannot be split from it.
        merge("scout", "Two.");
        hamlet.remove("librarian", "x", Who::Mayor).unwrap();
        let refused =
            hamlet.restore("scout", None, "", CHIEF).unwrap_err();
        assert!(refused.to_string().contains("restore that first"));
    }

    #[test]
    fn a_restored_agent_gets_back_the_roads_to_those_it_knew() {
        let (_dir, hamlet) = hamlet("restore");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        hamlet.build_road("scout", "poet", "test", "").unwrap();
        hamlet.build_road("scout", "librarian", "test", "").unwrap();
        hamlet.carried("scout", "librarian", "").unwrap();
        write(&hamlet, "scout", "found.txt", "Dune");
        let id = hamlet.residents()[1].id;
        hamlet.remove("scout", "x", Who::Mayor).unwrap();
        // The poet is replaced by another of its name meanwhile.
        hamlet.remove("poet", "x", Who::Mayor).unwrap();
        hamlet.create("poet", "Writes prose.", "test").unwrap();

        let back = hamlet.restore("scout", None, "", CHIEF).unwrap();
        assert_eq!(back.resident.id, id);
        assert_eq!(back.split_from, None);
        assert_eq!(
            read(&hamlet, "scout", "found.txt").unwrap(),
            "Dune"
        );
        assert_eq!(
            peers(&hamlet, "scout"),
            [road("librarian", 1), road(MAYOR, 0)]
        );
        assert!(hamlet.restore("scout", None, "", CHIEF).is_err());
    }

    #[test]
    fn a_name_taken_since_is_not_taken_back() {
        let (_dir, hamlet) = hamlet("reuse");
        write(&hamlet, "scout", "found.txt", "Dune");
        let old = hamlet.residents()[1].id;
        hamlet.remove("scout", "x", Who::Mayor).unwrap();
        let new =
            hamlet.create("scout", "Scouts again.", "test").unwrap();
        assert_ne!(new.id, old);
        // The newcomer knows nothing of the old one.
        assert!(read(&hamlet, "scout", "found.txt").is_none());
        assert!(hamlet.whereabouts("scout").is_none());

        let refused =
            hamlet.restore("scout", None, "", CHIEF).unwrap_err();
        assert!(refused.to_string().contains("as <new-name>"));
        let back = hamlet
            .restore("scout", Some("old-scout"), "", CHIEF)
            .unwrap();
        assert_eq!(
            (back.resident.id, back.resident.name.as_str()),
            (old, "old-scout")
        );
        assert_eq!(
            read(&hamlet, "old-scout", "found.txt").unwrap(),
            "Dune"
        );
        assert!(read(&hamlet, "scout", "found.txt").is_none());
    }

    #[test]
    fn a_crash_after_the_record_is_written_is_mended() {
        let (_dir, hamlet) = hamlet("crash");
        let mine = "# Notes\nAlice likes tea.\n";
        write(&hamlet, "librarian", "NOTES.md", mine);
        write(&hamlet, "scout", "found.txt", "Dune");
        // A merge that got as far as its line in the record.
        let dir = hamlet.shelf("scout-1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("NOTES.before.md"), mine).unwrap();
        std::fs::write(dir.join("NOTES.md"), "# Notes\nMerged.\n")
            .unwrap();
        hamlet
            .append(json!({"v": 1, "type": "agents_merged", "ts": 1, "kept": "librarian",
                "gone": "scout", "purpose": "Both.", "reason": "x", "by": MAYOR, "archive": "scout-1"}))
            .unwrap();
        assert!(hamlet.house_root("scout").exists());
        hamlet.repair();
        assert!(!hamlet.house_root("scout").exists());
        let got =
            |rel: &str| read(&hamlet, "librarian", rel).unwrap();
        assert_eq!(got("found.txt"), "Dune");
        assert_eq!(got("NOTES.md"), "# Notes\nMerged.\n");
        // Mending again changes nothing.
        write(&hamlet, "librarian", "NOTES.md", "# Notes\nLater.\n");
        hamlet.repair();
        assert_eq!(got("NOTES.md"), "# Notes\nLater.\n");

        // A restore that got as far as its line.
        hamlet
            .append(json!({"v": 1, "type": "agent_restored", "ts": 2, "archive": "scout-1",
                "name": "scout", "reason": "x", "by": "alice"}))
            .unwrap();
        hamlet.repair();
        assert_eq!(
            read(&hamlet, "scout", "found.txt").unwrap(),
            "Dune"
        );

        // A removal that got as far as its line. Creating an agent
        // of the same name puts the old house away first.
        hamlet
            .append(json!({"v": 1, "type": "agent_removed", "ts": 3, "name": "scout",
                "reason": "x", "by": MAYOR, "archive": "scout-3"}))
            .unwrap();
        hamlet.create("scout", "Scouts again.", "test").unwrap();
        assert!(read(&hamlet, "scout", "found.txt").is_none());
        assert!(
            hamlet
                .shelf("scout-3")
                .join("house/workspace/found.txt")
                .exists()
        );
    }

    #[test]
    fn the_chiefs_limits_say_what_the_mayor_may_do_and_how_many_fit()
    {
        let (_dir, hamlet) = hamlet("reserved");
        limits(
            &hamlet,
            "[mayor]\nremove_agents = false\nmerge_agents = false\n",
        );
        let refused = hamlet
            .remove("scout", "x", Who::Mayor)
            .unwrap_err()
            .to_string();
        assert!(
            refused
                .contains("the chief has reserved removing an agent"),
            "{refused}"
        );
        let refused = hamlet
            .merge(
                "librarian",
                "scout",
                "Both.",
                "x",
                Who::Mayor,
                None,
            )
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("the chief has reserved merging agents"),
            "{refused}"
        );
        assert_eq!(hamlet.residents().len(), 2);

        // The chief may do the things reserved to the chief.
        hamlet.remove("scout", "x", CHIEF).unwrap();
        // A restore counts against the chief's own limit.
        limits(&hamlet, "most_agents = 1\n");
        let refused =
            hamlet.restore("scout", None, "", CHIEF).unwrap_err();
        assert!(refused.to_string().contains("full (1 agents)"));
        limits(&hamlet, "most_agents = 2\n");
        hamlet.restore("scout", None, "", CHIEF).unwrap();

        // Limits that cannot be read stop the mayor, not the chief.
        limits(&hamlet, "most_agent = 2\n");
        assert!(hamlet.remove("scout", "x", Who::Mayor).is_err());
        hamlet.remove("scout", "x", CHIEF).unwrap();
    }

    #[test]
    fn a_model_belongs_to_an_agent_and_not_to_its_name() {
        let (_dir, hamlet) = hamlet("models");
        limits(&hamlet, "models = [\"small\"]\n");
        std::fs::write(
            hamlet_model::catalogue::path(hamlet.home()),
            "[[model]]\nid = \"small\"\nallowed = true\n",
        )
        .unwrap();
        hamlet.choose_model("scout", "small", "x").unwrap();

        // A newcomer of a removed agent's name does not get its
        // model.
        hamlet.remove("scout", "x", Who::Mayor).unwrap();
        hamlet.create("scout", "Scouts again.", "test").unwrap();
        assert_eq!(hamlet.model_of("scout"), None);

        // Restored, it has its model back.
        hamlet.restore("scout", Some("tracker"), "", CHIEF).unwrap();
        assert_eq!(
            hamlet.model_of("tracker").as_deref(),
            Some("small")
        );
    }

    #[test]
    fn roads_that_come_back_or_pass_on_are_not_paid_for_again() {
        let (_dir, hamlet) = hamlet("roadcost");
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        hamlet.build_road("scout", "poet", "test", "").unwrap();
        hamlet.build_road("scout", "librarian", "test", "").unwrap();
        let paid = || {
            crate::budget::Ledger::new(hamlet.root())
                .since(chrono::NaiveDate::MIN)
                .into_iter()
                .filter(|(_, line)| line["what"] == "road")
                .count()
        };
        let built = || {
            hamlet.roads().iter().filter(|r| !r.has(MAYOR)).count()
        };
        assert_eq!((paid(), built()), (2, 2));

        // A merge passes roads on and never adds to their number.
        hamlet
            .merge(
                "librarian",
                "scout",
                "Both.",
                "x",
                Who::Mayor,
                None,
            )
            .unwrap();
        assert_eq!((paid(), built()), (2, 1));
        hamlet.restore("scout", None, "", CHIEF).unwrap();
        assert_eq!((paid(), built()), (2, 2));

        // A restore brings every road back, past the chief's limit
        // if need be, and says so. The mayor can then build no more.
        hamlet.remove("scout", "x", Who::Mayor).unwrap();
        assert_eq!(built(), 0);
        limits(&hamlet, "most_roads = 1\n");
        let back = hamlet.restore("scout", None, "", CHIEF).unwrap();
        assert_eq!(back.over_roads, Some((2, 1)));
        assert!(
            back.report().contains("more than the 1 your limits")
        );
        assert_eq!((paid(), built()), (2, 2));
        assert!(
            hamlet.build_road("librarian", "poet", "x", "").is_err()
        );
    }

    #[test]
    fn sessions_deciding_at_once_lose_nothing() {
        let (_dir, hamlet) = hamlet("atonce");
        write(&hamlet, "scout", "found.txt", "Dune");
        let removed = std::thread::scope(|scope| {
            let tries = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        hamlet
                            .remove("scout", "x", Who::Mayor)
                            .is_ok()
                    })
                })
                .collect::<Vec<_>>();
            tries
                .into_iter()
                .filter_map(|t| t.join().ok())
                .filter(|ok| *ok)
                .count()
        });
        // The first line in the record wins and the rest do nothing.
        assert_eq!(removed, 1);
        let archived = hamlet.archived();
        assert_eq!(archived.len(), 1);
        assert_eq!(
            std::fs::read_to_string(
                hamlet
                    .shelf(&archived[0].id)
                    .join("house/workspace/found.txt")
            )
            .unwrap(),
            "Dune"
        );

        // A merge written after the agent it merges into has left
        // did not happen: the other agent stays as it was.
        hamlet.create("poet", "Writes verse.", "test").unwrap();
        hamlet.remove("librarian", "x", Who::Mayor).unwrap();
        hamlet
            .append(json!({"v": 1, "type": "agents_merged", "ts": 9, "kept": "librarian",
                "gone": "poet", "purpose": "Both.", "reason": "x", "by": MAYOR, "archive": "poet-9"}))
            .unwrap();
        hamlet.repair();
        assert_eq!(hamlet.residents()[0].name, "poet");
        assert_eq!(hamlet.residents()[0].purpose, "Writes verse.");
        assert!(hamlet.house_root("poet").exists());
    }
}
