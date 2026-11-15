use std::collections::BTreeSet;
use std::mem;
use std::sync::{Arc, PoisonError, RwLock};

use anyhow::{Result, bail};
use hamlet_actor::Ctx;
use hamlet_messages::realm::HamletId;
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::{
    AgentSeen, HamletSeen, ReachSeen, Revision, Seen, Spend, Status,
    WaitId, WaitKind, WaitSeen,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{AgentNumber, CallId, ReachVersion};
use tokio::sync::Mutex;

use super::act::Look;
use super::change::{self, Affected, Column as _, Rows};
use super::{Realm, budget, laid, tree, versions};

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
            "SELECT agent.id, agent.status, agent.ended_how, \
             agent.ended_why, hire.hamlet, hire.lead, reach.wanted, \
             reach.applied FROM agent LEFT JOIN hire ON hire.agent \
             = agent.id LEFT JOIN reach ON reach.agent = agent.id \
             ORDER BY agent.id",
            Vec::new(),
        )
        .await?
    {
        let [id, status, how, why, hamlet, lead, wanted, applied] =
            &row[..]
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
            hamlet: Option::of(hamlet)?.map(HamletId),
            lead: Option::of(lead)?.map(AgentNumber),
            status,
            reach: reach(Option::of(wanted)?, Option::of(applied)?),
            worker: None,
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

async fn waits(rows: &Rows<'_>) -> Result<Vec<WaitSeen>> {
    Ok(rows
        .rows::<(u64, u32, String, String)>(
            "SELECT id, agent, call, subject FROM wait WHERE \
             closed_at IS NULL AND kind = 'tool' ORDER BY id",
            Vec::new(),
        )
        .await?
        .into_iter()
        .map(|(wait, agent, call, subject)| WaitSeen {
            wait: WaitId(wait),
            agent: AgentNumber(agent),
            kind: WaitKind::Tool {
                call: CallId(call),
                subject,
            },
        })
        .collect())
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
        let spent = budget::spent(rows, &payer).await?;
        spend.push(Spend {
            payer: spent.payer,
            spent_micros: spent.micros,
            budget_micros: spent.budget.micros,
            spent_ms: spent.took_ms,
            budget_ms: spent.budget.ms,
        });
    }
    Ok(spend)
}

impl Look<Client> for ReadWorld {
    type Output = Seen;

    fn look(&self, _by: &Client, world: &World) -> Seen {
        world.clone()
    }
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::NeedsChief;
    use hamlet_messages::workspace::ConversationId;

    use super::super::testing::Started;
    use super::super::waits;
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
                    needs: Vec::new(),
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
