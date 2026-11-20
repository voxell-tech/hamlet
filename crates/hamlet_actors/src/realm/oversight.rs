//! A chief's oversight of agents: the `grant` and `trust` tables, the
//! footing a reach is built on, and `Revoke`.

pub(super) mod asking;
pub(super) mod reserved;

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::realm::oversight::{
    Permission, Revoke, RevokeRefusal, Revoked,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::reach::{
    Footing, Project, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{AgentNumber, ConversationId};

use super::act::{Act, Decided, Decision};
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer, number};
use super::workers::{self, errands};
use super::world::Part;
use super::{chiefs, record, settings, tree};

/// Each raises the reach of its agent and of the agent's living
/// workers, when it changes a row.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// `agent` may use `permission` until a chief revokes it.
    Grant {
        agent: AgentNumber,
        permission: Permission,
    },
    Revoke {
        agent: AgentNumber,
        permission: Permission,
    },
    /// `agent` may make edits or run commands for the rest of
    /// `conversation`, besides what it may already.
    Trust {
        agent: AgentNumber,
        conversation: ConversationId,
        edits: bool,
        commands: bool,
    },
    DropTrust {
        agent: AgentNumber,
        conversation: ConversationId,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let at = BindParam::Integer(writer.at().0);
    let (agent, changed) = match change {
        Change::Trust {
            edits: false,
            commands: false,
            ..
        } => return Ok(Affected::default()),
        Change::Grant { agent, permission } => (
            agent,
            writer
                .execute(
                    "INSERT INTO grant (agent, permission, at) \
                     SELECT ?1, ?2, ?3 WHERE NOT EXISTS (SELECT 1 \
                     FROM grant WHERE agent = ?1 AND permission = \
                     ?2 AND revoked_at IS NULL)",
                    vec![number(agent), named(permission)?, at],
                )
                .await?,
        ),
        Change::Revoke { agent, permission } => (
            agent,
            writer
                .execute(
                    "UPDATE grant SET revoked_at = ?3 WHERE agent = \
                     ?1 AND permission = ?2 AND revoked_at IS NULL",
                    vec![number(agent), named(permission)?, at],
                )
                .await?,
        ),
        Change::Trust {
            agent,
            conversation,
            edits,
            commands,
        } => (
            agent,
            writer
                .execute(
                    "INSERT INTO trust (agent, conversation, edits, \
                     commands, at) VALUES (?1, ?2, ?3, ?4, ?5) ON \
                     CONFLICT (agent, conversation) DO UPDATE SET \
                     edits = MAX(edits, excluded.edits), commands = \
                     MAX(commands, excluded.commands) WHERE edits < \
                     excluded.edits OR commands < excluded.commands",
                    vec![
                        number(agent),
                        BindParam::Text(conversation.0),
                        BindParam::Integer(i64::from(edits)),
                        BindParam::Integer(i64::from(commands)),
                        at,
                    ],
                )
                .await?,
        ),
        Change::DropTrust {
            agent,
            conversation,
        } => (
            agent,
            writer
                .execute(
                    "DELETE FROM trust WHERE agent = ?1 AND \
                     conversation = ?2",
                    vec![
                        number(agent),
                        BindParam::Text(conversation.0),
                    ],
                )
                .await?,
        ),
    };
    if changed == 0 {
        return Ok(Affected::default());
    }
    Ok(Affected {
        reaches: reached(&writer.rows(), agent).await?,
        ..Affected::seen([Part::Reaches])
    })
}

fn named(permission: Permission) -> Result<BindParam> {
    Ok(BindParam::Text(change::case_name(&permission)?))
}

/// `agent` and its living workers, whose footing is its own.
async fn reached(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<BTreeSet<AgentNumber>> {
    Ok(std::iter::once(agent)
        .chain(workers::living_of(rows, agent).await?)
        .collect())
}

/// `agent`'s footing: its hamlet's folder and mode, the network, its
/// trust, and the secrets' version.
pub(super) async fn footing(
    rows: &Rows<'_>,
    agent: AgentNumber,
    secrets: SecretsVersion,
) -> Result<Footing> {
    let hamlet = tree::lineage_of(rows, agent).await?.hamlet;
    let mut footing =
        hamlet_footing(rows, hamlet.as_ref(), secrets).await?;
    footing.network |=
        holds(rows, agent, Permission::Network).await?;
    footing.trust = trust(rows, agent).await?;
    Ok(footing)
}

/// The footing `hamlet` gives an agent that holds no grant and no
/// trust.
pub(super) async fn hamlet_footing(
    rows: &Rows<'_>,
    hamlet: Option<&chiefs::Hamlet>,
    secrets: SecretsVersion,
) -> Result<Footing> {
    Ok(Footing {
        project: hamlet.and_then(|hamlet| {
            Some(Project {
                folder: hamlet.folder.clone()?,
                mode: hamlet.mode,
            })
        }),
        network: settings::read(rows).await?.allow_net,
        trust: Vec::new(),
        secrets,
    })
}

pub(super) async fn holds(
    rows: &Rows<'_>,
    agent: AgentNumber,
    permission: Permission,
) -> Result<bool> {
    Ok(rows
        .value(
            "SELECT EXISTS (SELECT 1 FROM grant WHERE agent = ?1 \
             AND permission = ?2 AND revoked_at IS NULL)",
            vec![number(agent), named(permission)?],
        )
        .await?
        .unwrap_or(false))
}

async fn trust(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<Trust>> {
    Ok(rows
        .rows::<(String, bool, bool)>(
            "SELECT conversation, edits, commands FROM trust WHERE \
             agent = ?1 ORDER BY conversation",
            vec![number(agent)],
        )
        .await?
        .into_iter()
        .map(|(conversation, edits, commands)| Trust {
            conversation: ConversationId(conversation),
            edits,
            commands,
        })
        .collect())
}

pub(super) enum Holding {
    Grant {
        name: String,
    },
    /// No grant, and whether the settings give the permission to
    /// every agent.
    Nothing {
        name: String,
        to_all: bool,
    },
    NoAgent,
}

impl Act<Client> for Revoke {
    type Output = Revoked;
    type Loaded = Holding;
    type Done = String;

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<Holding> {
        if errands::standing(rows, self.agent).await?.is_some() {
            return Ok(Holding::NoAgent);
        }
        let name = record::agent_name(rows, self.agent).await?.name;
        if holds(rows, self.agent, self.permission).await? {
            return Ok(Holding::Grant { name });
        }
        let to_all = match self.permission {
            Permission::Network => {
                settings::read(rows).await?.allow_net
            }
        };
        Ok(Holding::Nothing { name, to_all })
    }

    fn decide(
        &self,
        _by: &Client,
        holding: Holding,
    ) -> Decision<String, Self::Output> {
        let permission = self.permission;
        Decision::Answer(match holding {
            Holding::Grant { name } => {
                return Decision::Change(Decided::new(name).change(
                    Change::Revoke {
                        agent: self.agent,
                        permission,
                    },
                ));
            }
            Holding::Nothing {
                name,
                to_all: false,
            } => Revoked::NoGrant { name, permission },
            Holding::Nothing { name, to_all: true } => {
                Revoked::AllowedToAll { name, permission }
            }
            Holding::NoAgent => Revoked::Refused {
                why: RevokeRefusal::NoSuchAgent {
                    address: self.agent.0.to_string(),
                },
            },
        })
    }

    fn answer(&self, name: String, _made: &Made) -> Self::Output {
        Revoked::Revoked {
            name,
            permission: self.permission,
        }
    }
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::{HamletId, NeedsChief};
    use hamlet_messages::workspace::reach::{Mode, Reach};
    use hamlet_messages::workspace::{
        CallId, ReachVersion, SetReach,
    };

    use super::super::calls::Push;
    use super::super::testing::Started;
    use super::super::{reach, secrets, waits};
    use super::*;

    const MAYOR: AgentNumber = AgentNumber(2);
    const STARTER: AgentNumber = AgentNumber(3);
    const WORKER: AgentNumber = AgentNumber(4);
    const CHIEFS_WORKER: AgentNumber = AgentNumber(5);
    const FOLDER: &str = "/work/mill";

    /// Hamlet `h` in [`FOLDER`] with agent 3, its worker 4, and a
    /// chief's worker 5.
    async fn with_workers() -> Started {
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, MAYOR).await;
        started.hire(&hamlet, STARTER, Some(MAYOR)).await;
        started
            .ctx
            .sql()
            .exec(&format!(
                "UPDATE hamlet SET folder = '{FOLDER}' WHERE id = \
                 'h'; INSERT INTO agent (id, kind, status, at) \
                 VALUES (4, 'worker', 'live', 0), (5, 'worker', \
                 'live', 0); INSERT INTO worker (agent, starter, \
                 hamlet, brief, tools, task, at) VALUES (4, 3, 'h', \
                 'b', '[]', 1, 0), (5, NULL, 'h', 'b', '[]', 2, 0)"
            ))
            .await
            .unwrap();
        started
    }

    async fn pushes(started: &Started, made: Change) -> Vec<Push> {
        change::commit(
            &started.realm,
            &started.ctx,
            vec![made.into()],
        )
        .await
        .unwrap()
    }

    async fn reach_of(
        started: &Started,
        agent: AgentNumber,
    ) -> SetReach {
        reach::of(&started.rows(), agent, &started.realm.secrets)
            .await
            .unwrap()
    }

    fn footing_of(set: SetReach) -> Footing {
        set.reach.footing().cloned().expect("a living reach")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_grant_reaches_the_next_reach() {
        let started = with_workers().await;
        let before = reach_of(&started, STARTER).await.version;
        started
            .commit(waits::Change::Open {
                asked: NeedsChief {
                    agent: STARTER,
                    call: CallId("c1".to_owned()),
                    conversation: ConversationId("c".to_owned()),
                    realm_task: None,
                    needs: Vec::new(),
                    subject: "curl example.com".to_owned(),
                    change: None,
                    reach_version: before,
                },
            })
            .await
            .unwrap();
        let grant = || Change::Grant {
            agent: STARTER,
            permission: Permission::Network,
        };

        let first = pushes(&started, grant()).await;
        let second = pushes(&started, grant()).await;

        assert_eq!(
            (first, second),
            (
                vec![Push::Reach(STARTER), Push::Reach(WORKER)],
                Vec::new()
            )
        );
        let starter = reach_of(&started, STARTER).await;
        assert_eq!(starter.version, ReachVersion(before.0 + 1));
        assert!(footing_of(starter).network);
        assert!(footing_of(reach_of(&started, WORKER).await).network);
        assert_eq!(
            reach_of(&started, CHIEFS_WORKER).await.reach,
            Reach::Worker {
                starter: None,
                footing: Footing {
                    project: Some(Project {
                        folder: FOLDER.into(),
                        mode: Mode::Manual,
                    }),
                    network: false,
                    trust: Vec::new(),
                    secrets: secrets::VERSION,
                },
            }
        );
        let open = started
            .rows()
            .value::<bool>(
                "SELECT closed_at IS NULL FROM wait WHERE agent = 3",
                Vec::new(),
            )
            .await
            .unwrap();
        assert_eq!(open, Some(true));
        started.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_trust_holds_for_its_conversation_until_dropped() {
        let started = with_workers().await;
        let conversation = || ConversationId("c".to_owned());
        let trust = |edits, commands| Change::Trust {
            agent: STARTER,
            conversation: conversation(),
            edits,
            commands,
        };
        let drop = || Change::DropTrust {
            agent: STARTER,
            conversation: conversation(),
        };
        let both = vec![Push::Reach(STARTER), Push::Reach(WORKER)];

        assert_eq!(pushes(&started, trust(false, false)).await, []);
        assert_eq!(pushes(&started, trust(true, false)).await, both);
        assert_eq!(pushes(&started, trust(true, false)).await, []);
        assert_eq!(pushes(&started, trust(false, true)).await, both);
        assert_eq!(
            footing_of(reach_of(&started, STARTER).await).trust,
            [Trust {
                conversation: conversation(),
                edits: true,
                commands: true,
            }]
        );
        assert_eq!(
            footing_of(reach_of(&started, WORKER).await).trust,
            []
        );
        assert_eq!(pushes(&started, drop()).await, both);
        assert_eq!(pushes(&started, drop()).await, []);
        assert_eq!(
            footing_of(reach_of(&started, STARTER).await).trust,
            []
        );
        started.stop().await;
    }
}
