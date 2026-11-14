//! A hamlet's tier, set by the chief.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U11 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::{
    Done, HamletId, SetTier, Tier, TierRefusal,
};
use hamlet_messages::sender::Client;

use super::super::act::{Act, Decision};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer};

/// What a tier change decides over: the hamlet and its living hires.
pub(in crate::realm) struct Tiering {
    hamlet: Option<super::Hamlet>,
    living: u32,
}

impl Act<Client> for SetTier {
    type Output = Done<TierRefusal>;
    type Loaded = Tiering;
    type Done = ();

    async fn load(
        &self,
        by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Tiering> {
        todo!()
    }

    fn decide(
        &self,
        by: &Client,
        tiering: Tiering,
    ) -> Decision<(), Self::Output> {
        todo!()
    }

    fn answer(&self, (): (), made: &Made) -> Self::Output {
        todo!()
    }
}

/// `chiefs::apply`'s arm for `Change::Tier`.
pub(in crate::realm) async fn set(
    writer: &Writer<'_>,
    hamlet: HamletId,
    tier: Tier,
) -> Result<Affected> {
    todo!()
}
