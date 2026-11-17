//! What a command's words name: an agent or a scholar by its address,
//! a scholar to hire at a version, and the chief's right over a
//! hamlet, a hire or an agent's model.

use hamlet_store::ids::{Actor, AgentId, ChiefId, HamletId};

use crate::core::Core;
use crate::knowledge::versions;
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
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (scholar, version) =
            match addresses::scholar_pinned(db, text)? {
                Ok(found) => found,
                Err(refusal) => return Ok(Err(refusal)),
            };
        if city::get(db, scholar)?.chief != Some(chief) {
            return Ok(Err(Refusal::ScholarNotOwn {
                name: addresses::scholar_name_of(db, scholar)?,
            }));
        }
        versions::existing(db, &layout, scholar, version)
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
