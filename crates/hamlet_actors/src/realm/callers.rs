use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_host::Caller;
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Party, Profile, Workspace};
use hamlet_messages::realm::budget::{OpenTask, TaskOpened};
use hamlet_messages::realm::client::{Answer, Say};
use hamlet_messages::realm::{
    Asked, ChiefRequest, Gate, LeadRefusal, ProfileId, Rank,
    Refusable, SecretList, Secrets, Standing, Tool, ToolCall,
};
use hamlet_messages::sender::{
    Client, Hand, Scrubber, Sender, sender, wrong_caller,
};
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{self, Act, Look, ToolAct};
use super::change::Rows;
use super::peer::When;
use super::tree::{self, Life, Lineage};
use super::workers::errands;
use super::{Realm, marker, record};

/// A sender the realm takes calls from.
pub(super) trait Witness:
    Sender + Send + Sync + 'static
{
    /// The sender as the `marker` table names it: its address.
    fn name(&self) -> String;
}

impl Witness for Client {
    fn name(&self) -> String {
        "client".to_owned()
    }
}

impl Witness for Agent {
    fn name(&self) -> String {
        format!("agent/{}", self.0.0)
    }
}

impl Witness for Hand {
    fn name(&self) -> String {
        Agent(Self::NUMBER).name()
    }
}

impl Witness for Workspace {
    fn name(&self) -> String {
        "workspace/local".to_owned()
    }
}

impl Witness for Scrubber {
    fn name(&self) -> String {
        match self {
            Self::Workspace => Workspace.name(),
            Self::Agent(agent) => agent.name(),
        }
    }
}

#[derive(Debug)]
pub(super) enum Stood {
    Standing(Standing),
    Gate(Gate),
}

/// The reason `agent` may not act: an errand's standing, or how it
/// left the tree.
pub(super) async fn standing(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Stood>> {
    if let Some(standing) = errands::standing(rows, agent).await? {
        return Ok(Some(Stood::Standing(standing)));
    }
    Ok(tree::left(rows, agent).await?.map(|left| {
        Stood::Standing(Standing::YouLeft {
            how: left.how,
            why: left.why,
        })
    }))
}

/// A sender whose decisions the record names (H17).
pub(super) trait Decider: Witness {
    fn by(&self) -> record::By;
}

impl Decider for Client {
    fn by(&self) -> record::By {
        record::By::Chief
    }
}

impl Decider for Agent {
    fn by(&self) -> record::By {
        record::By::Agent(self.0)
    }
}

impl Decider for Hand {
    fn by(&self) -> record::By {
        record::By::Agent(Self::NUMBER)
    }
}

/// A sender whose powers reach every hamlet.
pub(super) trait RealmWide: Decider {}

impl RealmWide for Client {}

impl RealmWide for Hand {}

/// A sender that can have a lead's powers over a hire (H41).
pub(super) trait Lead: Decider {
    /// A refusal `R` as this kind of sender is answered with it.
    type Why<R>: Send + 'static
    where
        R: Send + 'static;

    fn leads<R: Send + 'static>(
        &self,
        lineage: &Lineage,
        hire: AgentNumber,
    ) -> Result<(), Self::Why<R>>;

    fn refused<R: Send + 'static>(refusal: R) -> Self::Why<R>;
}

impl Lead for Client {
    type Why<R>
        = R
    where
        R: Send + 'static;

    fn leads<R: Send + 'static>(
        &self,
        _: &Lineage,
        _: AgentNumber,
    ) -> Result<(), R> {
        Ok(())
    }

    fn refused<R: Send + 'static>(refusal: R) -> R {
        refusal
    }
}

impl Lead for Agent {
    type Why<R>
        = LeadRefusal<R>
    where
        R: Send + 'static;

    fn leads<R: Send + 'static>(
        &self,
        lineage: &Lineage,
        hire: AgentNumber,
    ) -> Result<(), LeadRefusal<R>> {
        if lineage.is_led_by(self.0) {
            Ok(())
        } else {
            Err(LeadRefusal::Rank(Rank::NotALead { hire }))
        }
    }

    fn refused<R: Send + 'static>(refusal: R) -> LeadRefusal<R> {
        LeadRefusal::Act(refusal)
    }
}

/// A society tool only the Hand holds.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct ByHand<M>(pub M);

impl<M: Message> Action for ByHand<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for ByHand<M> {
    type To = M::To;
    type From = Hand;
}

/// The message `M`, with no id of its own for a repeat.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Bare<M>(pub M);

impl<M: Message> Action for Bare<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Bare<M> {
    type To = M::To;
    type From = M::From;
}

/// The message `M`, answered from the copy in memory.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Looks<M>(pub M);

impl<M: Message> Action for Looks<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Looks<M> {
    type To = M::To;
    type From = M::From;
}

/// The message `M`, answered from a profile's answer.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Relays<M>(pub M);

impl<M: Message> Action for Relays<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Relays<M> {
    type To = M::To;
    type From = M::From;
}

