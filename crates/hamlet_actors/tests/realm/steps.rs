use std::time::Duration;

use hamlet_messages::agent::{
    AgentInput, AgentKind, Author, Fill, Input, Place, PlaceVersion,
};
use hamlet_messages::profile::Branch;
use hamlet_messages::realm::ProfileId;
use hamlet_messages::realm::documents::ReadCopies;
use hamlet_messages::realm::workers::{StartWorker, Started};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::{
    Footing, Reach, SecretsVersion,
};
use hamlet_messages::workspace::{
    AgentNumber, ReachVersion, RealmTask, SetReach,
};

use super::stand_ins::Callee;
use super::{
    FOUR, MAYOR, Realm, THREE, founded, hamlet_with, hires,
    made_until, seen_submits, submits_to, tell, to, told_of_failed,
    tool, until, words, worker_shown,
};

const BLANK: ProfileId = ProfileId(2);

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

async fn failed_once_at(callee: Callee) -> Realm {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm.seen.fail_once(callee, MAYOR);
    founded(&realm).await;
    until("the stand-in fails its call", || {
        realm.seen.has_failed(callee, MAYOR)
    })
    .await;
    realm
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_goes_live_through_its_steps() {
    // found; wait live(2); the stand-ins saw the profile made, one
    // Branch `hires/2`, `agent/2` made with its place and standing
    // task, a Fill of `hires/2`, and a SetReach for agent 2.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;

    founded(&realm).await;
    realm.reach_held(MAYOR).await;
    let seen = hire_seen(&realm);
    realm.live(MAYOR).await;
    let copies =
        realm.tool(MAYOR, &ReadCopies::default()).await.unwrap();
    let (standing, closed) = realm
        .stopped_rows(|db| {
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

    assert_eq!(
        seen,
        HireSeen {
            branches: vec![Branch {
                name: "hires/2".into(),
                from: None,
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
                    folder: None,
                    manual: false,
                    agrees_roads: false,
                },
                brief: None,
                copies,
            }],
            fills: vec![Fill { profile: BLANK }],
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
    let (realm, _) = hamlet_with(0).await;

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
            .stopped_rows(|db| {
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

/// The agent rows that are still hiring, and how `agent` ended.
pub(super) async fn hiring_and_how(
    realm: &Realm,
    agent: AgentNumber,
) -> (i64, Option<String>) {
    realm
        .stopped_rows(|db| {
            let hiring = db.query_row(
                "SELECT COUNT(*) FROM agent WHERE status = 'hiring'",
                [],
                |row| row.get(0),
            )?;
            let how = db.query_row(
                "SELECT ended_how FROM agent WHERE id = ?1",
                [agent.0],
                |row| row.get(0),
            )?;
            Ok((hiring, how))
        })
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_agent_cannot_be_made_ends_as_failed() {
    // hire 3's agent refuses to be made: the hire ends as failed,
    // the mayor's letter to it is refused in the words of an end,
    // and no agent is left hiring.
    let (realm, _) = hamlet_with(0).await;
    realm.seen.fail_for_good(Callee::Create, THREE);
    hires(&realm, MAYOR, "h1").await;

    let told = realm
        .tool(MAYOR, &tool("t1", tell(to(THREE), "dig the well")))
        .await
        .unwrap();

    assert!(words(told).contains("its task failed"));
    assert!(submits_to(&realm, THREE).is_empty());
    let (hiring, how) = hiring_and_how(&realm, THREE).await;
    assert_eq!(hiring, 0);
    assert_eq!(how.as_deref(), Some(r#"{"type":"failed"}"#));
    let told = told_of_failed(&realm, MAYOR, THREE).await;
    assert_eq!(told, format!("{HIRE_3_FAILED}{PLAIN}"));
    realm.host.shutdown().await.unwrap();
}

const HIRE_3_FAILED: &str =
    "`hire#3 (Hire 1)` has left: its task failed: ";

async fn told_of_a_hire_failing_at(callee: Callee) -> String {
    let (realm, _) = hamlet_with(0).await;
    realm.seen.fail_for_good(callee, THREE);
    hires(&realm, MAYOR, "h1").await;

    let told = told_of_failed(&realm, MAYOR, THREE).await;

    let (hiring, how) = hiring_and_how(&realm, THREE).await;
    assert_eq!(hiring, 0);
    assert_eq!(how.as_deref(), Some(r#"{"type":"failed"}"#));
    realm.host.shutdown().await.unwrap();
    told
}

/// What the lead reads when a step's callee fails with an error of
/// its own and no refusal's words.
const PLAIN: &str = "An internal error occurred";

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_branch_cannot_be_made_ends_and_its_lead_hears_why()
 {
    let told = told_of_a_hire_failing_at(Callee::Branch).await;
    assert_eq!(told, format!("{HIRE_3_FAILED}{PLAIN}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_files_cannot_be_filled_ends_and_its_lead_hears_why()
 {
    let told = told_of_a_hire_failing_at(Callee::Fill).await;
    assert_eq!(told, format!("{HIRE_3_FAILED}{PLAIN}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_reach_is_refused_ends_and_its_lead_hears_why() {
    let told = told_of_a_hire_failing_at(Callee::SetReach).await;
    assert_eq!(told, format!("{HIRE_3_FAILED}{PLAIN}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_whose_agent_cannot_be_made_ends_and_its_starter_hears()
 {
    let (realm, _) = hamlet_with(1).await;
    realm.seen.fail_for_good(Callee::Create, FOUR);

    let started = realm
        .tool(
            THREE,
            &tool(
                "w1",
                StartWorker {
                    brief: "count the files".to_owned(),
                    tools: Vec::new(),
                    task: None,
                    hamlet: None,
                },
            ),
        )
        .await
        .unwrap();
    let told = seen_submits(&realm, THREE, 1).await;

    assert_eq!(
        started,
        Started::Started {
            worker: worker_shown(FOUR)
        }
    );
    assert_eq!(
        told[0].input,
        Input::WorkerAnswer {
            from: Author {
                agent: FOUR,
                name: Some(worker_shown(FOUR).name),
            },
            text: "It gave no answer.".to_owned(),
            conversation: None,
        }
    );
    let (_, how) = hiring_and_how(&realm, FOUR).await;
    assert_eq!(how.as_deref(), Some(r#"{"type":"failed"}"#));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_whose_agent_is_not_ready_is_made_once_it_is() {
    let (realm, _) = hamlet_with(0).await;
    realm.seen.fail_once(Callee::Create, THREE);

    hires(&realm, MAYOR, "h1").await;
    made_until(&realm, THREE).await;
    realm.live(THREE).await;

    let (hiring, how) = hiring_and_how(&realm, THREE).await;
    assert_eq!((hiring, how), (0, None));
    realm.host.shutdown().await.unwrap();
}
