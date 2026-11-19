use hamlet_messages::realm::choices::SetNickname;
use hamlet_messages::realm::documents::{EditSettings, Setting};
use hamlet_messages::realm::oversight::{
    Permission, Revoke, RevokeRefusal,
};
use hamlet_messages::realm::workers::{StartWorker, Started};
use hamlet_messages::realm::{
    Asked, Done, End, HamletId, Named, NeedsChief,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{
    Footing, Mode, Need, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, SetReach,
};
use hamlet_protocol::client::{Choice, RequestId};

use super::chiefs::chief;
use super::{
    FOUR, MAYOR, Realm, THREE, answer, done_to, founded, hires,
    needs, to, tool, until, words, worker_shown,
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

fn no_grant(name: &str) -> RevokeRefusal {
    RevokeRefusal::NoGrant {
        name: name.to_owned(),
        permission: Permission::Network,
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
        .stopped_rows(|db| {
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
                why: no_grant("hire#3 (Hire 1)")
            },
            Done::Refused {
                why: no_grant("hire#2 (Mayor)")
            }
        )
    );
    assert_eq!(wanted(&realm, THREE).await, raised);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_takes_its_starters_footing() {
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
async fn an_always_on_a_workers_network_wait_gives_it_the_network() {
    let (realm, _) = hamlet_in_a_folder().await;
    hires(&realm, MAYOR, "h1").await;
    realm.live(THREE).await;
    let start = StartWorker {
        brief: "fetch the page".to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    };
    let started = realm.tool(THREE, &tool("w", start)).await.unwrap();
    assert_eq!(
        started,
        Started::Started {
            worker: worker_shown(FOUR)
        }
    );

    let asked = NeedsChief {
        needs: vec![Need::Network],
        ..needs(FOUR, "c1")
    };
    realm.call_as_workspace(&asked).await.unwrap();
    let always = realm.ask(&answer("1", Choice::Always, None)).await;

    assert_eq!(always.unwrap(), Done::Done);
    until("the worker's reach has the network", || {
        matches!(
            held(&realm, FOUR).map(|set| set.reach),
            Some(Reach::Worker { footing, .. }) if footing.network
        )
    })
    .await;
    assert!(!footing(held(&realm, THREE)).network);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_revokes_the_agents_grants() {
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
            set.reach.footing().is_none_or(|footing| !footing.network)
        })
    })
    .await;
    let revoked = realm.ask(&revoke(THREE)).await.unwrap();

    assert_eq!(ended, done_to(THREE));
    assert_eq!(
        revoked,
        Done::Refused {
            why: no_grant("hire#3 (Hire 1)")
        }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoke_of_no_grant_is_refused_in_todays_words() {
    let (realm, _) = hamlet_in_a_folder().await;
    hires(&realm, MAYOR, "h1").await;
    realm
        .write(
            "INSERT INTO agent (id, kind, status, at) VALUES (9, \
             'worker', 'live', 0); INSERT INTO worker (agent, \
             starter, brief, tools, task, at) VALUES (9, 3, 'dig', \
             '[]', 1, 0);",
        )
        .await;
    let nickname = SetNickname {
        hire: to(THREE),
        name: Some("Spade".to_owned()),
        why: None,
    };
    assert_eq!(chief(&realm, nickname).await, Done::Done);

    let spade = realm.ask(&revoke(THREE)).await.unwrap();
    let hand = realm.ask(&revoke(Hand::NUMBER)).await.unwrap();
    let worker = realm.ask(&revoke(AgentNumber(9))).await.unwrap();
    let allow = EditSettings {
        set: vec![Setting::AllowNet { allow: true }],
    };
    assert_eq!(chief(&realm, allow).await, Done::Done);
    let everyone = realm.ask(&revoke(THREE)).await.unwrap();
    let nobody = realm.ask(&revoke(AgentNumber(77))).await.unwrap();

    assert_eq!(
        [words(spade), words(hand), words(worker), words(everyone)],
        [
            "hire#3 (Spade) holds no grant of network.",
            "hand holds no grant of network.",
            "worker#9 holds no grant of network.",
            "hire#3 (Spade) holds no grant of network; the settings \
             allow every agent the network.",
        ]
    );
    assert_eq!(words(nobody), "no agent has the address `77`");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_revokes_a_workers_grants() {
    // As today's end of any agent (society/agents.rs:271-281).
    let (realm, _) = hamlet_in_a_folder().await;
    hires(&realm, MAYOR, "h1").await;
    realm.live(THREE).await;
    let start = StartWorker {
        brief: "fetch the page".to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    };
    let started = realm.tool(THREE, &tool("w", start)).await.unwrap();
    assert_eq!(
        started,
        Started::Started {
            worker: worker_shown(FOUR)
        }
    );
    let asked = NeedsChief {
        needs: vec![Need::Network],
        ..needs(FOUR, "c1")
    };
    realm.call_as_workspace(&asked).await.unwrap();
    let always = realm.ask(&answer("1", Choice::Always, None)).await;
    assert_eq!(always.unwrap(), Done::Done);
    let end = End {
        hire: Named::Worker { worker: FOUR },
        why: None,
    };
    assert_eq!(chief(&realm, end).await, Done::Done);
    let open = realm
        .stopped_rows(|db| {
            db.query_row(
                "SELECT COUNT(*) FROM grant WHERE agent = 4 AND \
                 revoked_at IS NULL",
                [],
                |row| row.get::<_, i64>(0),
            )
        })
        .await;
    assert_eq!(open, 0);
    let revoked = realm.ask(&revoke(FOUR)).await.unwrap();
    assert_eq!(
        revoked,
        Done::Refused {
            why: no_grant("worker#4")
        }
    );
    realm.host.shutdown().await.unwrap();
}
