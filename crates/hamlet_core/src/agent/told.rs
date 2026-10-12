//! The system prompt of an agent, from its house, its hamlet and the
//! realm.

use hamlet_home::{Home, Project, charter};
use hamlet_rules::hamlet::{Hamlet, Resident};
use hamlet_rules::library::Library;
use hamlet_rules::realm::{Realm, Rules};

/// Everything an agent's system prompt is made from.
pub(crate) struct Told<'a> {
    pub house: &'a Home,
    /// The sandbox and network line.
    pub environment: &'a str,
    /// The model it runs, by name.
    pub model: &'a str,
    /// The tools that failed to load, as the prompt says them.
    pub faults: &'a str,
    pub project: Option<&'a Project>,
    pub library: Option<&'a Library>,
    /// Its hamlet and its entry in the record there.
    pub place: Option<(&'a Hamlet, &'a Resident)>,
    /// It holds the mayor's office.
    pub mayor: bool,
    pub rules: Rules,
    /// The words on the hamlet's budget.
    pub budget: &'a str,
}

impl Told<'_> {
    /// The prompt: the charter, the machine, the project, the
    /// character, the notes and the skills, then the library, the
    /// hamlet and the realm.
    pub(crate) fn prompt(&self) -> String {
        let mut environment = self.environment.to_string();
        if !self.model.is_empty() {
            environment.push_str(&format!(" Model: {}.", self.model));
        }
        environment.push_str(self.faults);
        // The first mayor's house is the home itself.
        let home = match self.place {
            Some((hamlet, _)) => hamlet.home(),
            None => &self.house.root,
        };
        let mut system = self.house.system_prompt(
            &charter::read(home),
            &environment,
            self.project,
        );
        if let Some(library) = self.library {
            system.push_str(&library.prompt());
        }
        if let Some((hamlet, resident)) = self.place {
            system.push_str(&match self.mayor {
                true => hamlet.prompt(self.budget),
                false => hamlet.identity(resident, self.budget),
            });
            let realm =
                Realm::with(hamlet.home().to_path_buf(), self.rules);
            system.push_str(&realm.brief(hamlet, &resident.name));
        }
        system
    }
}
