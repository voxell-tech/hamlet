//! The files of a parcel, measured and read for the realm under the
//! sending agent's reach.

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::{Ctx, Handler};
use hamlet_messages::address::Realm;
use hamlet_messages::realm::parcels::Part;
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::{
    Parcel, ParcelAnswer, ParcelPart, ParcelRefusal,
};

use super::Workspace;

impl Handler<Parcel> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        parcel: Parcel,
    ) -> Result<ParcelAnswer> {
        let Realm = sender(&ctx, &parcel)?;
        bail!("not built yet: U13")
    }
}

impl Handler<ParcelPart> for Workspace {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        part: ParcelPart,
    ) -> Result<Part<ParcelRefusal>> {
        let Realm = sender(&ctx, &part)?;
        bail!("not built yet: U13")
    }
}
