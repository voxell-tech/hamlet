//! What a command's words name: an agent or a profile by its address,
//! a profile to add an agent of at a version, and the chief's right
//! over a hamlet, an agent or an agent's model.

use hamlet_git::Repository;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ProfileId,
};

use crate::core::Core;
use crate::realm::change::Done;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind};
use crate::society::hamlet_agents::ProfileChoice;
use crate::society::refusal::Refusal;
use crate::society::{authority, city, hamlets};

/// The profiles an address may name.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ProfileReach {
    Living,
    /// The archived too.
    Kept,
}

/// The check that `word` is a profile's address, with or without a
/// version.
pub(super) fn is_profile(word: &str) -> bool {
    word.starts_with("profile#")
}

impl Core<'_, '_> {
    /// The profile `text` addresses: a living one of the chief's, at
    /// its `main` or pinned to `@version`.
    pub(super) fn profile_choice(
        &mut self,
        text: &str,
        chief: ChiefId,
    ) -> Done<ProfileChoice> {
        let (profile, version) = match self.profile_pinned(text)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        if city::get(db, profile)?.chief != Some(chief) {
            return Ok(Err(Refusal::ProfileNotOwn {
                name: addresses::profile_name_of(db, profile)?,
            }));
        }
        self.existing(profile, version)
    }

    /// The living profile `text` addresses, `profile#<id>` with
    /// `@<version>` to pin one, and that version.
    pub(crate) fn profile_pinned(
        &mut self,
        text: &str,
    ) -> Done<(ProfileId, Option<String>)> {
        let (address, version) = match text.split_once('@') {
            Some((address, version)) => {
                (address, Some(version.to_owned()))
            }
            None => (text, None),
        };
        Ok(self
            .profile_at(address, ProfileReach::Living)?
            .map(|profile| (profile, version)))
    }

    /// An agent of the living `profile`, at its `main` or pinned to
    /// `version` when that is one the profile has.
    pub(crate) fn existing(
        &mut self,
        profile: ProfileId,
        version: Option<String>,
    ) -> Done<ProfileChoice> {
        if let Some(version) = &version {
            let found = Repository::open(
                &self.realm.layout().repository(profile),
            )
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
            if found.is_none() || version == hamlet_git::MAIN {
                let db = self.realm.store().connection();
                return Ok(Err(Refusal::NoVersion {
                    name: addresses::profile_name_of(db, profile)?,
                    version: version.clone(),
                }));
            }
        }
        Ok(Ok(ProfileChoice::Existing {
            profile,
            pin: version,
        }))
    }

    /// The agent `text` addresses, said in `here`.
    pub(super) fn resolve(
        &mut self,
        text: &str,
        here: Option<&HamletId>,
        reach: Reach,
    ) -> Done<AgentId> {
        let address = match addresses::parse(text) {
            Ok(address) => address,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        addresses::resolve(db, &address, here, reach)
    }

    /// The profile `text` addresses, among those `reach` allows.
    pub(super) fn profile_at(
        &mut self,
        text: &str,
        reach: ProfileReach,
    ) -> Done<ProfileId> {
        let id = match addresses::parse_profile(text) {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let found = city::find(db, id)?.filter(|found| {
            found.kind == city::ProfileKind::Profile
                && (reach == ProfileReach::Kept || found.is_living())
        });
        Ok(match found {
            Some(found) => Ok(found.id),
            None => Err(Refusal::NoProfile {
                address: text.into(),
            }),
        })
    }

    /// Refuses unless this server's chief is a chief of `hamlet`.
    pub(super) fn chief_of(&mut self, hamlet: &HamletId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if hamlets::find(db, hamlet)?.is_none() {
            return Ok(Err(Refusal::NoHamlet {
                id: hamlet.to_string(),
            }));
        }
        if !authority::is_chief_of(db, chief, hamlet)? {
            return Ok(Err(Refusal::NotChief {
                hamlet: hamlet.to_string(),
            }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief may direct the agent of a
    /// hamlet `agent`.
    pub(crate) fn directs(&mut self, agent: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if agents::get(db, agent)?.kind != AgentKind::Hamlet {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Refusal::NotHamletAgent { name }));
        }
        if !authority::may_direct(db, Actor::Chief(chief), agent)? {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief may choose the model,
    /// effort and fast tier of `agent`.
    pub(super) fn steers(&mut self, agent: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if !authority::may_steer(db, Actor::Chief(chief), agent)? {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Refusal::NotYours { name }));
        }
        Ok(Ok(()))
    }

    pub(super) fn no_hamlet(&self) -> Refusal {
        Refusal::SayToLeadOrAgent
    }
}
