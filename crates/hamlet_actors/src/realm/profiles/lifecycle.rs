//! A chief's acts on a profile's life: its fork, its archive, its
//! return and its erasure, and its place in the city.

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::realm::Asked;
use hamlet_messages::realm::profiles::{
    ArchiveProfile, EraseProfile, ForkProfile, Forked, Lifecycle,
    MoveHouse, RestoreProfile, TurnAway, Welcome,
};
use hamlet_messages::sender::Client;
use serde::{Deserialize, Serialize};

use super::super::Realm;
use super::super::act::unbuilt;
use super::super::calls::Calls;
use super::super::change::{self, Rows};
use super::super::steps::{Cx, Steps};

/// A fork, its version's commit found before the realm decides it.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::realm) struct Forking<M>(pub M);

impl<M: Message> Action for Forking<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Forking<M> {
    type To = M::To;
    type From = M::From;
}

impl Handler<Forking<Asked<ForkProfile>>> for Realm {
    async fn handle(
        self: Arc<Self>,
        _ctx: Ctx<Self>,
        _forking: Forking<Asked<ForkProfile>>,
    ) -> Result<Forked> {
        bail!("not implemented")
    }
}

unbuilt!(Client:
    ArchiveProfile => Lifecycle,
    RestoreProfile => Lifecycle,
    EraseProfile => Lifecycle,
    Welcome => Lifecycle,
    TurnAway => Lifecycle,
    MoveHouse => Lifecycle,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) enum ForkStep {
    Create,
    Live,
}

impl Steps for ForkStep {
    const ALL: &'static [Self] = &[Self::Create, Self::Live];

    fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Live => "live",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::realm) enum EraseStep {
    Hires,
    Profile,
}

impl Steps for EraseStep {
    const ALL: &'static [Self] = &[Self::Hires, Self::Profile];

    fn name(self) -> &'static str {
        match self {
            Self::Hires => "hires",
            Self::Profile => "profile",
        }
    }
}

pub(in crate::realm) async fn fork(
    _cx: &Cx<'_>,
    _n: u64,
    _step: ForkStep,
) -> Result<Vec<change::Change>> {
    bail!("not implemented")
}

pub(in crate::realm) async fn fork_failed(
    _rows: &Rows<'_>,
    _n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    bail!("not implemented")
}

pub(in crate::realm) async fn erase(
    _cx: &Cx<'_>,
    _n: u64,
    _step: EraseStep,
) -> Result<Vec<change::Change>> {
    bail!("not implemented")
}

pub(in crate::realm) async fn erase_failed(
    _rows: &Rows<'_>,
    _n: u64,
) -> Result<(Vec<change::Change>, Calls)> {
    bail!("not implemented")
}
