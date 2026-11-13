//! The workspace through a host.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use hamlet_actor::Action;
use hamlet_actors::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion,
};
use hamlet_actors::workspace::{
    self, AgentNumber, ReachAnswer, ReachRefusal, ReachVersion,
    SetReach,
};
use hamlet_host::{Address, Cause, Host};
use hamlet_testing::Scratch;
use rusqlite::{Connection, OpenFlags};

const AGENT: AgentNumber = AgentNumber(7);

/// A host on `dir` with the workspace made.
async fn host(dir: &Path) -> Host {
    let host = Host::open(
        dir,
        HashMap::from([(
            workspace::KIND.to_owned(),
            workspace::factory(),
        )]),
    )
    .unwrap();
    host.get_or_start(workspace::KIND, workspace::MACHINE)
        .await
        .unwrap();
    host
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

/// Calls the workspace with a [`SetReach`] of agent 7.
async fn set_reach(
    host: &Host,
    version: u64,
    reach: Reach,
) -> ReachAnswer {
    let set = SetReach {
        agent: AGENT,
        version: ReachVersion(version),
        reach,
    };
    let reply = host
        .call(
            &Address::new(workspace::KIND, workspace::MACHINE),
            SetReach::NAME,
            hamlet_actor::action::encode_positional(&set).unwrap(),
            Duration::from_secs(10),
            Cause::root("test"),
        )
        .await
        .unwrap();
    ciborium::from_reader(reply.as_slice()).unwrap()
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

/// Version 3, then 2, then 3 again with another body: version 3's
/// first body stays in force, and each call answers version 3.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_reach_version_is_ignored() {
    let dir = Scratch::new("workspace-older-reach");
    let host = host(&dir).await;
    let mut answers = Vec::new();
    for (version, project) in
        [(3, None), (2, Some("/work/a")), (3, Some("/work/b"))]
    {
        answers.push(set_reach(&host, version, reach(project)).await);
    }
    host.shutdown().await.unwrap();

    assert_eq!(
        answers,
        vec![ReachAnswer::InForce(ReachVersion(3)); 3]
    );
    assert_eq!(reach_rows(&dir), [(7, 3, reach(None))]);
}

/// W2: a caller other than the realm is refused and the reach is
/// unchanged. The embedder's call here stands for any caller that is
/// not the realm.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "waits for the host to stamp a call with its caller, \
            which the workspace needs to tell the realm from any \
            other caller"]
async fn only_the_realm_sets_a_reach() {
    let dir = Scratch::new("workspace-reach-not-realm");
    let host = host(&dir).await;
    let answer = set_reach(&host, 1, reach(Some("/"))).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        answer,
        ReachAnswer::Refused(ReachRefusal::NotTheRealm)
    );
    assert!(reach_rows(&dir).is_empty());
}
