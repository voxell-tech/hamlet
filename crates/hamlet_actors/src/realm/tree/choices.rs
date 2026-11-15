//! The choices a lead or a chief makes for an agent: the `nickname`
//! and `model_choice` tables, `SetNickname` and `SetModel`.

use anyhow::{Result, bail};
use hamlet_messages::realm::Done;
use hamlet_messages::realm::choices::{
    ChoiceRefusal, SetModel, SetNickname,
};

use super::super::act::{Act, Decision, Unbuilt};
use super::super::callers::Lead;
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

impl<By: Lead> Act<By> for SetNickname {
    type Output = Done<By::Why<ChoiceRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &By,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U9")
    }

    fn decide(
        &self,
        _by: &By,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}

impl<By: Lead> Act<By> for SetModel {
    type Output = Done<By::Why<ChoiceRefusal>>;
    type Loaded = Unbuilt;
    type Done = Unbuilt;

    async fn load(
        &self,
        _by: &By,
        _rows: &Rows<'_>,
    ) -> Result<Unbuilt> {
        bail!("not built yet: U9")
    }

    fn decide(
        &self,
        _by: &By,
        loaded: Unbuilt,
    ) -> Decision<Unbuilt, Self::Output> {
        match loaded {}
    }

    fn answer(&self, done: Unbuilt, _made: &Made) -> Self::Output {
        match done {}
    }
}
