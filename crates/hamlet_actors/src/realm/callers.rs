use std::sync::Arc;

use anyhow::{Result, bail};
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_host::Caller;
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Party, Profile, Workspace};
use hamlet_messages::realm::budget::{OpenTask, TaskOpened};
use hamlet_messages::realm::choices::ChoiceRefusal;
use hamlet_messages::realm::client::{Answer, Say};
use hamlet_messages::realm::prompt::{PlaceText, ReadPlace};
use hamlet_messages::realm::{
    AgentName, Asked, ChiefRequest, Done, DoneTo, Gate, HamletId,
    LeadRefusal, OnlyAHire, ProfileId, Rank, Refusable, RoadDone,
    Seated, SecretList, Secrets, Standing, Tool, ToolCall,
};
use hamlet_messages::sender::{
    Client, Hand, Scrubber, Sender, sender, wrong_caller,
};
use hamlet_messages::workspace::{AgentNumber, Numbered};
use serde::{Deserialize, Serialize};

use super::act::{self, Act, Look, ToolAct};
use super::change::Rows;
use super::peer::When;
use super::tree::{self, Life, Lineage};
use super::workers::errands;
use super::{Realm, marker, prompt, record};

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

/// How a sender is answered for an act on an agent or a road: a chief
/// is told it was done, and an agent which agents it acted on.
pub(super) trait Shown {
    type OnAgent<W>: Refusable<Why = W> + Send + 'static
    where
        W: Send + 'static;
    type OnRoad<W>: Refusable<Why = W> + Send + 'static
    where
        W: Send + 'static;

    fn on_agent<W: Send + 'static>(
        agent: AgentName,
    ) -> Self::OnAgent<W>;

    fn on_road<W: Send + 'static>(
        a: AgentName,
        b: AgentName,
    ) -> Self::OnRoad<W>;
}

impl Shown for Client {
    type OnAgent<W>
        = Done<W>
    where
        W: Send + 'static;
    type OnRoad<W>
        = Done<W>
    where
        W: Send + 'static;

    fn on_agent<W: Send + 'static>(_: AgentName) -> Done<W> {
        Done::Done
    }

    fn on_road<W: Send + 'static>(
        _: AgentName,
        _: AgentName,
    ) -> Done<W> {
        Done::Done
    }
}

impl Shown for Agent {
    type OnAgent<W>
        = DoneTo<W>
    where
        W: Send + 'static;
    type OnRoad<W>
        = RoadDone<W>
    where
        W: Send + 'static;

    fn on_agent<W: Send + 'static>(agent: AgentName) -> DoneTo<W> {
        DoneTo::Done { agent }
    }

    fn on_road<W: Send + 'static>(
        a: AgentName,
        b: AgentName,
    ) -> RoadDone<W> {
        RoadDone::Done { a, b }
    }
}

impl Shown for Hand {
    type OnAgent<W>
        = DoneTo<W>
    where
        W: Send + 'static;
    type OnRoad<W>
        = RoadDone<W>
    where
        W: Send + 'static;

    fn on_agent<W: Send + 'static>(agent: AgentName) -> DoneTo<W> {
        Agent::on_agent(agent)
    }

    fn on_road<W: Send + 'static>(
        a: AgentName,
        b: AgentName,
    ) -> RoadDone<W> {
        Agent::on_road(a, b)
    }
}

/// A sender whose powers reach every hamlet.
pub(super) trait RealmWide: Decider + Shown {
    /// The answer to seating a mayor.
    type Seated: Refusable<Why = OnlyAHire> + Send + 'static;

    fn seated(mayor: AgentName, hamlet: HamletId) -> Self::Seated;
}

impl RealmWide for Client {
    type Seated = Done<OnlyAHire>;

    fn seated(_: AgentName, _: HamletId) -> Done<OnlyAHire> {
        Done::Done
    }
}

impl RealmWide for Hand {
    type Seated = Seated;

    fn seated(agent: AgentName, hamlet: HamletId) -> Seated {
        Seated::Done { agent, hamlet }
    }
}

/// A sender that can have a lead's powers over a hire (H41).
pub(super) trait Lead: Decider + Shown {
    /// A refusal `R` as this kind of sender is answered with it.
    type Why<R>: Send + 'static
    where
        R: Send + 'static;

