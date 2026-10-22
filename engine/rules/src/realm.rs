//! The realm: every hamlet of one home and the dealings between them
//! (H6, H14, H19, C7), read from `realm.jsonl` in the home. The first
//! hamlet's folder is the home, every other is `hamlets/<name>/`.

mod brief;
mod moves;
mod removal;
mod roads;
mod rules;
mod template;

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use hamlet_model::log;
use serde_json::{Value, json};

use crate::hamlet::{
    FOUNDER, HOME, Hamlet, MAYOR, Resident, check_name,
};
pub use crate::realm::moves::Moved;
pub use crate::realm::removal::Removal;
pub use crate::realm::rules::Rules;
pub use crate::realm::template::{Settler, SettlerSkill, Template};
use crate::record::{self, text};

/// The most letters of what a chief calls a hamlet.
const TITLE_LONGEST: usize = 32;

/// The origin of a hamlet.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// Founded by a human, who is its chief (C7).
    Human { user: String },
    /// Founded by vote `id` of `hamlet`, whose chiefs it belongs to
    /// (C7).
    Vote { hamlet: String, id: u64 },
}

/// A hamlet as the realm's record knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// Its chiefs. Empty means the human the home belongs to.
    pub chiefs: Vec<String>,
    /// The hamlet whose vote founded it.
    pub from: Option<String>,
    /// Its chief's name for it. Its `name` is the one in commands
    /// and on disk, and never changes.
    pub title: Option<String>,
    /// The place in the world its chief set its town down at, as x
    /// and z. None while it stands where the world first put it.
    pub at: Option<(f32, f32)>,
}

