//! A hamlet founded from a template: its mayor and its agents moved
//! in with their characters, notes and skills.

use anyhow::{Result, bail};
use hamlet_home::Home;

use crate::hamlet::{Hamlet, check_name};
use crate::realm::{Origin, Realm, TITLE_LONGEST};

/// A hamlet as it is founded from a template.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    /// Its name on disk and in commands.
    pub name: String,
    /// The name it is shown by.
    pub title: String,
    /// What it is for, as its mayor is told.
    pub purpose: String,
    pub mayor: Settler,
    /// Its agents besides the mayor.
    pub agents: Vec<Settler>,
}

/// An agent of a hamlet founded from a template, with all it knows.
#[derive(Clone, Debug, PartialEq)]
pub struct Settler {
    pub name: String,
    pub purpose: String,
    /// Its `CHARACTER.md`.
    pub character: String,
    /// Its `NOTES.md`.
    pub notes: String,
    pub skills: Vec<SettlerSkill>,
}

/// A skill a settler keeps in its house.
#[derive(Clone, Debug, PartialEq)]
pub struct SettlerSkill {
    /// The name of its folder and of its front matter.
    pub name: String,
    pub description: String,
    /// What follows the front matter.
    pub body: String,
}

impl SettlerSkill {
    /// Its `SKILL.md`.
    pub fn file(&self) -> String {
        // One line, so the front matter stays whole.
        let description =
            self.description.split_whitespace().collect::<Vec<_>>();
        format!(
            "---\nname: {}\ndescription: {}\n---\n\n{}\n",
            self.name,
            description.join(" "),
            self.body.trim()
        )
    }
}

impl Template {
    /// Everyone who comes with it, the mayor first.
    pub fn everyone(&self) -> impl Iterator<Item = &Settler> {
        std::iter::once(&self.mayor).chain(&self.agents)
    }
}

impl Realm {
    /// Founds `template` for `user`, who is its chief, and moves in
    /// its mayor and agents with all they know. Everything is checked
    /// before anything is written, so a hamlet refused leaves nothing
    /// behind.
    pub fn found_from(
        &self,
        template: &Template,
        user: &str,
    ) -> Result<Hamlet> {
        self.may_found(
            &template.name,
            &template.mayor.name,
            &template.purpose,
        )?;
        if template.title.chars().count() > TITLE_LONGEST {
            bail!("a hamlet's name is at most 32 letters");
        }
        let mut names = Vec::<&str>::new();
        for settler in template.everyone() {
            let name = settler.name.as_str();
            check_name(name)?;
            if names.contains(&name) || self.hamlet_of(name).is_some()
            {
                bail!(
                    "an agent named `{name}` already lives in the \
                     realm, and `{}` comes with its own",
                    template.name
                );
            }
            names.push(name);
            for skill in &settler.skills {
                check_name(&skill.name)?;
            }
        }
        let most = Hamlet::at(self.home(), &template.name)
            .limits()?
            .most_agents;
        if template.agents.len() > most {
            bail!(
                "`{}` comes with {} agents, and a hamlet holds at \
                 most {most}",
                template.name,
                template.agents.len()
            );
        }

        let hamlet = self.found(
            &template.name,
            &template.mayor.name,
            &template.purpose,
            Origin::Human {
                user: user.to_string(),
            },
        )?;
        self.name_hamlet(&template.name, user, &template.title)?;
        move_in(
            &hamlet.house(&template.mayor.name)?,
            &template.mayor,
        )?;
        for agent in &template.agents {
            hamlet.create(
                &agent.name,
                &agent.purpose,
                "came with the hamlet, founded from a template",
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
    use hamlet_testing::Scratch;

    use super::*;
    use crate::hamlet::HOME;

    fn settler(name: &str, skill: Option<&str>) -> Settler {
        Settler {
            name: name.into(),
            purpose: format!("{name} works."),
            character: format!("{name} is calm."),
            notes: format!("# Notes of {name}\n"),
            skills: skill
                .map(|skill| SettlerSkill {
                    name: skill.into(),
                    description: "Does it.".into(),
                    body: "Step by step.".into(),
                })
                .into_iter()
                .collect(),
        }
    }

    fn template(name: &str, agents: &[&str]) -> Template {
        Template {
            name: name.into(),
            title: format!("The {name}"),
            purpose: "Do the work.".into(),
            mayor: settler(&format!("{name}-lead"), Some("plan")),
            agents: agents
                .iter()
                .map(|agent| settler(agent, Some("dig")))
                .collect(),
        }
    }

    #[test]
    fn a_hamlet_from_a_template_is_founded_whole_and_only_once() {
        let dir = Scratch::new("realm-template");
        let realm = Realm::open(&dir).unwrap();
        realm
            .hamlet(HOME)
            .unwrap()
            .create("digger", "Digs.", "t")
            .unwrap();

        // One of its agents' names is taken: nothing is founded.
        let taken = template("quarry", &["digger"]);
        assert!(realm.found_from(&taken, "alice").is_err());
        assert!(!Hamlet::at(&dir, "quarry").root().exists());
        assert_eq!(realm.hamlets().len(), 1);
        // So with a skill whose name would leave its folder.
        let mut climbing = template("quarry", &["miner"]);
        climbing.agents[0].skills[0].name = "../../out".into();
        assert!(realm.found_from(&climbing, "alice").is_err());
        assert!(!Hamlet::at(&dir, "quarry").root().exists());

        // Another stands whole, under the name it is shown by and
        // with its chief.
        let mill = template("mill", &["miller", "carter"]);
        let hamlet = realm.found_from(&mill, "alice").unwrap();
        assert_eq!(hamlet.mayor().name, mill.mayor.name);
        assert_eq!(hamlet.residents().len(), mill.agents.len());
        for settler in mill.everyone() {
            let house = hamlet.house(&settler.name).unwrap();
            let skills = house
                .skill_index()
                .into_iter()
                .map(|skill| skill.name)
                .collect::<Vec<_>>();
            assert_eq!(skills, [settler.skills[0].name.clone()]);
            assert_eq!(
                std::fs::read_to_string(house.notes()).unwrap(),
                settler.notes
            );
            assert!(house.character().exists());
        }
        let places = realm.hamlets();
        let place = places.iter().find(|p| p.name == "mill").unwrap();
        assert_eq!(place.shown(), mill.title);
        assert_eq!(place.chiefs, ["alice"]);
        // Founded again, it is refused.
        assert!(realm.found_from(&mill, "alice").is_err());
    }
}