    /// Whether this sender leads the agent of `lineage`.
    fn leads<R: Send + 'static>(
        &self,
        lineage: &Lineage,
    ) -> Result<(), Self::Why<R>>;

    fn refused<R: Send + 'static>(refusal: R) -> Self::Why<R>;

    /// The answer to setting a hamlet's model, which only a chief
    /// does.
    fn hamlet_model() -> Self::OnAgent<Self::Why<ChoiceRefusal>>;
}

impl Lead for Client {
    type Why<R>
        = R
    where
        R: Send + 'static;

    fn leads<R: Send + 'static>(&self, _: &Lineage) -> Result<(), R> {
        Ok(())
    }

    fn refused<R: Send + 'static>(refusal: R) -> R {
        refusal
    }

    fn hamlet_model() -> Done<ChoiceRefusal> {
        Done::Done
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
    ) -> Result<(), LeadRefusal<R>> {
        if lineage.is_led_by(self.0) {
            Ok(())
        } else {
            Err(LeadRefusal::Rank(Rank::NotALead {
                hire: lineage.agent.name.clone(),
            }))
        }
    }

    fn refused<R: Send + 'static>(refusal: R) -> LeadRefusal<R> {
        LeadRefusal::Act(refusal)
    }

    fn hamlet_model() -> DoneTo<LeadRefusal<ChoiceRefusal>> {
        DoneTo::refused(LeadRefusal::Act(
            ChoiceRefusal::HamletFromAnAgent,
        ))
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

/// A society tool no chief reserves, refused only by its caller's
/// standing.
#[derive(Serialize, Deserialize)]
#[serde(transparent)]
pub(super) struct Unreserved<M>(pub M);

impl<M: Message> Action for Unreserved<M> {
    type Output = M::Output;
    const NAME: &'static str = M::NAME;
}

impl<M: Message> Message for Unreserved<M> {
    type To = M::To;
    type From = M::From;
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

impl Handler<ReadPlace> for Realm {
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        read: ReadPlace,
    ) -> Result<PlaceText> {
        let by = sender(&ctx, &read)?;
        if let Numbered::Errand(_) = by.0.numbered() {
            return refuse(ReadPlace::NAME, by);
        }
        let reading = prompt::Reading {
            read,
            secrets: self.secrets.secrets(),
        };
        act::run(&self, &ctx, by, None, ReadPlace::NAME, reading)
            .await
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
    T: ToolAct<Agent>,
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
    T: ToolAct<Hand>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        hand: ByHand<Tool<T>>,
    ) -> Result<<T as ToolCall>::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &hand)?;
        let (key, _, act) = act::keyed(&by, name, hand.0)?;
        act::run_keyed(&self, &ctx, by, Some(key), name, act).await
    }
}

impl<T> Handler<Unreserved<Tool<T>>> for Realm
where
    T: ToolAct<Agent>,
    <T::Answer as Refusable>::Why: From<Standing>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        tool: Unreserved<Tool<T>>,
    ) -> Result<<T as ToolCall>::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &tool)?;
        unreserved(
            name,
            act::run_tool(&self, &ctx, by, name, tool.0).await?,
        )
    }
}

/// The answer of the society tool `name`, which no chief reserves, to
/// a caller that `ran` it or whose standing refused it.
pub(super) fn unreserved<O>(
    name: &str,
    ran: Result<O, Stood>,
) -> Result<O>
where
    O: Refusable,
    O::Why: From<Standing>,
{
    match ran {
        Ok(answer) => Ok(answer),
        Err(Stood::Standing(why)) => Ok(O::refused(why.into())),
        Err(Stood::Gate(why)) => {
            bail!("{name} met a chief's gate: {why}")
        }
    }
}

pub(super) fn to_tool<O>(ran: Result<O, Stood>) -> O
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
        relay(&self, &ctx, &by, &relays.0, When::BeforeAnswer).await
    }
}

/// Answers `relayed`, asked by `by`, from the answer of the profile
/// it names.
pub(super) async fn relay<By, M: Relay<By>>(
    realm: &Realm,
    ctx: &Ctx<Realm>,
    by: &By,
    relayed: &M,
    when: When,
) -> Result<M::Output> {
    let (profile, call) =
        match relayed.ask(by, &Rows::outside(ctx)?).await? {
            Ok(asked) => asked,
            Err(answer) => return Ok(answer),
        };
    let answered =
        realm.peers.call(&Profile(profile), &call, when).await?;
    Ok(M::answer(answered))
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
