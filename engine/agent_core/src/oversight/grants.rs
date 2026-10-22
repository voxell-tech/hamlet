//! The permissions chiefs have given agents by answering "always"
//! (H11, H22), kept in `grants.jsonl` by the agent's number.

use std::path::{Path, PathBuf};

use anyhow::Result;
use bevy_ecs::prelude::Resource;
use hamlet_home::limits::{Permission, Permissions};
use hamlet_model::log;
use serde_json::{Value, json};

use super::danger::Danger;

/// The permission that would let a call with `danger` run. `None` for
/// a danger that is asked about every time, or allowed for a session
/// at most.
pub(crate) fn lifting(danger: Danger) -> Option<Permission> {
    match danger {
        Danger::Network => Some(Permission::Network),
        Danger::Unconfined => Some(Permission::Unconfined),
        Danger::Destructive
        | Danger::Reserved
        | Danger::Edit
        | Danger::Command => None,
    }
}

/// A permission a chief gave an agent for good.
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    /// The agent's number in the hamlet's record. A later agent of
    /// the same name has another, and does not inherit it.
    pub agent: u64,
    /// The agent's name when it was given.
    pub name: String,
    pub permission: Permission,
    /// The chief who gave it.
    pub by: String,
}

/// The grants in force after `events`.
fn fold(events: &[Value]) -> Vec<Grant> {
    let mut held = Vec::<Grant>::new();
    for e in events {
        let (Some(agent), Some(permission)) = (
            e["agent"].as_u64(),
            e["permission"].as_str().and_then(Permission::named),
        ) else {
            continue;
        };
        held.retain(|g| {
            g.agent != agent || g.permission != permission
        });
        if e["type"] == "granted" {
            held.push(Grant {
                agent,
                name: e["name"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                permission,
                by: e["by"].as_str().unwrap_or_default().to_string(),
            });
        }
    }
    held
}

/// The grants in force, and the record they are read from.
#[derive(Resource)]
pub struct Grants {
    path: PathBuf,
    held: Vec<Grant>,
}

impl Grants {
    /// The grants in the record of the home at `root`.
    pub fn open(root: &Path) -> Self {
        let path = root.join("grants.jsonl");
        let held = fold(&log::read_events(&path).unwrap_or_default());
        Grants { path, held }
    }

    pub fn held(&self) -> &[Grant] {
        &self.held
    }

    /// True when the agent numbered `agent` was granted `permission`.
    pub fn holds(&self, agent: u64, permission: Permission) -> bool {
        self.held
            .iter()
            .any(|g| g.agent == agent && g.permission == permission)
    }

    fn append(&mut self, event: Value) -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        // One write, so two sessions cannot mix their lines.
        file.write_all(format!("{event}\n").as_bytes())?;
        self.held =
            fold(&log::read_events(&self.path).unwrap_or_default());
        Ok(())
    }

    /// Gives the agent numbered `agent` the `permission`, in the name
    /// of the chief `by`.
    pub fn give(
        &mut self,
        agent: u64,
        name: &str,
        permission: Permission,
        by: &str,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "granted",
            "ts": log::now_millis(),
            "agent": agent,
            "name": name,
            "permission": permission.name(),
            "by": by,
        }))
    }

    /// Takes `permission` back. False when the agent did not hold
    /// it.
    pub fn revoke(
        &mut self,
        agent: u64,
        permission: Permission,
        by: &str,
    ) -> Result<bool> {
        let held = self.holds(agent, permission);
        if held {
            self.append(json!({
                "v": 1,
                "type": "revoked",
                "ts": log::now_millis(),
                "agent": agent,
                "permission": permission.name(),
                "by": by,
            }))?;
        }
        Ok(held)
    }
}

/// True when the agent `name`, numbered `agent`, may use
/// `permission`, by the chief's limits or by a grant.
pub(crate) fn holds(
    limits: &Permissions,
    grants: &Grants,
    name: &str,
    agent: Option<u64>,
    permission: Permission,
) -> bool {
    permission.standing(limits).iter().any(|n| n == name)
        || agent.is_some_and(|agent| grants.holds(agent, permission))
}

#[cfg(test)]
mod tests {
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn a_grant_is_kept_until_it_is_revoked() {
        let dir = Scratch::new("grants-keep");
        let root = &*dir;
        let mut grants = Grants::open(root);
        assert!(!grants.holds(7, Permission::Network));
        grants
            .give(7, "scout", Permission::Network, "alice")
            .unwrap();
        assert!(grants.holds(7, Permission::Network));
        assert!(!grants.holds(7, Permission::Unconfined));
        assert!(!grants.holds(3, Permission::Network));
        // It is read back from the record.
        assert!(Grants::open(root).holds(7, Permission::Network));
        assert!(
            grants.revoke(7, Permission::Network, "alice").unwrap()
        );
        assert!(!Grants::open(root).holds(7, Permission::Network));
        assert!(
            !grants.revoke(7, Permission::Network, "alice").unwrap()
        );
    }

    #[test]
    fn a_new_agent_of_the_same_name_does_not_inherit_a_grant() {
        let dir = Scratch::new("grants-number");
        let mut grants = Grants::open(&dir);
        grants
            .give(7, "scout", Permission::Network, "alice")
            .unwrap();
        let none = Permissions::default();
        assert!(holds(
            &none,
            &grants,
            "scout",
            Some(7),
            Permission::Network
        ));
        // The scout that came after has another number.
        assert!(!holds(
            &none,
            &grants,
            "scout",
            Some(9),
            Permission::Network
        ));
    }
}
