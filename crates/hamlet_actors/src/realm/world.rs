use std::collections::BTreeSet;
use std::mem;
use std::sync::{Arc, PoisonError, RwLock};

use anyhow::{Context as _, Result, bail};
use hamlet_actor::Ctx;
use hamlet_host::HostRef;
use hamlet_messages::realm::HamletId;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::{ReadWorld, World};
use hamlet_messages::realm::world::{
    AgentSeen, GrantSeen, GrantSource, HamletSeen, Lasting,
    ReachSeen, Revision, Seen, Spend, Starter, Status, WaitId,
    WaitKind, WaitSeen,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{AgentNumber, CallId, ReachVersion};
use tokio::sync::Mutex;

use super::act::Look;
use super::change::{self, Affected, Column as _, Rows};
use super::tree::choices;
use super::{
    Options, Realm, budget, chiefs, documents, laid, parcels, state,
    tree, versions, waits,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Part {
    Agents,
    Hamlets,
    Roads,
    Waits,
    Spend,
    Reaches,
    /// The parts only a client's whole state shows: the profiles, the
    /// settings and the limits, an agent's consulting, and the
    /// letters along the roads.
    State,
    /// Whether an agent is working, which no row holds.
    Working,
}

impl Part {
    const ALL: [Self; 7] = [
        Self::Agents,
        Self::Hamlets,
        Self::Roads,
        Self::Waits,
        Self::Spend,
        Self::Reaches,
        Self::State,
    ];
}

/// The copy of the world the realm answers clients from.
#[derive(Clone, Default)]
pub(super) struct Kept {
    pub seen: Seen,
    pub shown: state::Shown,
}

pub(super) struct Held {
    current: RwLock<Arc<Kept>>,
    rebuilding: Mutex<()>,
}

/// A change that moves the revision alone, for what a client is
/// shown that no row holds.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Moved,
}

pub(super) fn apply(change: Change) -> Affected {
    match change {
        Change::Moved => Affected::seen([Part::Working]),
    }
}

/// Moves the revision, so each client reads the world again.
pub(super) async fn moved(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
) -> Result<()> {
    change::commit(realm, ctx, vec![Change::Moved.into()]).await?;
    Ok(())
}

impl Held {
    pub async fn load(ctx: &Ctx<Realm>) -> Result<Self> {
        let rows = Rows::outside(ctx)?;
        let revision = versions::revision(&rows).await?;
        let kept = rebuilt(
            &rows,
            faux(ctx)?,
            Kept::default(),
            revision,
            &BTreeSet::from(Part::ALL),
        )
        .await?;
        Ok(Self {
            current: RwLock::new(Arc::new(kept)),
            rebuilding: Mutex::new(()),
        })
    }

    pub fn current(&self) -> Arc<Kept> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Rebuilds the parts `affected` moved at `revision`, and tells
    /// each subscriber the world moved.
    pub async fn refresh(
        &self,
        ctx: &Ctx<Realm>,
        revision: Revision,
        affected: &Affected,
    ) {
        let _rebuilding = self.rebuilding.lock().await;
        let held = self.current();
        if revision <= held.seen.revision {
            return;
        }
        let parts = if revision.0 == held.seen.revision.0 + 1 {
            affected.seen.clone()
        } else {
            BTreeSet::from(Part::ALL)
        };
        let built = async {
            let rows = Rows::outside(ctx)?;
            let faux = faux(ctx)?;
            rebuilt(&rows, faux, Kept::clone(&held), revision, &parts)
                .await
        }
        .await;
        match built {
            Ok(kept) => {
                let kept = Arc::new(kept);
                let _old = mem::replace(
                    &mut *self
                        .current
                        .write()
                        .unwrap_or_else(PoisonError::into_inner),
                    kept,
                );
                if let Err(error) = ctx.emit(World { revision }) {
                    tracing::warn!(
                        "the world's event for revision {} was not \
                         sent: {error:#}",
                        revision.0
                    );
                }
            }
            Err(error) => tracing::warn!(
                "the world's copy missed revision {}: {error:#}",
                revision.0
            ),
        }
    }
}

/// The model each agent of a faux realm runs.
const FAUX_MODEL: &str = "faux::faux-1";

/// Whether the realm's agents answer from a faux script.
fn faux(ctx: &Ctx<Realm>) -> Result<bool> {
    Ok(HostRef::of(ctx.inner())?
        .value::<Options>()
        .is_some_and(|options| options.faux))
}

async fn rebuilt(
    rows: &Rows<'_>,
    faux: bool,
    mut kept: Kept,
    revision: Revision,
    parts: &BTreeSet<Part>,
) -> Result<Kept> {
    let world = &mut kept.seen;
    world.revision = revision;
    if parts.contains(&Part::Agents) || parts.contains(&Part::Reaches)
    {
        world.agents = agents(rows, faux).await?;
        world.grants = grants(rows).await?;
    }
    if parts.contains(&Part::Hamlets) {
        world.hamlets = hamlets(rows).await?;
    }
    if parts.contains(&Part::Roads) {
        world.roads = laid::live_roads(rows).await?;
    }
    if parts.contains(&Part::Waits) {
        world.waits = waits(rows).await?;
    }
    if parts.contains(&Part::Spend) {
        world.spend = spend(rows).await?;
    }
    if parts.iter().any(|part| *part != Part::Working) {
        kept.shown = state::read(rows, &kept.seen).await?;
    }
    Ok(kept)
}

async fn agents(
    rows: &Rows<'_>,
    faux: bool,
) -> Result<Vec<AgentSeen>> {
    let mut agents = Vec::new();
    for row in rows
        .select(
            &format!(
                "SELECT agent.id, agent.status, agent.ended_how, \
                 agent.ended_why, hire.lead, reach.wanted, \
                 reach.applied, worker.agent, worker.starter, \
                 nickname.name, model_choice.model, \
                 model_choice.effort, model_choice.fast, \
                 allowance.micros FROM agent LEFT JOIN hire ON \
                 hire.agent = agent.id LEFT JOIN reach ON \
                 reach.agent = agent.id LEFT JOIN worker ON \
                 worker.agent = agent.id LEFT JOIN nickname ON \
                 nickname.agent = agent.id LEFT JOIN model_choice \
                 ON model_choice.of = {} LEFT JOIN allowance ON \
                 allowance.hire = agent.id WHERE agent.erased = 0 \
                 ORDER BY agent.id",
                choices::agent_key("agent.id")
            ),
            Vec::new(),
        )
        .await?
    {
        let [
            id,
            status,
            how,
            why,
            lead,
            wanted,
            applied,
            worker,
            starter,
            nickname,
            model,
            effort,
            fast,
            allowance,
        ] = &row[..]
        else {
            bail!("an agent's row of another form: {row:?}");
        };
        let agent = AgentNumber(u32::of(id)?);
        let tools_off = tree::tools::off_for(rows, agent).await?;
        let status = match tree::Status::of_row(
            rows,
            agent,
            &String::of(status)?,
            Option::of(how)?,
            Option::of(why)?,
        )
        .await?
        {
            tree::Status::Hiring => Status::Hiring,
            tree::Status::Live => Status::Live,
            tree::Status::Ended(left) => Status::Ended { left },
        };
        agents.push(AgentSeen {
            agent,
            hamlet: chiefs::hamlet::of_agent(rows, agent).await?,
            lead: Option::of(lead)?.map(AgentNumber),
            status,
            reach: reach(Option::of(wanted)?, Option::of(applied)?),
            worker: match (
                Option::<u32>::of(worker)?,
                Option::of(starter)?,
            ) {
                (None, _) => None,
                (Some(_), None) => Some(Starter::Chief),
                (Some(_), Some(agent)) => Some(Starter::Agent {
                    agent: AgentNumber(agent),
                }),
            },
            nickname: Option::of(nickname)?,
            runs: match faux {
                true => Some(FAUX_MODEL.to_owned()),
                false => documents::model(rows, agent)
                    .await?
                    .map(|chosen| chosen.model),
            },
            model: Option::of(model)?,
            effort: Option::of(effort)?
                .map(change::from_case_name)
                .transpose()?,
            fast: Option::of(fast)?,
            allowance_micros: Option::of(allowance)?,
            tools_off,
        });
    }
    Ok(agents)
}

fn reach(wanted: Option<u64>, applied: Option<u64>) -> ReachSeen {
    match (wanted, applied) {
        (None, _) => ReachSeen::None,
        (Some(wanted), Some(applied)) if applied >= wanted => {
            ReachSeen::Applied {
                version: ReachVersion(applied),
            }
        }
        (Some(wanted), applied) => ReachSeen::Pending {
            applied: applied.map(ReachVersion),
            wanted: ReachVersion(wanted),
        },
    }
}

async fn hamlets(rows: &Rows<'_>) -> Result<Vec<HamletSeen>> {
    rows.rows::<(String, String, u32, String)>(
        "SELECT id, name, mayor, tier FROM hamlet WHERE status = \
         'living' ORDER BY at, id",
        Vec::new(),
    )
    .await?
    .into_iter()
    .map(|(id, name, mayor, tier)| {
        Ok(HamletSeen {
            hamlet: HamletId(id),
            name,
            mayor: AgentNumber(mayor),
            tier: change::from_case_name(tier)?,
        })
    })
    .collect()
}

type WaitRow = (
    u64,
    u32,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<Vec<u8>>,
);

async fn waits(rows: &Rows<'_>) -> Result<Vec<WaitSeen>> {
    Ok(rows
        .rows::<WaitRow>(
            "SELECT id, agent, kind, call, subject, needs, detail \
             FROM wait WHERE closed_at IS NULL ORDER BY id",
            Vec::new(),
        )
        .await?
        .into_iter()
        .filter_map(|row| {
            let wait = row.0;
            wait_seen(row)
                .inspect_err(|error| {
                    tracing::warn!(
                        wait,
                        "a wait is left out: {error:#}"
                    );
                })
                .ok()
        })
        .collect())
}

fn wait_seen(
    (wait, agent, kind, call, subject, needs, detail): WaitRow,
) -> Result<WaitSeen> {
    let needed = || detail.as_deref().context("no detail");
    let kind = match change::from_case_name(kind)? {
        waits::Kind::Tool => {
            let needs = serde_json::from_str::<Vec<Need>>(
                needs.as_deref().context("no needs")?,
            )?;
            let (danger, always) = danger_of(&needs)?;
            WaitKind::Tool {
                call: CallId(call.context("no call")?),
                danger,
                always: Some(always),
                change: detail
                    .as_deref()
                    .map(ciborium::from_reader)
                    .transpose()?,
            }
        }
        waits::Kind::Question
        | waits::Kind::Objection
        | waits::Kind::Newcomer => ciborium::from_reader(needed()?)?,
        waits::Kind::Reserved => WaitKind::Reserved,
        waits::Kind::Send => WaitKind::Send {
            measure: parcels::measure_of(needed()?)?,
        },
        waits::Kind::Fault => WaitKind::Fault,
    };
    Ok(WaitSeen {
        wait: WaitId(wait),
        agent: AgentNumber(agent),
        subject: subject.context("no subject")?,
        kind,
    })
}

/// What a tool wait that needs `needs` lets through, the network
/// before a command before an edit, and what allowing it always
/// keeps.
fn danger_of(needs: &[Need]) -> Result<(Need, Lasting)> {
    let danger = [Need::Network, Need::Command, Need::Edit]
        .into_iter()
        .find(|need| needs.contains(need))
        .context("a tool wait that needs nothing")?;
    let always = match danger {
        Need::Network => Lasting::Grant,
        Need::Command | Need::Edit => Lasting::Session,
    };
    Ok((danger, always))
}

async fn grants(rows: &Rows<'_>) -> Result<Vec<GrantSeen>> {
    rows.rows::<(u32, String)>(
        "SELECT grant.agent, grant.permission FROM grant JOIN agent \
         ON agent.id = grant.agent WHERE grant.revoked_at IS NULL \
         AND agent.status != 'ended' ORDER BY grant.agent, \
         grant.permission",
        Vec::new(),
    )
    .await?
    .into_iter()
    .map(|(agent, permission)| {
        Ok(GrantSeen {
            agent: AgentNumber(agent),
            permission: change::from_case_name(permission)?,
            source: GrantSource::Grant,
        })
    })
    .collect()
}

async fn spend(rows: &Rows<'_>) -> Result<Vec<Spend>> {
    let realm = rows
        .value::<i64>("SELECT id FROM realm", Vec::new())
        .await?
        .map(|_| Payer::Realm);
    let hamlets = rows
        .values::<String>(
            "SELECT id FROM hamlet ORDER BY at, id",
            Vec::new(),
        )
        .await?
        .into_iter()
        .map(|hamlet| Payer::Hamlet {
            hamlet: HamletId(hamlet),
        });
    let mut spend = Vec::new();
    for payer in realm.into_iter().chain(hamlets) {
        spend.push(budget::spent(rows, &payer).await?.into());
    }
    Ok(spend)
}

impl Look<Client> for ReadWorld {
    type Output = Seen;

    fn look(&self, _by: &Client, realm: &Realm) -> Result<Seen> {
        realm.secrets.scrubbed_value(&realm.world.current().seen)
    }
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::NeedsChief;
    use hamlet_messages::workspace::ConversationId;

    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_copy_that_missed_a_change_rebuilds_every_part() {
        // load a copy; found a hamlet and open a wait, which it
        // misses; refresh it for the wait alone: it equals the
        // realm's own copy, mayor and hamlet with it. A refresh older
        // than it changes nothing.
        let started = Started::new().await;
        let missed = Held::load(&started.ctx).await.unwrap();
        started
            .found(&HamletId("h".to_owned()), AgentNumber(2))
            .await;
        started
            .commit(waits::Change::Open {
                asked: NeedsChief {
                    agent: AgentNumber(2),
                    call: CallId("c1".to_owned()),
                    conversation: ConversationId("c".to_owned()),
                    realm_task: None,
                    needs: vec![Need::Command],
                    subject: "rm -rf build".to_owned(),
                    change: None,
                    reach_version: ReachVersion(1),
                },
            })
            .await
            .unwrap();
        let revision =
            versions::revision(&started.rows()).await.unwrap();

        missed
            .refresh(
                &started.ctx,
                revision,
                &Affected::seen([Part::Waits]),
            )
            .await;
        let rebuilt = missed.current();
        missed
            .refresh(
                &started.ctx,
                Revision(revision.0 - 1),
                &Affected::seen([Part::Agents]),
            )
            .await;

        assert_eq!(rebuilt.seen, started.realm.world.current().seen);
        assert_eq!(
            (
                rebuilt.seen.agents.len(),
                rebuilt.seen.hamlets.len(),
                rebuilt.seen.waits.len()
            ),
            (2, 1, 1)
        );
        assert!(Arc::ptr_eq(&rebuilt, &missed.current()));
        started.stop().await;
    }
}
