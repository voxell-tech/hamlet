//! The realm's secrets as the workspace reads them, version 1 with
//! no value, and the providers' credentials an agent asks for and
//! keeps.

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::realm::models::{
    Credential, CredentialAnswer, CredentialRefusal, KeepCredential,
};
use hamlet_messages::realm::{SecretList, Secrets};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::reach::SecretsVersion;

use super::Realm;
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

impl Look<Agent> for Credential {
    type Output = CredentialAnswer;

    fn look(&self, _by: &Agent, _world: &World) -> CredentialAnswer {
        CredentialAnswer::Refused {
            why: CredentialRefusal::NotSignedIn {
                provider: self.provider.clone(),
            },
        }
    }
}

/// Writes the file before it commits, so it is the one handler
/// outside `Act`.
impl Handler<KeepCredential> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        keep: KeepCredential,
    ) -> Result<()> {
        let Agent(_) = sender(&ctx, &keep)?;
        bail!("not built yet: U12")
    }
}