impl Place {
    /// The name it is shown under.
    pub fn shown(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// Every hamlet of one home, under the rules its chief set.
pub struct Realm {
    home: PathBuf,
    pub rules: Rules,
}

/// The address of a hamlet's office of mayor from outside it.
pub fn office(hamlet: &str) -> String {
    format!("{MAYOR}@{hamlet}")
}

fn names(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

impl Realm {
    /// The realm of `home`, under the rules its chief has set.
    pub fn new(home: PathBuf) -> Self {
        let rules = Rules::read(&home);
        Realm::with(home, rules)
    }

    pub fn with(home: PathBuf, rules: Rules) -> Self {
        Realm { home, rules }
    }

    /// The realm of `home`. A home from before several hamlets is
    /// taken in as it stands: its one hamlet becomes the first, in
    /// place, and its mayor is given a name.
    pub fn open(home: &Path) -> Result<Self> {
        let realm = Realm::new(home.to_path_buf());
        if !realm.record().exists() {
            let first = Hamlet::new(home.to_path_buf());
            // Everyone but the mayor, who has no name yet.
            let taken = first
                .residents()
                .into_iter()
                .map(|r| r.name)
                .collect::<Vec<_>>();
            let mayor = [FOUNDER, "first-mayor", "old-mayor"]
                .into_iter()
                .find(|n| !taken.iter().any(|t| t == n))
                .unwrap_or(FOUNDER);
            first.found(mayor, "", true)?;
            realm.append(json!({
                "v": 1,
                "type": "hamlet_founded",
                "ts": log::now_millis(),
                "name": HOME,
                "mayor": mayor,
                "chiefs": [],
            }))?;
        }
        Ok(realm)
    }

    /// The home the realm is of.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The path of the record of the realm of `home`.
    pub fn path(home: &Path) -> PathBuf {
        home.join("realm.jsonl")
    }

    fn record(&self) -> PathBuf {
        Realm::path(&self.home)
    }

    fn events(&self) -> Vec<Value> {
        record::read(&self.record())
    }

    fn append(&self, event: Value) -> Result<()> {
        record::append(&self.record(), &event)
    }

    /// The hamlets that exist and those removed, each removed one
    /// with where it is archived.
    fn places(&self) -> (Vec<Place>, Vec<(Place, PathBuf)>) {
        self.places_in(&self.events())
    }

    /// `places` from the record's lines already read.
    fn places_in(
        &self,
        events: &[Value],
    ) -> (Vec<Place>, Vec<(Place, PathBuf)>) {
        let mut live = vec![Place {
            name: HOME.to_string(),
            chiefs: Vec::new(),
            from: None,
            title: None,
            at: None,
        }];
        let mut gone = Vec::<(Place, PathBuf)>::new();
        for e in events {
            let name = text(e, "name");
            match e["type"].as_str() {
                Some("hamlet_founded") => {
                    let place = Place {
                        name: name.clone(),
                        chiefs: names(&e["chiefs"]),
                        from: e["from"].as_str().map(str::to_string),
                        title: None,
                        at: None,
                    };
                    match live.iter_mut().find(|p| p.name == name) {
                        Some(known) => *known = place,
                        None => live.push(place),
                    }
                }
                Some("hamlet_named") => {
                    if let Some(place) =
                        live.iter_mut().find(|p| p.name == name)
                    {
                        let title = text(e, "title");
                        place.title =
                            (!title.is_empty()).then_some(title);
                    }
                }
                Some("hamlet_moved") => {
                    let at = e["x"].as_f64().zip(e["z"].as_f64());
                    if let Some(place) =
                        live.iter_mut().find(|p| p.name == name)
                    {
                        place.at =
                            at.map(|(x, z)| (x as f32, z as f32));
                    }
                }
                Some("hamlet_removed") => {
                    if let Some(at) =
                        live.iter().position(|p| p.name == name)
                    {
                        let archive =
                            self.home.join(text(e, "archive"));
                        gone.push((live.remove(at), archive));
                    }
                }
                Some("hamlet_restored") => {
                    if let Some(at) =
                        gone.iter().rposition(|(p, _)| p.name == name)
                    {
                        live.push(gone.remove(at).0);
                    }
                }
                Some("hamlet_erased") => {
                    if let Some(at) =
                        gone.iter().rposition(|(p, _)| p.name == name)
                    {
                        gone.remove(at);
                    }
                }
                _ => {}
            }
        }
        (live, gone)
    }

    pub fn hamlets(&self) -> Vec<Place> {
        self.places().0
    }

    /// Removed hamlets and the folder each is archived in.
    pub fn archived(&self) -> Vec<(String, PathBuf)> {
        self.places()
            .1
            .into_iter()
            .map(|(place, at)| (place.name, at))
            .collect()
    }

    /// Removed hamlets, each with what its chief called it.
    pub fn closed(&self) -> Vec<Place> {
        self.places()
            .1
            .into_iter()
            .map(|(place, _)| place)
            .collect()
    }

    pub fn hamlet(&self, name: &str) -> Result<Hamlet> {
        if !self.hamlets().iter().any(|p| p.name == name) {
            bail!("no hamlet named `{name}`");
        }
        Ok(Hamlet::at(&self.home, name))
    }

    /// The chiefs of a hamlet. Empty means the human the home
    /// belongs to.
    pub fn chiefs(&self, hamlet: &str) -> Vec<String> {
        self.hamlets()
            .into_iter()
            .find(|p| p.name == hamlet)
            .map(|p| p.chiefs)
            .unwrap_or_default()
    }

    pub fn is_chief(&self, hamlet: &str, user: &str) -> bool {
        self.hamlets().iter().any(|p| {
            p.name == hamlet
                && (p.chiefs.is_empty()
                    || p.chiefs.iter().any(|c| c == user))
        })
    }

    /// Every agent of every hamlet, with the hamlet it lives in.
    fn everyone(&self) -> Vec<(Resident, String)> {
        self.everyone_in(&self.hamlets())
    }

    /// `everyone` of the hamlets `live`.
    fn everyone_in(&self, live: &[Place]) -> Vec<(Resident, String)> {
        live.iter()
            .flat_map(|p| {
                Hamlet::at(&self.home, &p.name)
                    .members()
                    .into_iter()
                    .map(move |r| (r, p.name.clone()))
            })
            .collect()
    }

    /// The hamlet the agent `name` lives in.
    pub fn hamlet_of(&self, name: &str) -> Option<String> {
        self.everyone()
            .into_iter()
            .find(|(r, _)| r.name == name)
            .map(|(_, hamlet)| hamlet)
    }

    /// The address an agent has from outside its hamlet: the office
    /// while it is mayor, its name otherwise.
    pub fn address(&self, hamlet: &Hamlet, name: &str) -> String {
        if hamlet.is_mayor(name) {
            office(hamlet.name())
        } else {
            name.to_string()
        }
    }

    /// An address as the realm writes it, for one a model wrote
    /// with a hamlet it needs not: a mayor's own name is its office,
    /// and another agent's is its name alone.
    pub fn plain(&self, address: &str) -> String {
        let Some((name, place)) = address.split_once('@') else {
            return address.to_string();
        };
        match self.hamlet(place) {
            Ok(hamlet) if name != MAYOR && hamlet.is_mayor(name) => {
                format!("{MAYOR}@{place}")
            }
            Ok(hamlet)
                if name != MAYOR
                    && hamlet.resolve(name).is_some() =>
            {
                name.to_string()
            }
            _ => address.to_string(),
        }
    }

    /// The hamlet and agent an address reaches: `mayor@<hamlet>`
    /// for an office, a name for an agent.
    pub fn resolve(
        &self,
        address: &str,
    ) -> Option<(Hamlet, Resident)> {
        let (hamlet, name) =
            match address.strip_prefix(&format!("{MAYOR}@")) {
                Some(hamlet) => (hamlet.to_string(), MAYOR),
                None => (self.hamlet_of(address)?, address),
            };
        let hamlet = self.hamlet(&hamlet).ok()?;
        let resident = hamlet.resolve(name)?;
        Some((hamlet, resident))
    }

    /// `Ok` when a hamlet could be founded with these names, and the
    /// reason when not.
    pub(crate) fn may_found(
        &self,
        name: &str,
        mayor: &str,
        purpose: &str,
    ) -> Result<()> {
        check_name(name)?;
        check_name(mayor)?;
        if purpose.trim().is_empty() {
            bail!("say what the new hamlet is for in `purpose`");
        }
        let (live, _) = self.places();
        if live.iter().any(|p| p.name == name)
            || Hamlet::at(&self.home, name).root().exists()
        {
            bail!("a hamlet named `{name}` already exists");
        }
        if live.len() >= self.rules.most_hamlets {
            bail!(
                "the chief's limits allow no more hamlets ({} of {})",
                live.len(),
                self.rules.most_hamlets
            );
        }
        if self.hamlet_of(mayor).is_some() {
            bail!(
                "a new hamlet gets a new mayor, and an agent named \
                 `{mayor}` already exists"
            );
        }
        Ok(())
    }

    /// Founds a hamlet under a new mayor (H6). It has a road to
    /// every other mayor from the start (H14).
    pub fn found(
        &self,
        name: &str,
        mayor: &str,
        purpose: &str,
        origin: Origin,
    ) -> Result<Hamlet> {
        self.may_found(name, mayor, purpose)?;
        let (chiefs, from, by) = match &origin {
            Origin::Human { user } => {
                (vec![user.clone()], None, format!("human:{user}"))
            }
            Origin::Vote { hamlet, id } => (
                self.chiefs(hamlet),
                Some(hamlet.clone()),
                format!("vote:{hamlet}/{id}"),
            ),
        };
        let hamlet = Hamlet::at(&self.home, name);
        hamlet.found(mayor, purpose, false)?;
        hamlet.house(mayor)?;
        self.append(json!({
            "v": 1,
            "type": "hamlet_founded",
            "ts": log::now_millis(),
            "name": name,
            "mayor": mayor,
            "by": by,
            "from": from,
            "chiefs": chiefs,
        }))?;
        Ok(hamlet)
    }

    /// A chief gives the hamlet `target` the name it is shown by.
    /// An empty one puts back its own.
    pub(crate) fn name_hamlet(
        &self,
        target: &str,
        chief: &str,
        title: &str,
    ) -> Result<()> {
        self.hamlet(target)?;
        if !self.is_chief(target, chief) {
            bail!("only a chief of `{target}` can name it");
        }
        let title =
            title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.chars().count() > TITLE_LONGEST {
            bail!("a hamlet's name is at most 32 letters");
        }
        self.append(json!({
            "v": 1,
            "type": "hamlet_named",
            "ts": log::now_millis(),
            "name": target,
            "by": chief,
            "title": title,
        }))
    }

    /// A chief sets the town of the hamlet `target` down at `x` and
    /// `z` in the world. Whether the ground there can take it is
    /// for the world to say.
    pub(crate) fn move_hamlet(
        &self,
        target: &str,
        chief: &str,
        (x, z): (f32, f32),
    ) -> Result<()> {
        self.hamlet(target)?;
        if !self.is_chief(target, chief) {
            bail!("only a chief of `{target}` can move it");
        }
        if !x.is_finite() || !z.is_finite() {
            bail!("a place in the world is two numbers");
        }
        self.append(json!({
            "v": 1,
            "type": "hamlet_moved",
            "ts": log::now_millis(),
            "name": target,
            "by": chief,
            "x": x,
            "z": z,
        }))
    }
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    fn realm(tag: &str) -> (Scratch, Realm) {
        let dir = Scratch::new(&format!("realm-{tag}"));
        let realm = Realm::open(&dir).unwrap();
        (dir, realm)
    }

    #[test]
    fn a_realm_is_run_by_the_rules_its_chief_wrote() {
        let (dir, realm) = realm("rules");
        // With none written, the rules are as they are by default.
        assert_eq!(realm.rules, Rules::default());
        std::fs::write(
            hamlet_home::limits::path(&dir),
            "[votes]\npass = [1, 2]\nleast_voters = 2\nopen_minutes \
             = 5\n[hamlets]\nmost = 1\n",
        )
        .unwrap();
        let rules = Realm::new(dir.to_path_buf()).rules;
        assert_eq!(rules.pass, (1, 2));
        assert_eq!(rules.least_voters, 2);
        assert_eq!(rules.open_ms, 5 * 60 * 1000);
        assert_eq!(rules.most_hamlets, 1);
        // A rule that was not written keeps its value.
        assert_eq!(rules.retry_ms, Rules::default().retry_ms);
        // No second hamlet is founded in a home that allows one.
        let full = Realm::new(dir.to_path_buf()).found(
            "mill",
            "miller",
            "Trade.",
            human(),
        );
        assert!(full.is_err());
    }

    fn human() -> Origin {
        Origin::Human {
            user: "alice".into(),
        }
    }

    /// A home with agents `a` and `b`, and a hamlet `mill` with `x`
    /// and `y`.
    fn two(tag: &str) -> (Scratch, Realm, Hamlet, Hamlet) {
        let (dir, realm) = realm(tag);
        let home = realm.hamlet(HOME).unwrap();
        home.create("a", "Works.", "t").unwrap();
        home.create("b", "Works.", "t").unwrap();
        let mill = realm
            .found("mill", "miller", "Grinds.", human())
            .unwrap();
        mill.create("x", "Works.", "t").unwrap();
        mill.create("y", "Works.", "t").unwrap();
        (dir, realm, home, mill)
    }

    #[test]
    fn an_old_home_becomes_the_first_hamlet_where_it_stands() {
        let dir = Scratch::new("realm-old");
        // A home from before: a mayor at the root with two agents,
        // one of them named like the default first mayor.
        let old = Hamlet::new(dir.to_path_buf());
        hamlet_home::Home::open(dir.to_path_buf(), None).unwrap();
        std::fs::write(dir.join("workspace/NOTES.md"), "mine")
            .unwrap();
        for name in ["founder", "scout"] {
            old.append(json!({
                "v": 1, "type": "agent_created", "ts": 1,
                "name": name, "purpose": "Works.", "reason": "t",
            }))
            .unwrap();
        }
        old.carried(MAYOR, "scout", "t").unwrap();
        let before =
            std::fs::read_to_string(dir.join("hamlet.jsonl"))
                .unwrap();

        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        // Nothing moved and nothing was rewritten.
        let after = std::fs::read_to_string(dir.join("hamlet.jsonl"))
            .unwrap();
        assert!(after.starts_with(&before));
        assert_eq!(
            std::fs::read_to_string(dir.join("workspace/NOTES.md"))
                .unwrap(),
            "mine"
        );
        assert_eq!(home.mayor().name, "first-mayor");
        assert_eq!(home.hall().unwrap().root, dir.to_path_buf());
        assert_eq!(home.residents().len(), 2);
        assert_eq!(home.roads().len(), 2);
        assert_eq!(realm.hamlets().len(), 1);
        // Opening again changes nothing.
        Realm::open(&dir).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("hamlet.jsonl"))
                .unwrap(),
            after
        );
    }

    #[test]
    fn a_human_founds_a_hamlet_that_is_theirs() {
        let (_dir, realm, home, mill) = two("found");
        assert_eq!(realm.chiefs("mill"), ["alice"]);
        assert!(realm.is_chief("mill", "alice"));
        assert!(!realm.is_chief("mill", "bob"));
        assert!(realm.is_chief(HOME, "anyone"));
        assert_eq!(
            mill.hall().unwrap().root,
            home.root().join("hamlets/mill/agents/miller")
        );
        // Names are unique across the realm.
        assert!(mill.create("a", "Again.", "t").is_err());
        assert!(realm.found("mill", "m", "x", human()).is_err());
        assert!(realm.found("dock", "a", "x", human()).is_err());
        assert!(realm.found(HOME, "m", "x", human()).is_err());
        // Every mayor has a road to every other from the start.
        realm.found("dock", "docker", "Ships.", human()).unwrap();
        let roads = realm.roads();
        assert_eq!(roads.len(), 3);
        assert!(realm.has_road("mayor@home", "mayor@mill"));
        assert!(realm.has_road("mayor@dock", "mayor@mill"));
        let (hamlet, mayor) = realm.resolve("mayor@mill").unwrap();
        assert_eq!(
            (hamlet.name(), mayor.name.as_str()),
            ("mill", "miller")
        );
    }

    #[test]
    fn a_road_between_hamlets_needs_both_mayors() {
        let (_dir, realm, _, _) = two("road");
        assert!(realm.agree_road(HOME, "a", "b", "r").is_err());
        assert!(realm.agree_road("mill", "x", "y", "r").is_err());
        assert!(realm.agree_road(HOME, "a", "nobody", "r").is_err());
        assert!(
            realm.agree_road(HOME, "a", "mayor@mill", "r").is_err()
        );
        assert!(!realm.agree_road(HOME, "a", "x", "r").unwrap());
        assert!(!realm.has_road("a", "x"));
        // Agreeing twice is not two mayors.
        assert!(realm.agree_road(HOME, "x", "a", "r").is_err());
        let mill = realm.hamlet("mill").unwrap();
        assert!(
            realm
                .brief(&mill, "miller")
                .contains("agreed to a road between `a` and `x`")
        );
        assert!(realm.agree_road("mill", "x", "a", "r").unwrap());
        assert!(realm.has_road("x", "a"));
        assert!(realm.agree_road("mill", "x", "a", "r").is_err());
        // The asker pays.
        realm.carried("a", "x", HOME).unwrap();
        assert_eq!((realm.paid(HOME), realm.paid("mill")), (1, 0));
        let road = realm
            .roads()
            .into_iter()
            .find(|r| r.joins("a", "x"))
            .unwrap();
        assert_eq!(road.messages, 1);

        let closed = Realm::with(
            realm.home.clone(),
            Rules {
                roads_between_hamlets: false,
                ..Rules::default()
            },
        );
        assert!(closed.agree_road(HOME, "b", "y", "r").is_err());
    }

    #[test]
    fn an_agent_moves_with_its_house_when_both_mayors_agree() {
        let (_dir, realm, home, mill) = two("move");
        home.create("c", "Works.", "t").unwrap();
        home.build_road("a", "b", "t", "t").unwrap();
        home.build_road("a", "c", "t", "t").unwrap();
        let house = home.house("a").unwrap();
        std::fs::write(house.notes(), "what a knows").unwrap();

        let keep = |names: &[&str]| {
            names.iter().map(|n| n.to_string()).collect::<Vec<_>>()
        };
        // A mayor never leaves, and only the two mayors decide.
        assert!(
            realm
                .agree_move(HOME, FOUNDER, "mill", &[], "r")
                .is_err()
        );
        assert!(
            realm
                .agree_move("mill", "miller", HOME, &[], "r")
                .is_err()
        );
        realm.found("dock", "docker", "Ships.", human()).unwrap();
        assert!(
            realm.agree_move("dock", "a", "mill", &[], "r").is_err()
        );
        assert!(realm.agree_move(HOME, "a", HOME, &[], "r").is_err());
        assert!(
            realm.agree_move(HOME, "a", "nowhere", &[], "r").is_err()
        );

        let first = realm
            .agree_move(HOME, "a", "mill", &keep(&["b", "c"]), "r")
            .unwrap();
        assert_eq!(first, None);
        assert_eq!(realm.hamlet_of("a").as_deref(), Some(HOME));
        assert!(
            realm
                .brief(&mill, "miller")
                .contains("`a` moves to `mill`")
        );
        let moved = realm
            .agree_move("mill", "a", "mill", &keep(&["b"]), "r")
            .unwrap()
            .unwrap();
        assert_eq!(moved.kept, ["b"]);
        assert_eq!(moved.lapsed, ["c"]);

        assert_eq!(realm.hamlet_of("a").as_deref(), Some("mill"));
        assert!(home.member("a").is_none());
        assert!(!home.root().join("agents/a").exists());
        let house = mill.house("a").unwrap();
        assert_eq!(
            std::fs::read_to_string(house.notes()).unwrap(),
            "what a knows"
        );
        // It has a road to its new mayor, and the kept road now
        // crosses hamlets.
        assert!(mill.has_road(MAYOR, "a"));
        assert!(home.roads().iter().all(|r| !r.has("a")));
        assert!(realm.has_road("a", "b"));
        assert!(!realm.has_road("a", "c"));
        // The lapsed road can be agreed again like any other.
        assert!(!realm.agree_road(HOME, "a", "c", "r").unwrap());
        assert!(realm.agree_road("mill", "a", "c", "r").unwrap());
    }

    #[test]
    fn removing_a_hamlet_waits_for_its_chief_and_archives_every_house()
     {
        let (_dir, realm, _, mill) = two("remove");
        for name in ["dock", "farm", "fort"] {
            realm
                .found(name, &format!("{name}-mayor"), "x", human())
                .unwrap();
        }
        realm.agree_road(HOME, "a", "x", "r").unwrap();
        realm.agree_road("mill", "a", "x", "r").unwrap();
        // Two other mayors are not enough, three are.
        assert!(
            !realm.ask_removal("dock", "mill", "idle").unwrap().ready
        );
        assert!(
            !realm.ask_removal("farm", "mill", "idle").unwrap().ready
        );
        assert!(
            !realm.ask_removal("farm", "mill", "idle").unwrap().ready
        );
        assert!(
            realm.ask_removal("fort", "mill", "idle").unwrap().ready
        );
        // Asking removes nothing.
        assert!(mill.root().exists());
        assert!(realm.remove("mill", "bob", "x").is_err());
        assert!(realm.remove(HOME, "alice", "x").is_err());

        let archive = realm.remove("mill", "alice", "idle").unwrap();
        assert!(!mill.root().exists());
        for agent in ["miller", "x", "y"] {
            assert!(
                archive
                    .join("agents")
                    .join(agent)
                    .join("workspace/NOTES.md")
                    .exists()
            );
        }
        assert!(archive.join("hamlet.jsonl").exists());
        assert!(realm.hamlet("mill").is_err());
        assert_eq!(realm.hamlet_of("x"), None);
        assert!(!realm.has_road("a", "x"));
        assert!(!realm.has_road("mayor@home", "mayor@mill"));

        // The chief's word alone is enough, and its own mayor's
        // decision makes it ready at once.
        assert!(
            realm.ask_removal("dock", "dock", "done").unwrap().own
        );
        realm.remove("farm", "alice", "mine").unwrap();

        // The chief can bring it back as it was (H20).
        assert!(realm.restore("mill", "bob").is_err());
        let back = realm.restore("mill", "alice").unwrap();
        assert_eq!(back.mayor().name, "miller");
        assert_eq!(back.residents().len(), 2);
        assert!(realm.has_road("a", "x"));
        assert!(!realm.removal("mill").ready);
    }
}
