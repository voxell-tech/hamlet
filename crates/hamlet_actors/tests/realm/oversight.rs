use hamlet_messages::realm::oversight::{
    Permission, Revoke, RevokeRefusal,
};
use hamlet_messages::realm::{Asked, Done, End, HamletId};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, SetReach,
};
use hamlet_protocol::client::RequestId;

use super::{
    FOUR, MAYOR, Realm, THREE, founded, hires, to, tool, until,
};

const FOLDER: &str = "/work/mill";

/// The newest reach the workspace holds for `agent`.
fn held(realm: &Realm, agent: AgentNumber) -> Option<SetReach> {
    realm
        .seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .filter(|reach| reach.agent == agent)
        .max_by_key(|reach| reach.version)
        .cloned()
}

fn footing(set: Option<SetReach>) -> Footing {
    match set.map(|set| set.reach) {
        Some(Reach::Agent(footing)) => footing,
        other => panic!("an agent's reach: {other:?}"),
    }
}

fn revoke(agent: AgentNumber) -> Asked<Revoke> {
    Asked {
        id: RequestId::new(),
        act: Revoke {
            agent,
            permission: Permission::Network,
        },
    }
}

/// A realm with a hamlet whose folder is [`FOLDER`] in auto mode,
/// and the mayor live.
async fn hamlet_in_a_folder() -> (Realm, HamletId) {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    realm
        .write(&format!(
            "UPDATE hamlet SET folder = '{FOLDER}', mode = 'auto' \
             WHERE id = '{}'",
            hamlet.0
        ))
        .await;
    (realm, hamlet)
}

/// Hires agent 3 under the mayor with a network grant and the
/// trust of conversation `c` for edits.
async fn trusted_hire(realm: &Realm) {
    realm
        .write(
            "INSERT INTO grant (agent, permission, at) VALUES (3, \
             'network', 0); INSERT INTO trust (agent, conversation, \
             edits, commands, at) VALUES (3, 'c', 1, 0, 0)",
        )
        .await;
    hires(realm, MAYOR, "h1").await;
    realm.live(THREE).await;
}

async fn wanted(realm: &Realm, agent: AgentNumber) -> i64 {
    realm
        .rows(|db| {
            db.query_row(
                "SELECT wanted FROM reach WHERE agent = ?1",
                [agent.0],
                |row| row.get(0),
            )
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reach_takes_the_hamlets_mode_and_folder() {
    let (realm, _) = hamlet_in_a_folder().await;

    hires(&realm, MAYOR, "h1").await;
    realm.live(THREE).await;

    assert_eq!(
        footing(held(&realm, THREE)),
        Footing {
            project: Some(Project {
                folder: FOLDER.into(),
                mode: Mode::Auto,
            }),
            network: false,
            trust: Vec::new(),
            secrets: SecretsVersion(1),
        }
    );
    assert_eq!(footing(held(&realm, Hand::NUMBER)).project, None);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoke_raises_the_reach() {
    // agent 3 is hired holding a grant and a trust: its reach has
    // the network and the trust. The chief revokes the grant: the
    // workspace gets a newer reach without the network, the trust
    // kept. The same request again is answered the same; another
    // revoke finds no grant and raises nothing.
    let (realm, _) = hamlet_in_a_folder().await;
    trusted_hire(&realm).await;
    let granted = held(&realm, THREE).unwrap();
    let asked = revoke(THREE);

    let revoked = realm.ask(&asked).await.unwrap();
    until("the workspace holds a newer reach", || {
        held(&realm, THREE).unwrap().version > granted.version
    })
    .await;
    let again = realm.ask(&asked).await.unwrap();
    let raised = wanted(&realm, THREE).await;
    let none_left = realm.ask(&revoke(THREE)).await.unwrap();
    let never_held = realm.ask(&revoke(MAYOR)).await.unwrap();

    assert!(footing(Some(granted.clone())).network);
    assert_eq!(
        footing(held(&realm, THREE)),
        Footing {
            network: false,
            ..footing(Some(granted))
        }
    );
    assert_eq!((revoked, again), (Done::Done, Done::Done));
    assert_eq!(
        (none_left, never_held),
        (
            Done::Refused {
                why: RevokeRefusal::NoGrant { agent: THREE }
            },
            Done::Refused {
                why: RevokeRefusal::NoGrant { agent: MAYOR }
            }
        )
    );
    assert_eq!(wanted(&realm, THREE).await, raised);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_takes_its_starters_footing() {
    // agent 4 is a worker of agent 3, which holds a trust. A revoke
    // of 3's grant raises 4's reach too: a worker's reach with its
    // starter's footing, the trust left out.
    let (realm, hamlet) = hamlet_in_a_folder().await;
    trusted_hire(&realm).await;
    realm
        .write(&format!(
            "INSERT INTO agent (id, kind, status, at) VALUES (4, \
             'worker', 'live', 0); INSERT INTO worker (agent, \
             starter, hamlet, brief, tools, task, at) VALUES (4, 3, \
             '{}', 'Count the links', '[]', 1, 0)",
            hamlet.0
        ))
        .await;

    realm.ask(&revoke(THREE)).await.unwrap();
    until("the workspace holds the worker's reach", || {
        held(&realm, FOUR).is_some()
    })
    .await;
    until("the workspace holds the starter's reach", || {
        !footing(held(&realm, THREE)).network
    })
    .await;

    let starter = footing(held(&realm, THREE));
    assert_eq!(
        starter.trust,
        [Trust {
            conversation: ConversationId("c".to_owned()),
            edits: true,
            commands: false,
        }]
    );
    assert_eq!(
        held(&realm, FOUR).unwrap().reach,
        Reach::Worker {
            starter: Some(THREE),
            footing: Footing {
                trust: Vec::new(),
                ..starter
            },
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_revokes_the_agents_grants() {
    // agent 3 holds a grant and has a worker, 4. Its lead ends it:
    // the worker's footing loses the network, and a revoke finds no
    // grant.
    let (realm, hamlet) = hamlet_in_a_folder().await;
    trusted_hire(&realm).await;
    realm
        .write(&format!(
            "INSERT INTO agent (id, kind, status, at) VALUES (4, \
             'worker', 'live', 0); INSERT INTO worker (agent, \
             starter, hamlet, brief, tools, task, at) VALUES (4, 3, \
             '{}', 'Count the links', '[]', 1, 0)",
            hamlet.0
        ))
        .await;
    let end = End {
        hire: to(THREE),
        why: None,
    };

    let ended = realm.tool(MAYOR, &tool("e1", end)).await.unwrap();
    until("the worker's reach has no network", || {
        held(&realm, FOUR).is_some_and(|set| {
            set.reach
                .footing()
                .is_some_and(|footing| !footing.network)
        })
    })
    .await;
    let revoked = realm.ask(&revoke(THREE)).await.unwrap();

    assert_eq!(ended, Done::Done);
    assert_eq!(
        revoked,
        Done::Refused {
            why: RevokeRefusal::NoGrant { agent: THREE }
        }
    );
    realm.host.shutdown().await.unwrap();
}
