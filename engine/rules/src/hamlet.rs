//! The hamlet: its agents, their houses on disk and the holder of the
//! mayor's office, read from `hamlet.jsonl` in its folder. A house is
//! `agents/<name>/`; the first mayor's is the home's own workspace.

mod census;
mod prompt;
mod roads;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use hamlet_home::Home;
use hamlet_home::limits::Limits;
use hamlet_model::log;
use serde_json::{Value, json};

pub(crate) use crate::hamlet::census::by_whom;
pub use crate::hamlet::census::{Archived, Census, Merge, Task};
pub use crate::hamlet::roads::{Road, RoadUse, ends};
use crate::realm::Realm;
use crate::{lifecycle, record};

/// The address of the mayor's office in a message or a road. It
/// reaches whichever agent holds the office.
pub const MAYOR: &str = "mayor";

/// The name of the first hamlet, whose folder is the home.
pub const HOME: &str = "home";

/// The most letters a name of an agent or a hamlet has.
const NAME_LONGEST: usize = 24;

/// The name of a first mayor the record does not name.
pub const FOUNDER: &str = "founder";

/// An agent of a hamlet.
#[derive(Clone, Debug, PartialEq)]
pub struct Resident {
    pub name: String,
    pub purpose: String,
    /// Its number in the record, kept for life: a later agent of
    /// the same name has another.
    pub id: u64,
    /// The time it was created, in milliseconds. Nothing for the
    /// first mayor.
    pub since: u64,
}

/// Something the mayor does that the chief may reserve (H22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alone {
    CreateAgents,
    BuildRoads,
    RemoveAgents,
    MergeAgents,
}

/// A hamlet's folder and its record.
pub struct Hamlet {
    /// The home every hamlet lives under.
    home: PathBuf,
    name: String,
    root: PathBuf,
    pub lifecycle: lifecycle::Settings,
}

impl Hamlet {
    /// The first hamlet of the home at `root`.
    pub fn new(root: PathBuf) -> Self {
        Hamlet {
            home: root.clone(),
            name: HOME.to_string(),
            root,
            lifecycle: lifecycle::Settings::default(),
        }
    }

    /// The hamlet `name` of `home`.
    pub fn at(home: &Path, name: &str) -> Self {
        if name == HOME {
            return Hamlet::new(home.to_path_buf());
        }
        Hamlet {
            home: home.to_path_buf(),
            name: name.to_string(),
            root: home.join("hamlets").join(name),
            lifecycle: lifecycle::Settings::default(),
        }
    }