/// A message the realm answers from the answer of the profile it
/// names, changing nothing of its own.
pub(super) trait Relay<By>:
    Send + Sync + Sized + 'static
{
    type Call: Message<To = Profile>;
    type Output: Send + 'static;

    /// The profile and what it is asked, or the answer when the realm
    /// refuses.
    fn ask(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> impl Future<
        Output = Result<
            Result<(ProfileId, Self::Call), Self::Output>,
        >,
    > + Send;

    fn answer(answer: <Self::Call as Action>::Output)
    -> Self::Output;
}

/// The message `M`, answered only to a caller that is living.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Living<M>(pub M);

impl<M: Message> Action for Living<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Living<M> {
    type To = M::To;
    type From = M::From;
}

/// Refuses `agent`'s call of `action` unless its row is one of
/// `admitted`.
pub(super) async fn admit(
    ctx: &Ctx<Realm>,
    action: &'static str,
    agent: Agent,
    admitted: &[Life],
) -> Result<()> {
    let life = tree::life(&Rows::outside(ctx)?, agent.0).await?;
    if admitted.contains(&life) {
        return Ok(());
    }
    refuse(action, agent)
}

/// Refuses `agent`'s call of `action` as a wrong caller.
pub(super) fn refuse<T>(
    action: &'static str,
    agent: Agent,
) -> Result<T> {
    Err(wrong_caller(action, Some(Caller::Actor(agent.address()?))))
}

impl<M> Handler<Living<M>> for Realm
where
    M: Message<From = Agent>,
    Realm: Handler<M>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        living: Living<M>,
    ) -> Result<<M as Action>::Output> {
        let by = sender(&ctx, &living)?;
        admit(&ctx, M::NAME, by, &[Life::Living]).await?;
        <Self as Handler<M>>::handle(self, ctx, living.0).await
    }
}

impl Handler<Secrets> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        secrets: Secrets,
    ) -> Result<SecretList> {
        let by = sender(&ctx, &secrets)?;
        if let Scrubber::Agent(agent) = by {
            let rowed = [Life::Living, Life::Ended];
            admit(&ctx, Secrets::NAME, agent, &rowed).await?;
        }
        Ok(act::look(&self, &by, &secrets))
    }
}

impl Handler<Say> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        say: Say,
    ) -> Result<<Say as Action>::Output> {
        let by = sender(&ctx, &say)?;
        let id = marker::Id::Request(say.0.id.clone());
        act::run(&self, &ctx, by, Some(id), Say::NAME, say).await
    }
}

impl Handler<Answer> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        answer: Answer,
    ) -> Result<<Answer as Action>::Output> {
        let by = sender(&ctx, &answer)?;
        let id = marker::Id::Request(answer.0.id.clone());
        act::run(&self, &ctx, by, Some(id), Answer::NAME, answer)
            .await
    }
}

impl Handler<OpenTask> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        open: OpenTask,
    ) -> Result<TaskOpened> {
        let by = sender(&ctx, &open)?;
        let id = marker::Id::Request(open.request.clone());
        act::run(&self, &ctx, by, Some(id), OpenTask::NAME, open)
            .await
    }
}

impl<T> Handler<Asked<T>> for Realm
where
    T: ChiefRequest
        + Act<Client, Output = <T as ChiefRequest>::Answer>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        asked: Asked<T>,
    ) -> Result<<T as ChiefRequest>::Answer> {
        let name = <Asked<T> as Action>::NAME;
        let by = sender(&ctx, &asked)?;
        let id = marker::Id::Request(asked.id);
        act::run(&self, &ctx, by, Some(id), name, asked.act).await
    }
}

impl<T> Handler<Tool<T>> for Realm
where
    T: ToolAct,
    <T::Answer as Refusable>::Why: From<Standing> + From<Gate>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        tool: Tool<T>,
    ) -> Result<<T as ToolCall>::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &tool)?;
        let ran = act::run_tool(&self, &ctx, by, name, tool).await?;
        Ok(to_tool(ran))
    }
}

impl<T> Handler<ByHand<Tool<T>>> for Realm
where
    T: ToolCall + Act<Hand, Output = <T as ToolCall>::Answer>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        hand: ByHand<Tool<T>>,
    ) -> Result<<T as ToolCall>::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &hand)?;
        let ByHand(tool) = hand;
        let id = marker::Id::Call(tool.call.clone());
        let key = marker::Key::of(&by, id, name, &tool)?;
        act::run_keyed(&self, &ctx, by, Some(key), name, tool.act)
            .await
    }
}

fn to_tool<O>(ran: Result<O, Stood>) -> O
where
    O: Refusable,
    O::Why: From<Standing> + From<Gate>,
{
    ran.unwrap_or_else(|stood| match stood {
        Stood::Standing(why) => O::refused(why.into()),
        Stood::Gate(why) => O::refused(why.into()),
    })
}

impl<M> Handler<Bare<M>> for Realm
where
    M: Message
        + Act<<M as Message>::From, Output = <M as Action>::Output>,
    M::From: Witness,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        bare: Bare<M>,
    ) -> Result<<M as Action>::Output> {
        let by = sender(&ctx, &bare)?;
        let Bare(message) = bare;
        act::run(&self, &ctx, by, None, M::NAME, message).await
    }
}

impl<M> Handler<Relays<M>> for Realm
where
    M: Message
        + Relay<<M as Message>::From, Output = <M as Action>::Output>,
    M::From: Witness,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        relays: Relays<M>,
    ) -> Result<<M as Action>::Output> {
        let by = sender(&ctx, &relays)?;
        let asked = relays.0.ask(&by, &Rows::outside(&ctx)?).await?;
        let (profile, call) = match asked {
            Ok(asked) => asked,
            Err(answer) => return Ok(answer),
        };
        let answered = self
            .peers
            .call(&Profile(profile), &call, When::BeforeAnswer)
            .await?;
        Ok(M::answer(answered))
    }
}

impl<M> Handler<Looks<M>> for Realm
where
    M: Message
        + Look<<M as Message>::From, Output = <M as Action>::Output>,
    M::From: Witness,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        looks: Looks<M>,
    ) -> Result<<M as Action>::Output> {
        let by = sender(&ctx, &looks)?;
        Ok(act::look(&self, &by, &looks.0))
    }
}
