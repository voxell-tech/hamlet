//! Who makes a call of the realm, and the forms a call arrives in.
//! The `Actions` tuple names a form per entry, so the rule of who may
//! call is a type: an entry whose act is not written for its form's
//! sender does not build.
//!
//! The senders are the messages' (`Client`, `Agent`, `Hand`,
//! `Workspace`), read from the host's stamp by
//! [`hamlet_messages::sender::sender`], which fails a sender of
//! another kind with `message.wrong_caller`. The realm's own traits
//! about a sender are implemented on them here. This is also the one
//! place a society tool's answer gains what refuses the caller
//! itself: the families `Standing` and `Gate`.

#![expect(
    dead_code,
    reason = "the scaffold: the units' bodies use these"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::{Action, Ctx, Handler};
use hamlet_messages::Message;
use hamlet_messages::address::{Agent, Workspace};
use hamlet_messages::realm::client::{Answer, Say};
use hamlet_messages::realm::{
    Asked, ChiefRequest, Gate, LeadRefusal, Rank, Refusable,
    SocietyTool, Standing, Tool, ToolCall,
};
use hamlet_messages::sender::{Client, Hand, Sender, sender};
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{self, Act, Look};
use super::change::Rows;
use super::tree::{self, Lineage};
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

/// What refuses an agent itself, before its society tool runs.
#[derive(Debug)]
pub(super) enum Stood {
    Standing(Standing),
    Gate(Gate),
}

/// A sender that is an agent of the tree, whose own standing is read
/// before its society tool runs.
///
/// `standing` is an `async fn` in a trait, so its future is known to
/// be `Send` only where the sender is a concrete type, as in the one
/// handler that reads it. A handler generic over the member does not
/// build.
pub(super) trait Member: Witness {
    /// Why the sender itself is refused: it has ended, or its lead
    /// turned `tool` off for it (C16). Read from rows, never from the
    /// caller's copy of its place.
    async fn standing(
        &self,
        rows: &Rows<'_>,
        tool: SocietyTool,
    ) -> Result<Option<Stood>>;
}

impl Member for Agent {
    async fn standing(
        &self,
        rows: &Rows<'_>,
        tool: SocietyTool,
    ) -> Result<Option<Stood>> {
        if let Some(left) = tree::left(rows, self.0).await? {
            return Ok(Some(Stood::Standing(Standing::YouLeft {
                how: left.how,
                why: left.why,
            })));
        }
        Ok(tree::tools::is_off(rows, self.0, tool)
            .await?
            .then_some(Stood::Gate(Gate::ToolOff { tool })))
    }
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

/// A sender whose powers reach every hamlet: the chief and the Hand,
/// who found hamlets and seat mayors (H6, H18). No agent but the Hand
/// is one, so `Tool<Found>` or `Tool<Seat>` listed for an agent does
/// not build; the Hand sends them as `ByHand<Tool<_>>`. Neither is
/// refused for its standing (the chief has none; the Hand never
/// leaves and has no lead), so both answer the act's own family.
pub(super) trait RealmWide: Decider {}

impl RealmWide for Client {}

impl RealmWide for Hand {}

/// A sender that can have a lead's powers over a hire (H41): the
/// chief over every hire, an agent over the hires below it, the Hand
/// over all.
pub(super) trait Lead: Decider {
    /// A refusal `R` as this kind of sender is answered with it. The
    /// chief is never refused for want of a lead's powers, so its
    /// answers cannot hold that refusal.
    type Why<R>: Send + 'static
    where
        R: Send + 'static;

    /// Whether the sender leads `hire`, read from the hire's lineage.
    fn leads<R: Send + 'static>(
        &self,
        lineage: &Lineage,
        hire: AgentNumber,
    ) -> Result<(), Self::Why<R>>;

    /// `refusal`, as this sender is answered with it.
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

// The realm's own forms. On the wire each is the message it wraps.

/// A society tool only the Hand holds: `found` and `seat`.
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

// The handlers. Each reads its sender first, so no act runs for a
// sender of another kind, and then hands the act to `act::run`.

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
    T: ToolCall + Act<Agent, Output = <T as ToolCall>::Answer>,
    <T::Answer as Refusable>::Why: From<Standing> + From<Gate>,
{
    async fn handle(
        self: Arc<Self>,
        ctx: Ctx<Self>,
        tool: Tool<T>,
    ) -> Result<<T as ToolCall>::Answer> {
        let name = <Tool<T> as Action>::NAME;
        let by = sender(&ctx, &tool)?;
        let id = marker::Id::Call(tool.call);
        let ran = act::run_tool(
            &self,
            &ctx,
            by,
            id,
            name,
            T::TOOL,
            tool.act,
        )
        .await?;
        Ok(to_tool(ran))
    }
}

/// The Hand's own tools. No standing is read: the act runs as a
/// chief's request does, and answers its own family.
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
        let id = marker::Id::Call(tool.call);
        act::run(&self, &ctx, by, Some(id), name, tool.act).await
    }
}

/// A society tool's answer: the sender's own standing first, then the
/// act's.
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