    /// Another handle on the same hamlet.
    pub fn again(&self) -> Self {
        Hamlet {
            home: self.home.clone(),
            name: self.name.clone(),
            root: self.root.clone(),
            lifecycle: self.lifecycle,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The home it lives under.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Its own folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn record(&self) -> PathBuf {
        self.root.join("hamlet.jsonl")
    }

    pub(crate) fn events(&self) -> Vec<Value> {
        record::read(&self.record())
    }

    pub(crate) fn append(&self, event: Value) -> Result<()> {
        record::append(&self.record(), &event)
    }

    /// The chief's limits for this hamlet, read from their file.
    pub fn limits(&self) -> Result<Limits> {
        Limits::load(self.root())
    }

    /// Refuses what the chief has not left to the mayor alone.
    pub fn mayor_may(&self, act: Alone) -> Result<()> {
        let mayor = self.limits()?.mayor;
        let (allowed, what) = match act {
            Alone::CreateAgents => {
                (mayor.create_agents, "creating an agent")
            }
            Alone::BuildRoads => {
                (mayor.build_roads, "building a road")
            }
            Alone::RemoveAgents => {
                (mayor.remove_agents, "removing an agent")
            }
            Alone::MergeAgents => {
                (mayor.merge_agents, "merging agents")
            }
        };
        if !allowed {
            bail!(
                "the chief has reserved {what}. Tell the chief what \
                 you want and why; the chief can allow it on the \
                 hamlet's Bylaws page, under what the mayor may do \
                 without asking"
            );
        }
        Ok(())
    }

    pub(crate) fn house_root(&self, name: &str) -> PathBuf {
        self.house_in(&self.census(), name)
    }

    /// `house_root` from a census already read.
    pub(crate) fn house_in(
        &self,
        census: &Census,
        name: &str,
    ) -> PathBuf {
        if census.at_root(name) {
            self.root.clone()
        } else {
            self.root.join("agents").join(name)
        }
    }

    /// The workspace of each agent the mayor leads, by its name.
    pub fn workspaces(&self) -> Vec<(String, PathBuf)> {
        let census = self.census();
        let led = census.residents.iter().map(|resident| {
            let house = self.house_in(&census, &resident.name);
            (resident.name.clone(), house.join("workspace"))
        });
        led.collect()
    }

    /// The folder of the house of `name`. `None` when it is the
    /// hamlet's own folder, which cannot be moved.
    pub fn house_dir(&self, name: &str) -> Option<PathBuf> {
        let root = self.house_root(name);
        (root != self.root).then_some(root)
    }

    /// The agent that holds the mayor's office.
    pub fn mayor(&self) -> Resident {
        self.census().mayor
    }

    /// True when the agent `name` holds the mayor's office.
    pub(crate) fn is_mayor(&self, name: &str) -> bool {
        self.census().mayor.name == name
    }

    /// Everyone who lives here, the mayor first.
    pub fn members(&self) -> Vec<Resident> {
        self.census().members()
    }

    /// The living agent `name`, the mayor included.
    pub fn member(&self, name: &str) -> Option<Resident> {
        self.members().into_iter().find(|r| r.name == name)
    }

    /// The agent an address reaches: the mayor for the office, the
    /// agent itself for a name.
    pub fn resolve(&self, address: &str) -> Option<Resident> {
        let census = self.census();
        if address == MAYOR {
            return Some(census.mayor);
        }
        census.members().into_iter().find(|r| r.name == address)
    }

    /// The address an agent sends from and is reached at: the office
    /// while it is mayor, its name otherwise.
    pub fn address(&self, name: &str) -> String {
        if self.is_mayor(name) {
            MAYOR.to_string()
        } else {
            name.to_string()
        }
    }

    /// The agent number `id` as it is now, and whether it holds the
    /// mayor's office. `Err` says how it left.
    pub fn seat(
        &self,
        id: u64,
        name: &str,
    ) -> Result<(Resident, bool)> {
        let mayor = self.census().mayor;
        if mayor.id == id {
            return Ok((mayor, true));
        }
        Ok((self.still_here(id, name)?, false))
    }

    /// Writes down who founded the hamlet, unless the record already
    /// says. With `house_at_root` the first mayor's house is the
    /// hamlet's own folder.
    pub fn found(
        &self,
        mayor: &str,
        purpose: &str,
        house_at_root: bool,
    ) -> Result<()> {
        if self.events().iter().any(|e| e["type"] == "founded") {
            return Ok(());
        }
        std::fs::create_dir_all(&self.root)?;
        self.append(json!({
            "v": 1,
            "type": "founded",
            "ts": log::now_millis(),
            "mayor": mayor,
            "purpose": purpose.trim(),
            "house": if house_at_root { "root" } else { "agents" },
        }))
    }

    /// Gives the mayor's office to `to`, an agent of this hamlet,
    /// and returns who held it. That agent stays as an ordinary one
    /// (H18). `by` says who decided: a chief or a vote.
    pub fn set_mayor(
        &self,
        to: &str,
        by: &str,
        reason: &str,
    ) -> Result<Resident> {
        let census = self.census();
        if census.mayor.name == to {
            bail!("`{to}` is already the mayor");
        }
        if !census.residents.iter().any(|r| r.name == to) {
            bail!("no agent named `{to}` in this hamlet");
        }
        self.append(json!({
            "v": 1,
            "type": "mayor_changed",
            "ts": log::now_millis(),
            "from": census.mayor.name,
            "to": to,
            "by": by,
            "reason": reason.trim(),
        }))?;
        Ok(census.mayor)
    }

    /// The agents living in the hamlet, the mayor aside.
    pub fn residents(&self) -> Vec<Resident> {
        self.census().residents
    }

    /// The house of whoever is mayor.
    pub fn hall(&self) -> Result<Home> {
        let census = self.census();
        Home::open(self.house_in(&census, &census.mayor.name), None)
    }

    /// Opens the house of `name`, creating its layout if missing.
    pub fn house(&self, name: &str) -> Result<Home> {
        let census = self.census();
        if !census.members().iter().any(|r| r.name == name) {
            bail!("no agent named `{name}`");
        }
        Home::open(self.house_in(&census, name), None)
    }

    /// True when it holds as many agents as the chief allows.
    pub(crate) fn is_full(&self) -> Result<bool> {
        Ok(self.residents().len() >= self.limits()?.most_agents)
    }

    /// Adds an agent and builds its house. The mayor's `reason` is
    /// kept in the record (H17).
    pub fn create(
        &self,
        name: &str,
        purpose: &str,
        reason: &str,
    ) -> Result<Resident> {
        check_name(name)?;
        if purpose.trim().is_empty() {
            bail!("say what the agent is for in `purpose`");
        }
        self.mend();
        // A name is an agent's for life, in whichever hamlet.
        let census = self.census();
        if census.members().iter().any(|r| r.name == name)
            || Realm::new(self.home.clone()).hamlet_of(name).is_some()
        {
            bail!("an agent named `{name}` already exists");
        }
        let most = self.limits()?.most_agents;
        if census.residents.len() >= most {
            bail!(
                "the hamlet is full ({most} agents). Only the chief \
                 can raise `most_agents` in limits.toml"
            );
        }
        // A new agent starts knowing nothing, whatever an earlier
        // one of its name left behind.
        self.set_aside(&census, name)?;
        Home::open(self.house_in(&census, name), None)?;
        self.append(json!({
            "v": 1,
            "type": "agent_created",
            "ts": log::now_millis(),
            "name": name,
            "purpose": purpose.trim(),
            "reason": reason.trim(),
        }))?;
        self.residents()
            .into_iter()
            .find(|r| r.name == name)
            .ok_or_else(|| {
                anyhow::anyhow!("the record was not written")
            })
    }

    /// Notes that a chief instructed `name` directly, so the mayor is
    /// told (H10). Returns the number of the task, which protects
    /// the agent until `answered` (H7).
    pub fn instructed(
        &self,
        name: &str,
        user: &str,
        text: &str,
    ) -> Result<u64> {
        let task = log::now_millis();
        self.append(json!({
            "v": 1,
            "type": "instructed",
            "ts": task,
            "task": task,
            "name": name,
            "user": user,
            "text": text,
        }))?;
        Ok(task)
    }

    /// Notes that the turn `name` ran on a chief's task ended, and
    /// whether it gave an answer.
    pub fn answered(
        &self,
        name: &str,
        task: u64,
        ok: bool,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "answered",
            "ts": log::now_millis(),
            "name": name,
            "task": task,
            "ok": ok,
        }))
    }
}

