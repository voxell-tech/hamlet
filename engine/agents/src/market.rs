//! What the Agent Bazaar asks of the agents: a model added to the
//! catalogue, a skill taken into the library and a hamlet founded
//! from a template.

use bevy::ecs::world::World;
use hamlet_core::agents;
use hamlet_core::catalogue::ModelCatalogue;
use hamlet_core::core::{HomeDir, RefreshLists, Settings};
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{
    ListedModel, Settler, SettlerSkill, SkillFile, Template,
};
use hamlet_model::catalogue::{self, Catalogue, Listed, Price};
use hamlet_rules::library::Library;
use hamlet_rules::realm::{self, Realm};

use crate::Chief;
use crate::answer::MOST_PRICE;

/// The most letters of a skill's name.
const SKILL_NAME_LONGEST: usize = 64;

/// The most files a skill taken may hold.
const MOST_SKILL_FILES: usize = 256;

/// The most bytes of text a skill taken may hold, all its files
/// together.
const MOST_SKILL_BYTES: usize = 4 << 20;

/// The Agent Bazaar's methods on [`World`].
pub(crate) trait MarketWorldExt {
    /// Adds `model`, listed by the endpoint `endpoint` of the
    /// settings, to the catalogue, allowed, and gives the name it
    /// goes by.
    fn add_model(
        &mut self,
        endpoint: &str,
        model: ListedModel,
    ) -> Result<String, Failure>;

    /// Takes the skill `name`, made of `files`, into the library for
    /// the chief, as brought from `from`.
    fn take_skill(
        &mut self,
        name: &str,
        files: Vec<SkillFile>,
        from: &str,
    ) -> Result<(), Failure>;

    /// Founds `template` for the chief, and brings its agents up.
    fn found_from(
        &mut self,
        template: Template,
    ) -> Result<(), Failure>;
}

impl MarketWorldExt for World {
    fn add_model(
        &mut self,
        endpoint: &str,
        model: ListedModel,
    ) -> Result<String, Failure> {
        let at = self
            .resource::<Settings>()
            .config
            .endpoint(endpoint)
            .cloned()
            .ok_or(Failure::NotFound)?;
        let id = model.id.trim();
        if id.is_empty() {
            return Err(Failure::Refused(
                "the model has no id".into(),
            ));
        }
        let fits = |price: Option<f64>| {
            price.is_none_or(|price| {
                price.is_finite()
                    && (0.0..=MOST_PRICE).contains(&price)
            })
        };
        if !fits(model.input) || !fits(model.output) {
            return Err(Failure::Refused(format!(
                "a price is from 0 to {MOST_PRICE}"
            )));
        }
        let listed = Listed {
            id: id.to_string(),
            price: model
                .input
                .zip(model.output)
                .map(|(input, output)| Price { input, output }),
            context: model.context,
        };
        let home = self.resource::<HomeDir>().0.root.clone();
        let mut all = Catalogue::load(&home);
        let added = all.add(
            endpoint,
            &listed,
            catalogue::is_local(&at.base_url),
        );
        let note = model.note.split_whitespace().collect::<Vec<_>>();
        if !note.is_empty() {
            added.note = note.join(" ");
        }
        let name = added.name();
        let saved = all.save(&home);
        self.resource_mut::<ModelCatalogue>().reread();
        saved
            .map(|()| name)
            .map_err(|why| Failure::Failed(format!("{why:#}")))
    }

    fn take_skill(
        &mut self,
        name: &str,
        files: Vec<SkillFile>,
        from: &str,
    ) -> Result<(), Failure> {
        let fits = !name.is_empty()
            && name.len() <= SKILL_NAME_LONGEST
            && name.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_digit()
                    || c == '-'
            });
        if !fits {
            return Err(Failure::Refused(format!(
                "a skill's name is 1 to {SKILL_NAME_LONGEST} \
                 lowercase letters, digits and hyphens"
            )));
        }
        let bytes =
            files.iter().map(|file| file.text.len()).sum::<usize>();
        if files.len() > MOST_SKILL_FILES || bytes > MOST_SKILL_BYTES
        {
            return Err(Failure::Refused(format!(
                "a skill holds at most {MOST_SKILL_FILES} files and \
                 {} MB",
                MOST_SKILL_BYTES >> 20
            )));
        }
        let home = self.resource::<HomeDir>().0.root.clone();
        let library = Library::of(&home);
        if library.root().join(name).exists() {
            return Err(Failure::Refused(format!(
                "the library already has a skill named `{name}`"
            )));
        }
        let files = files
            .into_iter()
            .map(|file| (file.path, file.text))
            .collect::<Vec<_>>();
        let by = format!("chief:{}", self.resource::<Chief>().name);
        library
            .give(name, &files, &by, from)
            .map_err(|why| Failure::Failed(format!("{why:#}")))?;
        self.trigger(RefreshLists);
        Ok(())
    }

    fn found_from(
        &mut self,
        template: Template,
    ) -> Result<(), Failure> {
        let home = self.resource::<HomeDir>().0.root.clone();
        let chief = self.resource::<Chief>().name.clone();
        let template = realm::Template {
            name: template.name,
            title: template.title,
            purpose: template.purpose,
            mayor: settler(template.mayor),
            agents: template
                .agents
                .into_iter()
                .map(settler)
                .collect(),
        };
        Realm::new(home)
            .found_from(&template, &chief)
            .map_err(|why| Failure::Failed(format!("{why:#}")))?;
        agents::sync(self);
        Ok(())
    }
}

fn settler(settler: Settler) -> realm::Settler {
    realm::Settler {
        name: settler.name,
        purpose: settler.purpose,
        character: settler.character,
        notes: settler.notes,
        skills: settler.skills.into_iter().map(skill).collect(),
    }
}

fn skill(skill: SettlerSkill) -> realm::SettlerSkill {
    realm::SettlerSkill {
        name: skill.name,
        description: skill.description,
        body: skill.body,
    }
}
