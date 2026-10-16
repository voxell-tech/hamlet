//! A hamlet bought at the bazaar, founded whole: its mayor and its
//! agents moved in with their characters, notes and skills.

use anyhow::{Result, bail};
use hamlet_home::Home;
use hamlet_model::market::hamlets::{Listing, Settler};

use super::{Origin, Realm, TITLE_LONGEST};
use crate::hamlet::{Hamlet, check_name};

impl Realm {
    /// Founds `bought` for `user`, who is its chief, and moves in its
    /// mayor and agents with all they know. Everything is checked
    /// before anything is written, so a hamlet refused leaves
    /// nothing behind.
    pub fn found_bought(
        &self,
        bought: &Listing,
        user: &str,
    ) -> Result<Hamlet> {
        self.may_found(
            &bought.name,
            &bought.mayor.name,
            &bought.purpose,
        )?;
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
            for skill in &settler.skills {
                check_name(&skill.name)?;
            }
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
            &bought.purpose,
            Origin::Human {
                user: user.to_string(),
            },
        )?;
        self.name_hamlet(&bought.name, user, &bought.title)?;
        move_in(&hamlet.house(&bought.mayor.name)?, &bought.mayor)?;
        for agent in &bought.agents {
            hamlet.create(
                &agent.name,
                &agent.purpose,
                "came with the hamlet, bought at the Agent Bazaar",
            )?;
            move_in(&hamlet.house(&agent.name)?, agent)?;
        }
        Ok(hamlet)
    }
}

/// Writes what `settler` knows into its house.
fn move_in(house: &Home, settler: &Settler) -> Result<()> {
    if !settler.character.trim().is_empty() {
        house.set_character(&settler.character)?;
    }
    if !settler.notes.trim().is_empty() {
        std::fs::write(house.notes(), &settler.notes)?;
    }
    for skill in &settler.skills {
        let folder = house.skills().join(&skill.name);
        std::fs::create_dir_all(&folder)?;
        std::fs::write(folder.join("SKILL.md"), skill.file())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hamlet_model::market::hamlets::for_sale;
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::HOME;

    #[test]
    fn a_bought_hamlet_is_founded_whole_and_only_once() {
        let dir = Scratch::new("realm-bought");
        let realm = Realm::open(&dir).unwrap();
        let home = realm.hamlet(HOME).unwrap();
        home.create("quarry", "Digs.", "t").unwrap();
        let all = for_sale();

        // One of its agents' names is taken: nothing is founded.
        let lamplight = &all[0];
        assert!(realm.found_bought(lamplight, "alice").is_err());
        assert!(!Hamlet::at(&dir, &lamplight.name).root().exists());
        assert_eq!(realm.hamlets().len(), 1);

        // Every other one stands whole, under the name it is shown
        // by and with its chief.
        for bought in &all[1..] {
            let hamlet = realm.found_bought(bought, "alice").unwrap();
            assert_eq!(hamlet.mayor().name, bought.mayor.name);
            assert_eq!(hamlet.residents().len(), bought.agents.len());
            for settler in bought.everyone() {
                let house = hamlet.house(&settler.name).unwrap();
                let skills = house
                    .skill_index()
                    .into_iter()
                    .map(|skill| skill.name)
                    .collect::<Vec<_>>();
                let kept = settler
                    .skills
                    .iter()
                    .map(|skill| skill.name.clone())
                    .collect::<Vec<_>>();
                assert_eq!(skills, kept);
                assert_eq!(
                    std::fs::read_to_string(house.notes()).unwrap(),
                    settler.notes
                );
                assert!(house.character().exists());
            }
        }
        let places = realm.hamlets();
        let ink =
            places.iter().find(|p| p.name == "inkwell").unwrap();
        assert_eq!(ink.shown(), "Inkwell Row");
        assert_eq!(ink.chiefs, ["alice"]);
        // Bought again, it is refused.
        assert!(realm.found_bought(&all[1], "alice").is_err());
    }
}
