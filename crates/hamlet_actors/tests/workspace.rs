//! The workspace through a host, with the demo's stub realm.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hamlet_actors::workspace::reach::{
    Access, Footing, Gate, Mode, Op, Project, Reach, RealmFolder,
    Refusal, SecretsVersion, gate,
};
use hamlet_actors::workspace::{
    self, AgentNumber, ConversationId, ReachApplied, ReachVersion,
    SetReach,
};
use hamlet_host::{Address, Host};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};

#[path = "../../hamlet_host/examples/demo/realm.rs"]
mod realm;

/// How long a test waits for sends to be answered.
const PATIENCE: Duration = Duration::from_secs(20);

const AGENT: AgentNumber = AgentNumber(7);

fn kinds() -> HashMap<String, hamlet_actor_core::ActorFactory> {
    HashMap::from([
        (workspace::KIND.to_owned(), workspace::factory()),
        (realm::KIND.to_owned(), realm::factory(realm::KIND)),
        // The stub realm under another kind, whose sends the
        // workspace must not take.
        ("agent".to_owned(), realm::factory("agent")),
    ])
}

fn reach(project: Option<&str>) -> Reach {
    Reach::Agent(Footing {
        project: project.map(|folder| Project {
            folder: folder.into(),
            mode: Mode::Auto,
        }),
        network: false,
        trust: Vec::new(),
        secrets: SecretsVersion(1),
    })
}

fn set(version: u64, reach: Reach) -> SetReach {
    SetReach {
        agent: AGENT,
        version: ReachVersion(version),
        reach,
    }
}

/// Waits until `done` holds for the standing of the stub realm at
/// `at`, and returns that standing.
async fn settled(
    host: &Host,
    at: &Address,
    done: impl Fn(&realm::Standing) -> bool,
) -> realm::Standing {
    let begun = tokio::time::Instant::now();
    loop {
        let now = realm::standing(host, at).await.unwrap();
        if done(&now) {
            return now;
        }
        assert!(begun.elapsed() < PATIENCE, "not settled: {now:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The workspace's reach rows, `(agent, version, body)`.
fn reach_rows(dir: &Path) -> Vec<(i64, i64, Reach)> {
    let file = dir.join("actors/workspace/local.sqlite");
    let db = Connection::open_with_flags(
        file,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let mut read = db
        .prepare(
            "SELECT agent, version, body FROM reach ORDER BY agent",
        )
        .unwrap();
    read.query_map([], |row| {
        let body: Vec<u8> = row.get(2)?;
        Ok((
            row.get(0)?,
            row.get(1)?,
            ciborium::from_reader(body.as_slice()).unwrap(),
        ))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// Design 2.5, lane 6: version 3 then version 2. Version 3 stays in
/// force, and both are acknowledged with the version in force.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_reach_version_is_ignored() {
    let dir = Scratch::new("workspace-older-reach");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start(workspace::KIND, workspace::MACHINE)
        .await
        .unwrap();
    let at = Address::new(realm::KIND, realm::KEY);
    realm::put(&host, &at, set(3, reach(None))).await.unwrap();
    realm::put(&host, &at, set(2, reach(Some("/work/project"))))
        .await
        .unwrap();
    let standing =
        settled(&host, &at, |now| now.acks.len() == 2).await;
    host.shutdown().await.unwrap();

    let ack = |sent, in_force| ReachApplied {
        agent: AGENT,
        sent: ReachVersion(sent),
        in_force: ReachVersion(in_force),
    };
    assert_eq!(standing.acks, [ack(3, 3), ack(2, 3)]);
    let rows = reach_rows(&dir);
    assert_eq!(rows, [(7, 3, reach(None))]);
    let home = RealmFolder::new(&dir).unwrap();
    let path = PathBuf::from("/work/project/a.txt");
    assert_eq!(
        gate(
            &home,
            AGENT,
            Some(&rows[0].2),
            Some(SecretsVersion(1)),
            &Op::Change {
                conversation: &ConversationId("c".into()),
                path: &path,
            }
        ),
        Gate::Refuse(Refusal::OutsideReach {
            path,
            access: Access::Change,
        })
    );
}

/// W2: only the realm sets a reach. A `SetReach` from another kind is
/// answered, so it is not tried again, and changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_reach_from_another_kind_is_refused() {
    let dir = Scratch::new("workspace-reach-wrong-kind");
    let host = Host::open(&dir, kinds()).unwrap();
    host.get_or_start(workspace::KIND, workspace::MACHINE)
        .await
        .unwrap();
    let at = Address::new("agent", "9");
    realm::put(&host, &at, set(1, reach(Some("/"))))
        .await
        .unwrap();
    let standing =
        settled(&host, &at, |now| now.unanswered == 0).await;
    host.shutdown().await.unwrap();

    assert!(standing.acks.is_empty());
    assert!(reach_rows(&dir).is_empty());
}
