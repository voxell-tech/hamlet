//! A chief's acts on a hamlet: its name, its site, its mode, and its
//! folder with the `hamlet_folder` table of the folders it had.

use anyhow::{Result, bail};
use hamlet_messages::realm::hamlets::{
    MoveHamlet, NameHamlet, SetFolder, SetMode,
};
use hamlet_messages::realm::{Done, HamletRefusal};
use hamlet_messages::sender::Client;

use super::super::act::{Act, Decision, Unbuilt};
use super::super::calls::Made;
use super::super::change::{Affected, Rows, Writer};

#[derive(Debug, PartialEq)]
pub(in crate::realm) enum Change {}

pub(in crate::realm) async fn apply(
    _writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {}
}

impl Act<Client> for NameHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U9")
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

impl Act<Client> for MoveHamlet {
    type Output = Done<HamletRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U9")
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

impl Act<Client> for SetMode {
    type Output = Done<HamletRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U9")
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

impl Act<Client> for SetFolder {
    type Output = Done<HamletRefusal>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &Client,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U9")
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
