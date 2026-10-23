//! The chief's gifts to an agent, as the app gives them by hand: a
//! skill for its house, or a file for its inbox.

use std::path::{Path, PathBuf};

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_core::agents::{self, LivesIn, Settlement};
use hamlet_core::core::HomeDir;
use hamlet_core::session::UserMessage;
use hamlet_engine_api::Failure;
use hamlet_engine_api::agents::{Gift, Recipient};
use hamlet_engine_grants::DroppedFiles;
use hamlet_rules::gifts;
use hamlet_rules::hamlet::{HOME, Hamlet};

use crate::Chief;
use crate::answer::AgentsWorldExt;
use crate::conversation::ConversationWorldExt;

/// The gifts' methods on [`World`].
pub(crate) trait GiftsWorldExt {
    /// The living agent `name`, its name in its record, and the
    /// record of its hamlet.
    fn receiver(
        &mut self,
        name: &str,
    ) -> Result<(Entity, String, Hamlet), Failure>;

    /// The skills `agent` could be given.
    fn offers_for(&self, agent: Entity) -> Vec<Gift>;

    /// Gives the agent `name` the skill `skill` of `from`, or of the
    /// starters, as the chief's `/give`.
    fn give(
        &mut self,
        name: &str,
        skill: &str,
        from: Option<&str>,
    ) -> Result<(), Failure>;

    /// The file the person dropped on the views of `plugin`, which
    /// was told of it as `id`.
    fn dropped(
        &self,
        plugin: Entity,
        id: u32,
    ) -> Result<PathBuf, Failure>;

    /// Puts a copy of what is at `path` in the inbox of the agent
    /// `name`, and tells it so in the chief's conversation with it.
    fn hand(
        &mut self,
        name: &str,
        path: &Path,
    ) -> Result<(), Failure>;
}

impl GiftsWorldExt for World {
    fn receiver(
        &mut self,
        name: &str,
    ) -> Result<(Entity, String, Hamlet), Failure> {
        let agent =
            agents::named(self, name).ok_or(Failure::NotFound)?;
        let hamlet = self
            .get::<LivesIn>(agent)
            .and_then(|lives| self.get::<Settlement>(lives.0))
            .ok_or(Failure::NotFound)?
            .record();
        Ok((agent, agents::name_of(self, agent), hamlet))
    }

    fn offers_for(&self, agent: Entity) -> Vec<Gift> {
        let offers = self
            .get::<LivesIn>(agent)
            .and_then(|lives| self.get::<Settlement>(lives.0))
            .map(|place| {
                place.record().offers(&agents::name_of(self, agent))
            });
        offers
            .unwrap_or_default()
            .into_iter()
            .map(|offer| Gift {
                skill: offer.skill,
                from: offer.from,
            })
            .collect()
    }

    fn give(
        &mut self,
        name: &str,
        skill: &str,
        from: Option<&str>,
    ) -> Result<(), Failure> {
        let (agent, name, hamlet) = self.receiver(name)?;
        // Each is one word of the command.
        let word = |text: &str| {
            !text.is_empty() && !text.contains(char::is_whitespace)
        };
        if !word(skill) || from.is_some_and(|from| !word(from)) {
            return Err(Failure::Refused(
                "a skill and its giver are one word each".into(),
            ));
        }
        let text = match from {
            Some(from) => format!("/give {name} {skill} from {from}"),
            None => format!("/give {name} {skill}"),
        };
        let place = agents::hamlet_of(self, agent)
            .unwrap_or(HOME)
            .to_string();
        self.decree(&place, text)?;
        let given = hamlet
            .house(&name)
            .is_ok_and(|house| house.skills().join(skill).is_dir());
        if !given {
            return Err(Failure::Failed(format!(
                "{name} was not given {skill}"
            )));
        }
        Ok(())
    }

    fn dropped(
        &self,
        plugin: Entity,
        id: u32,
    ) -> Result<PathBuf, Failure> {
        self.get_resource::<DroppedFiles>()
            .and_then(|files| files.get(plugin, id))
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                Failure::Refused(
                    "no such file was dropped on its views".into(),
                )
            })
    }

    fn hand(
        &mut self,
        name: &str,
        path: &Path,
    ) -> Result<(), Failure> {
        let (_, name, hamlet) = self.receiver(name)?;
        if !path.is_absolute() {
            return Err(Failure::Refused(
                "the path is not absolute".into(),
            ));
        }
        // The home holds the keys and the agents' houses.
        let home = &self.resource::<HomeDir>().0.root;
        let inside = |at: &Path| {
            let home = home.canonicalize().unwrap_or(home.clone());
            at.canonicalize().is_ok_and(|at| at.starts_with(&home))
        };
        if inside(path) {
            return Err(Failure::Refused(
                "a file of the home is not handed over".into(),
            ));
        }
        let at = hamlet
            .hand(&name, path)
            .map_err(|why| Failure::Failed(format!("{why:#}")))?;
        let key = self.chief_key(&Recipient::Agent(name));
        let user = self.resource::<Chief>().name.clone();
        self.trigger(UserMessage {
            key,
            user,
            text: gifts::letter(&at),
        });
        Ok(())
    }
}
