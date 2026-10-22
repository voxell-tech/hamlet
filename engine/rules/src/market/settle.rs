//! A version of a listing bought, written into the home where the
//! chief chose and recorded in `bought.json`.

use std::path::Path;

use anyhow::{Result, bail};
use hamlet_model::market::package::{Contents, Kind, Package};

use crate::market::install::{
    bring_up, found_bought, hire, install_tools, refresh,
};
use crate::market::kept::{Keeping, Kept};
use crate::realm::Realm;

/// The reason recorded for an agent bought and moved in.
const BOUGHT: &str = "bought at the Agent Bazaar";

/// Where a package bought is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A hamlet founded in the realm.
    Realm,
    /// An agent moved into the hamlet of this name.
    Hamlet(String),
    /// Tools added to the house of `agent` of `hamlet`.
    House { hamlet: String, agent: String },
}

impl Keeping {
    /// Where it was written, to be written there again.
    pub fn target(&self) -> Option<Target> {
        Some(match self.kind {
            Kind::Hamlet => Target::Realm,
            Kind::Agent => Target::Hamlet(self.hamlet.clone()?),
            Kind::Tools => Target::House {
                hamlet: self.hamlet.clone()?,
                agent: self.house.clone()?,
            },
        })
    }
}

/// One version of a listing to write into the home.
pub struct Delivery<'a> {
    /// The listing's id at the bazaar.
    pub id: &'a str,
    pub title: &'a str,
    pub version: &'a str,
    pub package: &'a Package,
    pub target: &'a Target,
    /// The chief, as the realm names them.
    pub by: &'a str,
}

/// Writes `delivery` into `home`: an agent hired or brought up to the
/// version, a hamlet founded or brought up, or tools added or
/// replaced. Returns the record of all that was bought, with this
/// one, and what came of it in words.
pub fn settle(
    home: &Path,
    delivery: Delivery,
) -> Result<(Kept, String)> {
    let Delivery {
        id,
        title,
        version,
        package,
        target,
        by,
    } = delivery;
    let realm = Realm::new(home.to_path_buf());
    let mut kept = Kept::load(home);
    let before = kept.get(id).cloned();
    let (names, hamlet, house, words) =
        match (&package.contents, target) {
            (Contents::Agent { agent }, Target::Hamlet(hamlet)) => {
                let still = before.as_ref().and_then(|before| {
                    let name = before.names.first()?;
                    let hamlet = before.hamlet.clone()?;
                    (realm.hamlet_of(name).as_ref() == Some(&hamlet))
                        .then(|| (hamlet, name.clone()))
                });
                match still {
                    Some((hamlet, name)) => {
                        refresh(
                            &realm.hamlet(&hamlet)?.house(&name)?,
                            agent,
                        )?;
                        let words = format!(
                            "{name} is up to version {version}."
                        );
                        (vec![name], Some(hamlet), None, words)
                    }
                    None => {
                        let at = realm.hamlet(hamlet)?;
                        let name = hire(&at, agent, BOUGHT)?;
                        let words =
                            format!("{name} moved into {hamlet}.");
                        (
                            vec![name],
                            Some(hamlet.clone()),
                            None,
                            words,
                        )
                    }
                }
            }
            (Contents::Hamlet { hamlet }, Target::Realm) => {
                let stands = realm
                    .hamlets()
                    .iter()
                    .any(|p| p.name == hamlet.name);
                let words = match (stands, before.is_some()) {
                    (true, true) => {
                        bring_up(
                            &realm.hamlet(&hamlet.name)?,
                            hamlet,
                        )?;
                        format!(
                            "{} is up to version {version}.",
                            hamlet.title
                        )
                    }
                    (true, false) => bail!(
                        "the realm has a hamlet named `{}` already",
                        hamlet.name
                    ),
                    (false, _) => {
                        found_bought(&realm, hamlet, by)?;
                        format!(
                            "{} stands in your realm, with {} and \
                             all they know.",
                            hamlet.title, hamlet.mayor.name
                        )
                    }
                };
                (vec![hamlet.name.clone()], None, None, words)
            }
            (
                Contents::Tools { tools },
                Target::House { hamlet, agent },
            ) => {
                let replacing = before
                    .as_ref()
                    .filter(|before| {
                        before.hamlet.as_ref() == Some(hamlet)
                            && before.house.as_ref() == Some(agent)
                    })
                    .map(|before| before.names.clone())
                    .unwrap_or_default();
                let house = realm.hamlet(hamlet)?.house(agent)?;
                let names = install_tools(&house, tools, &replacing)?;
                let words = format!(
                    "{agent} can use {} now.",
                    names.join(", ")
                );
                (
                    names,
                    Some(hamlet.clone()),
                    Some(agent.clone()),
                    words,
                )
            }
            _ => bail!("that cannot be installed there"),
        };
    kept.0.insert(
        id.to_string(),
        Keeping {
            kind: package.kind(),
            title: title.to_string(),
            version: version.to_string(),
            names,
            hamlet,
            house,
        },
    );
    kept.save(home)?;
    Ok((kept, words))
}

#[cfg(test)]
mod tests {
    use hamlet_model::market::package::{Agent, File, NOTES};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::HOME;

    fn scribe(character: &str) -> Package {
        let file = |path: &str, text: &str| File {
            path: path.into(),
            data: text.as_bytes().to_vec(),
            executable: false,
        };
        Package::new(Contents::Agent {
            agent: Agent {
                name: "scribe".into(),
                purpose: "Writes letters.".into(),
                model: None,
                files: vec![
                    file("CHARACTER.md", character),
                    file(NOTES, "# Notes\nfrom the seller"),
                ],
            },
        })
    }

    #[test]
    fn an_agent_bought_moves_in_and_is_brought_up_to_a_new_version() {
        let dir = Scratch::new("market-settle");
        let realm = Realm::open(&dir).unwrap();
        let deliver =
            |version, package: &Package, target: &Target| {
                settle(
                    &dir,
                    Delivery {
                        id: "l_scribe",
                        title: "Scribe",
                        version,
                        package,
                        target,
                        by: "alice",
                    },
                )
            };
        let into = Target::Hamlet(HOME.into());
        let (kept, words) =
            deliver("1.0.0", &scribe("Careful."), &into).unwrap();
        assert!(words.contains("moved into"), "{words}");
        let house =
            realm.hamlet(HOME).unwrap().house("scribe").unwrap();
        let read = |path| std::fs::read_to_string(path).unwrap();
        assert_eq!(read(house.character()), "Careful.");
        std::fs::write(house.notes(), "# Notes\nits own").unwrap();

        let target = kept.get("l_scribe").unwrap().target().unwrap();
        let (kept, _) =
            deliver("1.1.0", &scribe("Bolder."), &target).unwrap();
        assert_eq!(kept.get("l_scribe").unwrap().version, "1.1.0");
        assert_eq!(read(house.character()), "Bolder.");
        assert_eq!(read(house.notes()), "# Notes\nits own");
        assert_eq!(realm.hamlet(HOME).unwrap().residents().len(), 1);
        assert_eq!(Kept::load(&dir), kept);

        // An agent cannot be founded as a hamlet.
        assert!(
            deliver("1.1.0", &scribe("x"), &Target::Realm).is_err()
        );
    }
}
