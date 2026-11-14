//! The realm's secrets as the workspace reads them: version 1, with
//! no value.

#![expect(
    unused_variables,
    reason = "the scaffold: U8 fills this file"
)]

use hamlet_messages::address::Workspace;
use hamlet_messages::realm::{SecretList, Secrets};

use super::act::Look;
use super::world::World;

impl Look<Workspace> for Secrets {
    type Output = SecretList;

    fn look(&self, by: &Workspace, world: &World) -> SecretList {
        todo!()
    }
}
