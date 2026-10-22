//! Removing a hamlet, bringing it back and deleting it for good (H19,
//! H20).

use std::path::PathBuf;

use anyhow::{Result, bail};
use hamlet_model::log;
use serde_json::json;

use crate::hamlet::{HOME, Hamlet};
use crate::realm::Realm;
use crate::record::text;

/// The ones who have asked for a hamlet's removal (H19).
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    /// Its own mayor decided it.
    pub own: bool,
    /// The other hamlets whose mayors asked.
    pub others: Vec<String>,
    /// Enough have asked for the chief to be asked to confirm.
    pub ready: bool,
}

impl Realm {
    /// The ones who have asked for the removal of `target` since it
    /// was last founded or restored.
    pub fn removal(&self, target: &str) -> Removal {
        let live = self.hamlets();
        let mut asked = Vec::<String>::new();
        for e in &self.events() {
            match e["type"].as_str() {
                Some("removal_asked")
                    if text(e, "hamlet") == target =>
                {
                    let by = text(e, "by");
                    if !asked.contains(&by) {
                        asked.push(by);
                    }
                }
                Some("hamlet_founded" | "hamlet_restored")
                    if text(e, "name") == target =>
                {
                    asked.clear()
                }
                _ => {}
            }
        }
        // A removed hamlet's mayor no longer counts.
        asked.retain(|by| live.iter().any(|p| &p.name == by));
        let own = asked.iter().any(|by| by == target);
        let others = asked
            .into_iter()
            .filter(|by| by != target)
            .collect::<Vec<_>>();
        Removal {
            own,
            ready: own || others.len() >= self.rules.removal_mayors,
            others,
        }
    }

    /// The mayor of `by` asks for `target` to be removed: its own
    /// hamlet by its own decision, or another's along with other
    /// mayors (H19). Nothing is removed until a chief confirms.
    pub fn ask_removal(
        &self,
        by: &str,
        target: &str,
        reason: &str,
    ) -> Result<Removal> {
        self.hamlet(by)?;
        self.hamlet(target)?;
        if reason.trim().is_empty() {
            bail!("say why in `reason`; the chief is shown it");
        }
        self.append(json!({
            "v": 1,
            "type": "removal_asked",
            "ts": log::now_millis(),
            "hamlet": target,
            "by": by,
            "reason": reason.trim(),
        }))?;
        Ok(self.removal(target))
    }

    /// A chief removes a hamlet, or confirms the removal mayors
    /// asked for; the chief's word alone is enough (H19). Every
    /// house is archived with the hamlet's record, and the folder
    /// they are archived in is returned.
    pub fn remove(
        &self,
        target: &str,
        chief: &str,
        reason: &str,
    ) -> Result<PathBuf> {
        let hamlet = self.hamlet(target)?;
        if !self.is_chief(target, chief) {
            bail!("only a chief of `{target}` can remove it");
        }
        if target == HOME {
            bail!(
                "the first hamlet's folder is the home itself, so \
                 it cannot be removed"
            );
        }
        let archive = PathBuf::from("archive")
            .join("hamlets")
            .join(format!("{target}.{}", log::now_millis()));
        let to = self.home.join(&archive);
        std::fs::create_dir_all(to.parent().expect("has a parent"))?;
        std::fs::rename(hamlet.root(), &to)?;
        self.append(json!({
            "v": 1,
            "type": "hamlet_removed",
            "ts": log::now_millis(),
            "name": target,
            "by": chief,
            "reason": reason.trim(),
            "archive": archive,
        }))?;
        Ok(to)
    }

    /// A chief brings back the hamlet `name` as it was when removed
    /// (H20).
    pub fn restore(&self, name: &str, chief: &str) -> Result<Hamlet> {
        let (live, gone) = self.places();
        let Some((place, archive)) =
            gone.iter().rfind(|(p, _)| p.name == name)
        else {
            bail!("no removed hamlet named `{name}`");
        };
        if !place.chiefs.is_empty()
            && !place.chiefs.iter().any(|c| c == chief)
        {
            bail!("only a chief of `{name}` can restore it");
        }
        if live.len() >= self.rules.most_hamlets {
            bail!("the chief's limits allow no more hamlets");
        }
        let hamlet = Hamlet::at(&self.home, name);
        if hamlet.root().exists() {
            bail!("a hamlet named `{name}` exists again");
        }
        // Names given away since are not taken back.
        let back = Hamlet::new(archive.clone());
        if let Some(taken) = back
            .members()
            .into_iter()
            .find(|r| self.hamlet_of(&r.name).is_some())
        {
            bail!(
                "an agent named `{}` exists now, so `{name}` cannot \
                 come back as it was",
                taken.name
            );
        }
        std::fs::create_dir_all(
            hamlet.root().parent().expect("has a parent"),
        )?;
        std::fs::rename(archive, hamlet.root())?;
        self.append(json!({
            "v": 1,
            "type": "hamlet_restored",
            "ts": log::now_millis(),
            "name": name,
            "by": chief,
        }))?;
        Ok(hamlet)
    }

    /// A chief deletes what is archived of the removed hamlet
    /// `name`, for good: every house, with its notes, skills and
    /// logs. It cannot be restored after.
    pub fn erase(&self, name: &str, chief: &str) -> Result<()> {
        let (_, gone) = self.places();
        let Some((place, archive)) =
            gone.iter().rfind(|(p, _)| p.name == name)
        else {
            bail!(
                "no removed hamlet named `{name}`. A hamlet is \
                 removed before it can be deleted"
            );
        };
        if !place.chiefs.is_empty()
            && !place.chiefs.iter().any(|c| c == chief)
        {
            bail!("only a chief of `{name}` can delete it");
        }
        // Only ever a folder of the archive.
        let kept = self.home.join("archive").join("hamlets");
        if !archive.starts_with(&kept) {
            bail!("`{name}` is not archived where hamlets are");
        }
        if archive.exists() {
            std::fs::remove_dir_all(archive)?;
        }
        self.append(json!({
            "v": 1,
            "type": "hamlet_erased",
            "ts": log::now_millis(),
            "name": name,
            "by": chief,
        }))
    }
}
