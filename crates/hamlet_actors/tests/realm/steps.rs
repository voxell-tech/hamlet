use std::time::Duration;

use hamlet_messages::agent::{
    AgentInput, AgentKind, Fill, Place, PlaceVersion,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::{Asked, FoundAnswer, ProfileId};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{
    Footing, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, ReachVersion, RealmTask, SetReach,
};
use hamlet_protocol::client::RequestId;

use super::{Callee, Realm, found, until};

const MAYOR: AgentNumber = AgentNumber(2);

/// The mayor's blank profile, which `Found` makes.
const BLANK: ProfileId = ProfileId(2);

/// What the stand-ins saw of the mayor's hire.
#[derive(Debug, Default, PartialEq)]
struct HireSeen {
    branches: Vec<Branch>,
    made: Vec<AgentInput>,
    fills: Vec<Fill>,
    reaches: Vec<SetReach>,
}

fn hire_seen(realm: &Realm) -> HireSeen {
    let seen = &realm.seen;
    HireSeen {
        branches: seen
            .branches
            .lock()
            .unwrap()
            .iter()
            .filter(|(profile, _)| *profile == BLANK)
            .map(|(_, branch)| branch.clone())
            .collect(),
        made: seen
            .made
            .lock()
            .unwrap()
            .iter()
            .filter(|(agent, _)| *agent == MAYOR)
            .map(|(_, input)| input.clone())
            .collect(),
        fills: seen
            .fills
            .lock()
            .unwrap()
            .iter()
            .filter(|(agent, _)| *agent == MAYOR)
            .map(|(_, fill)| fill.clone())
            .collect(),
        reaches: seen
            .reaches
            .lock()
            .unwrap()
            .iter()
            .filter(|reach| reach.agent == MAYOR)
            .cloned()
            .collect(),
    }
}

/// A realm whose mayor's hire failed at `callee`. The Hand is live
/// first, so its own steps take no failure.
async fn failed_once_at(callee: Callee) -> Realm {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm.seen.fail_once.lock().unwrap().insert(callee);
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();
    until("the stand-in fails its call", || {
        !realm.seen.fail_once.lock().unwrap().contains(&callee)
    })
    .await;
    realm
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_goes_live_through_its_steps() {
    // found; wait live(2); the stand-ins saw the profile made, one
    // Branch `hires/2`, `agent/2` made with its place and standing
    // task, a Fill of `hires/2`, and a SetReach for agent 2; its row
    // is closed.
    let realm = Realm::start().await;
    // The Hand going live moves the mayor's place.
    realm.live(Hand::NUMBER).await;

    let answer = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();
    realm.reach_held(MAYOR).await;
    let seen = hire_seen(&realm);
    realm.live(MAYOR).await;
    let (standing, closed) = realm
        .rows(|db| {
            let standing = db.query_row(
                "SELECT id FROM task WHERE agent = 2 AND origin = \
                 'standing'",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            let closed = db.query_row(
                "SELECT closed FROM step WHERE kind = 'hire' AND n \
                 = 2",
                [],
                |row| row.get::<_, bool>(0),
            )?;
            Ok((standing, closed))
        })
        .await;

    assert!(
        matches!(answer, FoundAnswer::Founded { mayor: MAYOR, .. }),
        "{answer:?}"
    );
    let branch = "hires/2".to_owned();
    assert_eq!(
        seen,
        HireSeen {
            // A call never makes its callee: a branch taken is a
            // profile made.
            branches: vec![Branch {
                name: branch.clone()
            }],
            made: vec![AgentInput {
                kind: AgentKind::Hire,
                task: RealmTask(standing.to_string()),
                place: Place {
                    version: PlaceVersion(1),
                    lead: None,
                    hires: Vec::new(),
                    roads: vec![Hand::NUMBER],
                    tools_off: Vec::new(),
                },
            }],
            fills: vec![Fill {
                profile: BLANK,
                branch,
            }],
            reaches: vec![SetReach {
                agent: MAYOR,
                version: ReachVersion(1),
                reach: Reach::Agent(Footing {
                    project: None,
                    network: false,
                    trust: Vec::new(),
                    secrets: SecretsVersion(1),
                }),
            }],
        }
    );
    assert!(closed, "a live hire's row is closed");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hiring_row_is_finished_at_the_realms_start() {
    // a profile that fails `Branch` once; found; restart; live(2).
    let realm = failed_once_at(Callee::Branch).await;
    // Room for a step that would be tried again before the start.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = hire_seen(&realm);

    let realm = realm.restart().await;
    realm.reach_held(MAYOR).await;
    let after = hire_seen(&realm);
    realm.live(MAYOR).await;

    assert_eq!(before, HireSeen::default());
    assert_eq!((after.branches.len(), after.made.len()), (1, 1));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_step_twice_leaves_one_hire() {
    // fail each step's callee once in turn; each run ends with one
    // agent 2, one branch, one SetReach kept.
    for callee in [
        Callee::Branch,
        Callee::Create,
        Callee::Fill,
        Callee::SetReach,
    ] {
        let realm = failed_once_at(callee).await.restart().await;
        realm.reach_held(MAYOR).await;
        let seen = hire_seen(&realm);
        realm.live(MAYOR).await;

        assert_eq!(
            [
                seen.branches.len(),
                seen.made.len(),
                seen.fills.len(),
                seen.reaches.len()
            ],
            [1; 4],
            "{callee:?} failed once: {seen:?}"
        );
        realm.host.shutdown().await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_done_again_leaves_one_hire() {
    // found; live(2); set its row back to before each step in turn,
    // with the agent hiring; each run ends with one live hire, one
    // agent made and its reach in force.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();
    realm.live(MAYOR).await;

    for done in [
        "NULL",
        "'profile'",
        "'branch'",
        "'create'",
        "'fill'",
        "'reach'",
    ] {
        realm
            .write(&format!(
                "UPDATE step SET done = {done}, closed = 0 WHERE \
                 kind = 'hire' AND n = 2; UPDATE agent SET status = \
                 'hiring' WHERE id = 2;"
            ))
            .await;
        realm.live(MAYOR).await;
        let (agents, hires, (status, wanted, applied)) = realm
            .rows(|db| {
                let count = |table: &str| {
                    db.query_row(
                        &format!("SELECT COUNT(*) FROM {table}"),
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                };
                let mayor = db.query_row(
                    "SELECT agent.status, reach.wanted, \
                     reach.applied FROM agent JOIN reach ON \
                     reach.agent = agent.id WHERE agent.id = 2",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    },
                )?;
                Ok((count("agent")?, count("hire")?, mayor))
            })
            .await;

        assert_eq!(
            (
                agents,
                hires,
                status.as_str(),
                hire_seen(&realm).made.len()
            ),
            (2, 1, "live", 1),
            "set back to {done}"
        );
        assert_eq!(Some(wanted), applied, "set back to {done}");
    }
    realm.host.shutdown().await.unwrap();
}