/// A name as a model wrote it, in the form a name must have:
/// `FelineBiologist` and `feline_biologist` are `feline-biologist`.
pub fn tidy_name(name: &str) -> String {
    let mut tidy = String::new();
    for c in name.trim().chars() {
        let upper = c.is_ascii_uppercase();
        if upper
            && tidy.ends_with(|last: char| last.is_ascii_lowercase())
        {
            tidy.push('-');
        }
        match c {
            ' ' | '_' | '-' if !tidy.ends_with('-') => tidy.push('-'),
            ' ' | '_' | '-' => {}
            _ => tidy.push(c.to_ascii_lowercase()),
        }
    }
    tidy.trim_matches('-').to_string()
}

/// Refuses a name an agent cannot have.
pub(crate) fn check_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= NAME_LONGEST
        && name.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    if !valid {
        bail!(
            "a name is 1 to 24 lowercase letters and hyphens, such \
             as `librarian`"
        );
    }
    if name == MAYOR {
        bail!("`mayor` is you");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn hamlet(tag: &str) -> (Scratch, Hamlet) {
        let dir = Scratch::new(&format!("hamlet-{tag}"));
        let hamlet = Hamlet::new(dir.to_path_buf());
        (dir, hamlet)
    }

    #[test]
    fn bad_and_repeated_names_are_refused() {
        let (_dir, hamlet) = hamlet("names");
        assert!(hamlet.create("Bad Name", "x", "y").is_err());
        assert!(hamlet.create("mayor", "x", "y").is_err());
        assert!(hamlet.create("scout", "", "y").is_err());
        hamlet.create("scout", "Looks things up.", "y").unwrap();
        assert!(hamlet.create("scout", "Again.", "y").is_err());
        assert!(
            hamlet.create(FOUNDER, "The mayor's name.", "y").is_err()
        );
        assert!(hamlet.house("nobody").is_err());
    }

    #[test]
    fn the_mayor_is_an_office_an_agent_holds() {
        let (_dir, hamlet) = hamlet("office");
        hamlet.create("scout", "Looks things up.", "y").unwrap();
        hamlet.create("smith", "Fixes things.", "y").unwrap();
        hamlet.build_road("scout", "smith", "y", "t").unwrap();
        hamlet.carried(MAYOR, "smith", "t").unwrap();
        assert_eq!(hamlet.mayor().name, FOUNDER);
        // The first mayor's house is the hamlet's own folder.
        assert_eq!(hamlet.hall().unwrap().root, hamlet.root());
        assert!(hamlet.set_mayor("nobody", "chief:a", "x").is_err());
        assert!(hamlet.set_mayor(FOUNDER, "chief:a", "x").is_err());

        let old = hamlet.set_mayor("scout", "chief:a", "x").unwrap();
        assert_eq!((old.name.as_str(), old.id), (FOUNDER, 0));
        assert!(hamlet.is_mayor("scout"));
        assert_eq!(hamlet.address("scout"), MAYOR);
        assert_eq!(hamlet.resolve(MAYOR).unwrap().name, "scout");
        // Nothing moved: each keeps its own house and its number.
        assert_eq!(
            hamlet.hall().unwrap().root,
            hamlet.root().join("agents/scout")
        );
        assert_eq!(
            hamlet.house(FOUNDER).unwrap().root,
            hamlet.root()
        );
        assert!(!hamlet.seat(0, FOUNDER).unwrap().1);
        // The old mayor is an ordinary agent with a road to the
        // office; the office keeps the roads it had.
        let names = hamlet
            .residents()
            .into_iter()
            .map(|r| r.name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["smith", FOUNDER]);
        assert!(hamlet.has_road(MAYOR, FOUNDER));
        assert!(!hamlet.has_road(MAYOR, "scout"));
        let smith = hamlet
            .roads()
            .into_iter()
            .find(|r| r.joins(MAYOR, "smith"))
            .unwrap();
        assert_eq!(smith.messages, 1);
        assert!(hamlet.prompt("").contains("You are `scout`"));
        // Its own roads sleep while it holds the office.
        assert!(!hamlet.has_road("scout", "smith"));
        // The first mayor cannot be removed: its house is the
        // hamlet's folder. Nor can whoever is mayor.
        let chief = crate::lifecycle::Who::Chief("a");
        assert!(hamlet.remove(FOUNDER, "x", chief).is_err());
        assert!(hamlet.remove("scout", "x", chief).is_err());

        hamlet.set_mayor(FOUNDER, "chief:a", "x").unwrap();
        assert!(hamlet.has_road("scout", "smith"));
        assert!(hamlet.has_road(MAYOR, "scout"));
        assert!(!hamlet.has_road(MAYOR, FOUNDER));
    }
}
