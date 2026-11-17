//! What a command's words name: an agent or a scholar by its address,
//! a scholar to hire at a version, and the chief's right over a
//! hamlet, a hire or an agent's model.

use hamlet_git::Repository;
use hamlet_store::ids::{
    Actor, AgentId, ChiefId, HamletId, ScholarId,
};

use crate::core::Core;
use crate::realm::change::Done;
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind};
use crate::society::hires::Hiring;
use crate::society::refusal::Refusal;
use crate::society::{authority, city, hamlets};

/// The check that `word` is a scholar's address, with or without a
/// version.
pub(super) fn is_scholar(word: &str) -> bool {
    word.starts_with("scholar#")
}

impl Core<'_, '_> {
    /// The scholar `text` addresses: a living one of the chief's, at
    /// its `main` or pinned to `@version`.
    pub(super) fn hiring(
        &mut self,
        text: &str,
        chief: ChiefId,
    ) -> Done<Hiring> {
        let (scholar, version) = match self.scholar_pinned(text)? {
            Ok(found) => found,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        if city::get(db, scholar)?.chief != Some(chief) {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: addresses::scholar_name_of(db, scholar)?,
            }));
        }
        self.existing(scholar, version)
    }

    /// The living scholar `text` addresses, `scholar#<id>` with
    /// `@<version>` to pin one, and that version.
    pub(crate) fn scholar_pinned(
        &mut self,
        text: &str,
    ) -> Done<(ScholarId, Option<String>)> {
        let (address, version) = match text.split_once('@') {
            Some((address, version)) => {
                (address, Some(version.to_owned()))
            }
            None => (text, None),
        };
        Ok(self
            .scholar_at(address, Reach::Living)?
            .map(|scholar| (scholar, version)))
    }

    /// A hire of the living `scholar`, at its `main` or pinned to
    /// `version` when that is one the scholar has.
    pub(crate) fn existing(
        &mut self,
        scholar: ScholarId,
        version: Option<String>,
    ) -> Done<Hiring> {
        if let Some(version) = &version {
            let found = Repository::open(
                &self.realm.layout().repository(scholar),
            )
            .ok()
            .and_then(|repo| repo.resolve(version).ok().flatten());
            if found.is_none() || version == hamlet_git::MAIN {
                let db = self.realm.store().connection();
                return Ok(Err(Refusal::NoVersion {
                    name: addresses::scholar_name_of(db, scholar)?,
                    version: version.clone(),
                }));
            }
        }
        Ok(Ok(Hiring::Existing {
            scholar,
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

    /// The scholar `text` addresses, among those `reach` allows.
    pub(crate) fn scholar_at(
        &mut self,
        text: &str,
        reach: Reach,
    ) -> Done<ScholarId> {
        let id = match addresses::parse_scholar(text) {
            Ok(id) => id,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let db = self.realm.store().connection();
        let found = city::find(db, id)?.filter(|found| {
            found.kind == city::ScholarKind::Scholar
                && (reach == Reach::Kept || found.is_living())
        });
        Ok(match found {
            Some(found) => Ok(found.id),
            None => Err(Refusal::NoScholar {
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
        if !authority::may_govern(db, Actor::Chief(chief), hamlet)? {
            return Ok(Err(Refusal::NotChief {
                hamlet: hamlet.to_string(),
            }));
        }
        Ok(Ok(()))
    }

    /// Refuses unless this server's chief leads the hire `hire`.
    pub(crate) fn leads(&mut self, hire: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if agents::get(db, hire)?.kind != AgentKind::Hire {
            let name = addresses::name_of(db, hire)?;
            return Ok(Err(Refusal::NotHire { name }));
        }
        if !authority::may_lead(db, Actor::Chief(chief), hire)? {
            let name = addresses::name_of(db, hire)?;
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
        Refusal::SayToMayorOrHire
    }
}
