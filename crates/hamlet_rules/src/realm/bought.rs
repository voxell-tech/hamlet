//! A hamlet bought at the bazaar, founded whole: its mayor and its
//! agents moved in with their characters, notes and skills.

use anyhow::{Result, bail};
use hamlet_model::market::package::Hamlet as Bought;

use super::{Origin, Realm, TITLE_LONGEST};
use crate::hamlet::{Hamlet, check_name};
use crate::market::install::move_in;

impl Realm {
    /// Founds `bought` for `user`, who is its chief, and moves in its
    /// mayor and agents with all they know. Everything is checked
    /// before anything is written, so a hamlet refused leaves
    /// nothing behind.
    pub fn found_bought(
        &self,
        bought: &Bought,
        user: &str,
    ) -> Result<Hamlet> {
        let purpose = match bought.purpose.trim() {
            "" => "came from the Agent Bazaar",
            purpose => purpose,
        };
        self.may_found(&bought.name, &bought.mayor.name, purpose)?;
        if bought.title.chars().count() > TITLE_LONGEST {
            bail!("a hamlet's name is at most 32 letters");
        }
        let mut names = Vec::<&str>::new();
        for settler in bought.everyone() {
            let name = settler.name.as_str();
            check_name(name)?;
            if names.contains(&name) || self.hamlet_of(name).is_some()
            {
                bail!(
                    "an agent named `{name}` already lives in the \
                     realm, and `{}` comes with its own",
                    bought.name
                );
            }
            names.push(name);
        }
        let most = Hamlet::at(self.home(), &bought.name)
            .limits()?
            .most_agents;
        if bought.agents.len() > most {
            bail!(
                "`{}` comes with {} agents, and a hamlet holds at \
                 most {most}",
                bought.name,
                bought.agents.len()
            );
        }

        let hamlet = self.found(
            &bought.name,
            &bought.mayor.name,
            purpose,
            Origin::Human {
                user: user.to_string(),
            },
        )?;
        if !bought.title.trim().is_empty() {
            self.name_hamlet(&bought.name, user, &bought.title)?;
        }
        move_in(&hamlet.house(&bought.mayor.name)?, &bought.mayor)?;
        for agent in &bought.agents {
            let purpose = match agent.purpose.trim() {
                "" => "came with the hamlet",
                purpose => purpose,
            };
            hamlet.create(
                &agent.name,
                purpose,
                "came with the hamlet, bought at the Agent Bazaar",
            )?;
            move_in(&hamlet.house(&agent.name)?, agent)?;
        }
        Ok(hamlet)
    }
}

#[cfg(test)]
mod tests {
    use hamlet_model::market::package::{Agent, File, NOTES};
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::HOME;
    use crate::market::pack_hamlet;

    fn agent(name: &str) -> Agent {
        Agent {
            name: name.into(),
            purpose: format!("{name} works."),
            model: None,
            files: vec![
                File {
                    path: NOTES.into(),
                    data: format!("# Notes\n{name}").into_bytes(),
                    executable: false,
                },
                File {
                    path: format!(
                        "workspace/skills/{name}-way/SKILL.md"
                    ),
                    data: b"---\nname: way\n---\n".to_vec(),
                    executable: false,
                },
            ],
        }
    }

    fn press() -> Bought {
        Bought {
            name: "lamplight".into(),
            title: "Lamplight Press".into(),
            kind: "Publishing house".into(),
            purpose: "Turns a manuscript into a book.".into(),
            seed: "7341".into(),
            mayor: agent("publisher"),
            agents: vec![agent("editor"), agent("setter")],
        }
    }

    #[test]
    fn a_bought_hamlet_is_founded_whole_only_once_and_packs_back() {
        let dir = Scratch::new("realm-bought");
        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        home.create("editor", "Digs.", "t").unwrap();

        // One of its agents' names is taken: nothing is founded.
        assert!(realm.found_bought(&press(), "alice").is_err());
        assert!(!Hamlet::at(&dir, "lamplight").root().exists());
        assert_eq!(realm.hamlets().len(), 1);

        let bought = Bought {
            agents: vec![agent("proofer"), agent("setter")],
            ..press()
        };
        let hamlet = realm.found_bought(&bought, "alice").unwrap();
        assert_eq!(hamlet.mayor().name, "publisher");
        assert_eq!(hamlet.residents().len(), 2);
        let place = realm
            .hamlets()
            .into_iter()
            .find(|p| p.name == "lamplight")
            .unwrap();
        assert_eq!(place.shown(), "Lamplight Press");
        assert_eq!(place.chiefs, ["alice"]);
        // Bought again, it is refused.
        assert!(realm.found_bought(&bought, "alice").is_err());

        let packed = pack_hamlet(
            &realm,
            "lamplight",
            "Publishing house",
            true,
        )
        .unwrap();
        assert_eq!(packed.title, bought.title);
        assert_eq!(packed.purpose, bought.purpose);
        for (sold, again) in bought.everyone().zip(packed.everyone())
        {
            assert_eq!(sold.name, again.name);
            assert_eq!(sold.text(NOTES), again.text(NOTES));
        }
    }
}
