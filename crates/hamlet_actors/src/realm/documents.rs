//! The chief's documents, the charter and the catalogue: the
//! `document` table, their edits, and the copies agents read.

use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::copies::Copies;
use hamlet_messages::realm::Done;
use hamlet_messages::realm::documents::{
    CatalogueRefusal, CharterRead, EditCatalogue, EditCharter,
    EditRefusal, ReadCharter, ReadCopies,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::Realm;
use super::act::{Act, Decision, Unbuilt};
use super::calls::Made;
use super::change::{Affected, Rows, Writer};

/// A part of the copies an agent reads.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Part {
    Charter,
    Character,
    Model,
    Settings,
}

/// The agents a moved part of the copies goes to.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub(super) enum Readers {
    Living,
    Agent(AgentNumber),
}

/// A part of the copies a commit moved, with its readers.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
pub(super) struct Copied {
    pub part: Part,
    pub readers: Readers,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {}

pub(super) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

/// Pushes `copied`'s part, as it is now, to its readers.
pub(super) async fn send(
    _realm: &Arc<Realm>,
    _ctx: &Ctx<Realm>,
    _copied: Copied,
) -> Result<()> {
    bail!("not built yet: U10")
}

impl Act<Client> for EditCharter {
    type Output = Done<EditRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U10")
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl Act<Client> for EditCatalogue {
    type Output = Done<CatalogueRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U10")
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl Act<Client> for ReadCharter {
    type Output = CharterRead;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U10")
    }

    fn decide(
        &self,
        _by: &Client,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl Act<Agent> for ReadCopies {
    type Output = Copies;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Agent,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U10")
    }

    fn decide(
        &self,
        _by: &Agent,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
