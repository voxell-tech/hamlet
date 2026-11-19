use std::collections::BTreeSet;
use std::mem;
use std::sync::{Arc, PoisonError, RwLock};

use anyhow::{Context as _, Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::realm::HamletId;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::ReadWorld;
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
    Realm, budget, chiefs, laid, parcels, tree, versions, waits,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Part {
    Agents,
    Hamlets,
    Roads,
    Waits,
    Spend,
    Reaches,
}

impl Part {
    const ALL: [Self; 6] = [
        Self::Agents,
        Self::Hamlets,
        Self::Roads,
        Self::Waits,
        Self::Spend,
        Self::Reaches,
    ];
}

pub(super) type World = Seen;

pub(super) struct Held {
    current: RwLock<Arc<World>>,
    rebuilding: Mutex<()>,
}

impl Held {
    pub async fn load(ctx: &Ctx<Realm>) -> Result<Self> {
        let rows = Rows::outside(ctx)?;
        let revision = versions::revision(&rows).await?;
        let world = rebuilt(
            &rows,
            World::default(),
            revision,
            &BTreeSet::from(Part::ALL),
        )
        .await?;
        Ok(Self {
            current: RwLock::new(Arc::new(world)),
            rebuilding: Mutex::new(()),
        })
    }

    pub fn current(&self) -> Arc<World> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub async fn refresh(
        &self,
        ctx: &Ctx<Realm>,
        revision: Revision,
        affected: &Affected,
    ) {
        let _rebuilding = self.rebuilding.lock().await;
        let held = self.current();
        if revision <= held.revision {
            return;
        }
        let parts = if revision.0 == held.revision.0 + 1 {
            affected.seen.clone()
        } else {
            BTreeSet::from(Part::ALL)
        };
        let built = async {
            let rows = Rows::outside(ctx)?;
            rebuilt(&rows, World::clone(&held), revision, &parts)
                .await
        }
        .await;
        match built {
            Ok(world) => {
                let world = Arc::new(world);
                let _old = mem::replace(
                    &mut *self
                        .current
                        .write()
                        .unwrap_or_else(PoisonError::into_inner),
                    world,
                );
            }
            Err(error) => tracing::warn!(
                "the world's copy missed revision {}: {error:#}",
                revision.0
            ),
        }
    }
}

async fn rebuilt(
    rows: &Rows<'_>,
    mut world: World,
    revision: Revision,
    parts: &BTreeSet<Part>,
) -> Result<World> {
    world.revision = revision;
    if parts.contains(&Part::Agents) || parts.contains(&Part::Reaches)
    {
        world.agents = agents(rows).await?;
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
    Ok(world)
}

async fn agents(rows: &Rows<'_>) -> Result<Vec<AgentSeen>> {
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
                 allowance.hire = agent.id ORDER BY agent.id",
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
        let status = match tree::Status::of_row(
            agent,
            &String::of(status)?,
            Option::of(how)?,
            Option::of(why)?,
        )? {
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
            model: Option::of(model)?,
            effort: Option::of(effort)?
                .map(change::from_case_name)
                .transpose()?,
            fast: Option::of(fast)?,
            allowance_micros: Option::of(allowance)?,
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
        "SELECT id, name, mayor, tier FROM hamlet ORDER BY at, id",
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
    rows.rows::<WaitRow>(
        "SELECT id, agent, kind, call, subject, needs, detail FROM \
         wait WHERE closed_at IS NULL ORDER BY id",
        Vec::new(),
    )
    .await?
    .into_iter()
    .map(|row| {
        let wait = row.0;
        wait_seen(row).with_context(|| format!("wait {wait}"))
    })
    .collect()
}

fn wait_seen(
    (wait, agent, kind, call, subject, needs, detail): WaitRow,
) -> Result<WaitSeen> {
    let detail = || detail.as_deref().context("no detail");
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
                change: None,
            }
        }
        waits::Kind::Question | waits::Kind::Objection => {
            ciborium::from_reader(detail()?)?
        }
        waits::Kind::Reserved => WaitKind::Reserved,
        waits::Kind::Send => WaitKind::Send {
            measure: parcels::measure_of(detail()?)?,
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

    fn look(&self, _by: &Client, realm: &Realm) -> Seen {
        World::clone(&realm.world.current())
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

        assert_eq!(rebuilt, started.realm.world.current());
        assert_eq!(
            (
                rebuilt.agents.len(),
                rebuilt.hamlets.len(),
                rebuilt.waits.len()
            ),
            (2, 1, 1)
        );
        assert!(Arc::ptr_eq(&rebuilt, &missed.current()));
        started.stop().await;
    }
}
