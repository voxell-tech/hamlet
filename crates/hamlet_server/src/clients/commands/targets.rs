//! What a command's words name, and the chief's right over it.

use hamlet_store::ids::{Actor, AgentId, ChiefId, HamletId};

use crate::core::Core;
use crate::knowledge::versions;
use crate::realm::change::{Done, checked, done, reject};
use crate::society::addresses::{self, Reach};
use crate::society::agents::{self, AgentKind};
use crate::society::hamlet_agents::ProfileChoice;
use crate::society::rejection::Rejection;
use crate::society::{authority, city, hamlets};

pub(super) fn is_profile(word: &str) -> bool {
    word.starts_with("profile#")
}

impl Core<'_, '_> {
    /// The living profile of `chief`'s that `text` addresses, at its
    /// `main` or pinned to `@version`.
    pub(super) fn profile_choice(
        &mut self,
        text: &str,
        chief: ChiefId,
    ) -> Done<ProfileChoice> {
        let layout = self.realm.layout().clone();
        let db = self.realm.store().connection();
        let (profile, version) =
            done!(addresses::profile_pinned(db, text));
        if city::get(db, profile)?.chief != Some(chief) {
            reject!(Rejection::ProfileNotOwn {
                name: addresses::profile_name_of(db, profile)?,
            });
        }
        versions::existing(db, &layout, profile, version)
    }

    /// The agent `text` addresses, said in `here`.
    pub(super) fn resolve(
        &mut self,
        text: &str,
        here: Option<&HamletId>,
        reach: Reach,
    ) -> Done<AgentId> {
        let address = checked!(addresses::parse(text));
        let db = self.realm.store().connection();
        addresses::resolve(db, &address, here, reach)
    }

    /// Rejects unless this server's chief is a chief of `hamlet`.
    pub(super) fn chief_of(&mut self, hamlet: &HamletId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if hamlets::find(db, hamlet)?.is_none() {
            return Ok(Err(Rejection::NoHamlet {
                id: hamlet.to_string(),
            }));
        }
        if !authority::may_govern(db, Actor::Chief(chief), hamlet)? {
            return Ok(Err(Rejection::NotChief {
                hamlet: hamlet.to_string(),
            }));
        }
        Ok(Ok(()))
    }

    /// Rejects unless this server's chief may direct the agent of a
    /// hamlet `agent`.
    pub(crate) fn directs(&mut self, agent: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if agents::get(db, agent)?.kind != AgentKind::Hamlet {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Rejection::NotHamletAgent { name }));
        }
        if !authority::may_direct(db, Actor::Chief(chief), agent)? {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Rejection::NotYours { name }));
        }
        Ok(Ok(()))
    }

    /// Rejects unless this server's chief may choose the model,
    /// effort and fast tier of `agent`.
    pub(super) fn steers(&mut self, agent: AgentId) -> Done<()> {
        let chief = self.realm.chief();
        let db = self.realm.store().connection();
        if !authority::may_steer(db, Actor::Chief(chief), agent)? {
            let name = addresses::name_of(db, agent)?;
            return Ok(Err(Rejection::NotYours { name }));
        }
        Ok(Ok(()))
    }

    pub(super) fn no_hamlet(&self) -> Rejection {
        Rejection::SayToLeadOrAgent
    }
}
