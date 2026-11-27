//! The chiefs' clients: their messages checked, their connections,
//! what a chief's edits carry, and the changes between two states a
//! client is sent.

pub(crate) mod check;
pub(crate) mod connection;
pub(crate) mod documents;

use hamlet_protocol::client::{self as protocol, Update};

/// The changes that take a client from `old` to `new`, or `None` when
/// only a whole state does: a part changed that has no change of its
/// own.
pub(crate) fn changes(
    old: &protocol::State,
    new: &protocol::State,
) -> Option<Vec<Update>> {
    let whole = old.closed != new.closed
        || old.library != new.library
        || old.deeds != new.deeds
        || old.hand != new.hand
        || old.limits != new.limits
        || old.confinement != new.confinement
        || old.realm != new.realm
        || old.chief != new.chief
        || old.hamlets.len() > new.hamlets.len()
        || new
            .hamlets
            .iter()
            .zip(&old.hamlets)
            .any(|(n, o)| n.id != o.id);
    if whole {
        return None;
    }
    let mut out = Vec::new();
    for hamlet in &new.hamlets {
        if old.hamlets.iter().find(|h| h.id == hamlet.id)
            != Some(hamlet)
        {
            out.push(Update::Hamlet {
                hamlet: hamlet.clone(),
            });
        }
    }
    for agent in &new.agents {
        if old.agents.iter().find(|a| a.id == agent.id) != Some(agent)
        {
            out.push(Update::Agent {
                agent: agent.clone(),
            });
        }
    }
    for gone in &old.agents {
        if !new.agents.iter().any(|a| a.id == gone.id) {
            out.push(Update::AgentGone { id: gone.id });
        }
    }
    for profile in &new.profiles {
        if old.profiles.iter().find(|s| s.id == profile.id)
            != Some(profile)
        {
            out.push(Update::Profile {
                profile: profile.clone(),
            });
        }
    }
    for gone in &old.profiles {
        if !new.profiles.iter().any(|s| s.id == gone.id) {
            out.push(Update::ProfileGone { id: gone.id });
        }
    }
    for wait in &new.waits {
        if old.waits.iter().find(|w| w.id == wait.id) != Some(wait) {
            out.push(Update::Wait { wait: wait.clone() });
        }
    }
    for gone in &old.waits {
        if !new.waits.iter().any(|w| w.id == gone.id) {
            out.push(Update::WaitClosed {
                how: "settled".into(),
                id: gone.id.clone(),
            });
        }
    }
    if old.grants != new.grants {
        out.push(Update::Grants {
            grants: new.grants.clone(),
        });
    }
    if old.settings != new.settings {
        out.push(Update::Settings {
            settings: new.settings.clone(),
        });
    }
    if old.providers != new.providers {
        out.push(Update::Providers {
            providers: new.providers.clone(),
        });
    }
    Some(out)
}
