//! The realm's secrets as the workspace reads them: version 1, with
//! no value.

use hamlet_messages::address::Workspace;
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::workspace::reach::SecretsVersion;

use super::act::Look;
use super::world::World;

pub(super) const VERSION: SecretsVersion = SecretsVersion(1);

impl Look<Workspace> for Secrets {
    type Output = SecretList;

    fn look(&self, _by: &Workspace, _world: &World) -> SecretList {
        SecretList {
            version: VERSION,
            values: Vec::new(),
        }
    }
}
