//! The measure of a tell's or an ask's files under the sender's
//! reach, before the call runs.

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::realm::{
    AgentRefusal, Ask, Enclosed, LetterRefusal, Refusable, Tell, Tool,
};
use hamlet_messages::sender::sender;
use hamlet_messages::workspace::{
    self, AgentNumber, ParcelAnswer, ParcelRefusal,
};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

use super::super::act::{self, ToolAct};
use super::super::change::Rows;
use super::super::peer::When;
use super::super::{Realm, callers, marker};
use super::told;

/// A tell or an ask whose files the workspace measures under the
/// sender's reach once nothing else refuses it and before it runs, so
/// that files it may not send refuse the call. A repeat answers as
/// the first call did.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(in crate::realm) struct Measuring<M>(pub M);

impl<M: Message> Action for Measuring<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Measuring<M> {
    type To = M::To;
    type From = M::From;
}

/// A society tool that sends the files under its paths.
pub(in crate::realm) trait Encloses {
    fn paths(&self) -> &[Enclosed];
}

impl Encloses for Tell {
    fn paths(&self) -> &[Enclosed] {
        &self.paths
    }
}

impl Encloses for Ask {
    fn paths(&self) -> &[Enclosed] {
        &self.paths
    }
}

impl<T> Handler<Measuring<Tool<T>>> for Realm
where
    T: ToolAct<Agent> + Encloses,
    T::Answer: Refusable<Why = AgentRefusal<LetterRefusal>>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        Measuring(tool): Measuring<Tool<T>>,
    ) -> Result<T::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &tool)?;
        let (key, called, tool) = act::called(&by, name, tool)?;
        let paths = tool.paths().to_vec();
        let run = tool.run(&called);
        let rows = Rows::outside(&ctx)?;
        let repeat =
            marker::find::<IgnoredAny>(&rows, &key).await?.is_some();
        if !repeat
            && act::goes(&rows, &by, T::TOOL, &called, &run).await?
            && let Some(why) = unsendable(&self, by.0, &paths).await
        {
            return Ok(T::Answer::refused(AgentRefusal::Act(
                LetterRefusal::Files { words: told(&why) },
            )));
        }
        let ran = act::run_called(
            &self,
            &ctx,
            by,
            (key, called),
            name,
            T::TOOL,
            run,
        )
        .await?;
        Ok(callers::to_tool(ran))
    }
}

/// Why the workspace refuses to measure the files under `paths` for
/// `sender`. A measure that fails is left to the parcel's own step.
async fn unsendable(
    realm: &Realm,
    sender: AgentNumber,
    paths: &[Enclosed],
) -> Option<ParcelRefusal> {
    if paths.is_empty() {
        return None;
    }
    let asked = workspace::Parcel {
        agent: sender,
        paths: paths.to_vec(),
    };
    match realm
        .peers
        .call(&Workspace, &asked, When::BeforeAnswer)
        .await
    {
        Ok(ParcelAnswer::Refused { why }) => Some(why),
        _ => None,
    }
}
