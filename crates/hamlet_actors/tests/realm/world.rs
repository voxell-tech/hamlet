use std::time::{Duration, Instant};

use hamlet_actor::Action as _;
use hamlet_messages::address::{self, Party as _};
use hamlet_messages::agent::Input;
use hamlet_messages::agent::chat::Count;
use hamlet_messages::realm::budget::{BudgetLimit, Charge, Payer};
use hamlet_messages::realm::choices::{
    ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::client::{ReadWorld, SayAnswer};
use hamlet_messages::realm::documents::{EditSettings, Setting};
use hamlet_messages::realm::models::{Cause, TurnFailed};
use hamlet_messages::realm::oversight::{
    AskChief, Lapse, LapseReason, Object, Permission, Question,
    Revoke, Revoked,
};
use hamlet_messages::realm::parcels::{Measure, ParcelId};
use hamlet_messages::realm::workers::{
    Ended, SetAllowance, StartWorker, Started, WorkerDone,
};
use hamlet_messages::realm::world::{
    AgentSeen, GrantSeen, GrantSource, Lasting, ObjectionSource,
    ReachSeen, Revision, Seen, Starter, Status, WaitId, WaitKind,
    WaitSeen,
};
use hamlet_messages::realm::{
    Asked, ChiefRequest, Done, End, HireAnswer, HowEnded, Left,
    Limit, Named, NeedsChief, ReservedAct, SetLimits,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::files::EditDiff;
use hamlet_messages::workspace::reach::Need;
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, ReachVersion, RealmTask,
};
use hamlet_protocol::Effort;
use hamlet_protocol::client::{Choice, RequestId};
use serde::Serialize;
use tokio::task::JoinSet;

use super::asking::chief_task;
use super::stand_ins::{Callee, Forward};
use super::{
    DEADLINE, MAYOR, Realm, THREE, answer, call, decoded, done_to,
    founded, hamlet_with, hire, needs, say, sent, standing,
    submits_to, tell, to, tool, until,
};

pub(super) async fn seen_until(
    realm: &Realm,
    what: &str,
    holds: impl Fn(&Seen) -> bool,
) -> Seen {
    let begun = Instant::now();
    loop {
        let seen = realm.ask(&ReadWorld).await.unwrap();
        if holds(&seen) {
            return seen;
        }
        assert!(
            begun.elapsed() < DEADLINE,
            "never: {what}: {seen:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn agent(seen: &Seen, agent: AgentNumber) -> Option<&AgentSeen> {
    seen.agents.iter().find(|seen| seen.agent == agent)
}

async fn end_the_hire(realm: &Realm) {
    let ended = realm
        .tool(
            MAYOR,
            &tool(
                "e1",
                End {
                    hire: Named::Agent { agent: THREE },
                    why: None,
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(ended, done_to(THREE));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_world_answers_while_a_hire_waits_on_another_actor() {
    // hold Fill; found; once agent 2 is made, its steps wait on the
    // held Fill: ReadWorld answers at once, with agent 2 hiring.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    realm.seen.hold(Callee::Fill);
    founded(&realm).await;
    until("agent 2 is made", || {
        realm
            .seen
            .made
            .lock()
            .unwrap()
            .iter()
            .any(|(n, _)| *n == MAYOR)
    })
    .await;

    let read = tokio::time::timeout(
        Duration::from_secs(5),
        realm.ask(&ReadWorld),
    )
    .await;
    realm.seen.release(Callee::Fill);
    realm.live(MAYOR).await;

    let seen = read.expect("the world answers at once").unwrap();
    assert_eq!(
        agent(&seen, MAYOR).map(|mayor| &mayor.status),
        Some(&Status::Hiring)
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_revision_survives_a_restart() {
    // hire 3 under mayor 2; open a wait for the mayor; fail the
    // SetReach of 3's end, so its reach stays pending; read the
    // world; restart: the world is the same, its revision with it.
    let (realm, _) = hamlet_with(1).await;
    realm.call_as_workspace(&needs(MAYOR, "c1")).await.unwrap();
    realm.seen.fail_once(Callee::SetReach, THREE);
    end_the_hire(&realm).await;
    until("the workspace fails the end's SetReach", || {
        realm.seen.has_failed(Callee::SetReach, THREE)
    })
    .await;

    let before = realm.ask(&ReadWorld).await.unwrap();
    let realm = realm.restart().await;
    let after = realm.ask(&ReadWorld).await.unwrap();

    assert_ne!(before.revision, Revision(0));
    assert_eq!(
        before.waits,
        [WaitSeen {
            wait: WaitId(1),
            agent: MAYOR,
            subject: "rm -rf build".to_owned(),
            kind: WaitKind::Tool {
                call: CallId("c1".to_owned()),
                danger: Need::Command,
                always: Some(Lasting::Session),
                change: None,
            },
        }]
    );
    assert_eq!(
        agent(&before, THREE).map(|hire| &hire.reach),
        Some(&ReachSeen::Pending {
            applied: Some(ReachVersion(1)),
            wanted: ReachVersion(2),
        })
    );
    assert_eq!(after, before);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_revoke_shows_pending_until_the_workspace_answers() {
    // hold SetReach; end a hire: ReadWorld shows its reach Pending;
    // release SetReach: Applied.
    let (realm, _) = hamlet_with(1).await;
    realm.seen.hold(Callee::SetReach);

    end_the_hire(&realm).await;
    let pending = realm.ask(&ReadWorld).await.unwrap();
    realm.seen.release(Callee::SetReach);
    let applied =
        seen_until(&realm, "the end's reach applied", |seen| {
            agent(seen, THREE).is_some_and(|hire| {
                matches!(hire.reach, ReachSeen::Applied { .. })
            })
        })
        .await;

    assert_eq!(
        agent(&pending, THREE).map(|hire| &hire.reach),
        Some(&ReachSeen::Pending {
            applied: Some(ReachVersion(1)),
            wanted: ReachVersion(2),
        })
    );
    assert_eq!(
        agent(&applied, THREE).map(|hire| &hire.reach),
        Some(&ReachSeen::Applied {
            version: ReachVersion(2)
        })
    );
    assert!(applied.revision > pending.revision);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_burst_of_charges_moves_the_world_only_forward() {
    // found; 20 charges to the hamlet at once while the world is
    // read again and again: no read shows a lower revision or less
    // spend than the one before, and the last shows the hamlet and
    // all 20 charges.
    let (realm, hamlet) = hamlet_with(0).await;
    let task = standing(&realm, MAYOR);
    let pays = Payer::Hamlet {
        hamlet: hamlet.clone(),
    };
    let spent = |seen: &Seen| {
        seen.spend
            .iter()
            .find(|spend| spend.payer == pays)
            .map(|spend| spend.spent_micros)
    };

    let mut charges = JoinSet::new();
    for n in 0..20 {
        let host = realm.host.clone();
        let charge = Charge {
            call: CallId(format!("c{n}")),
            task: task.clone(),
            micros: 7,
            took_ms: 1,
            tokens: None,
        };
        let forward = Forward {
            name: Charge::NAME.to_owned(),
            args: hamlet_actor::action::encode_positional(&charge)
                .unwrap(),
        };
        charges.spawn(async move {
            let mayor = address::Agent(MAYOR);
            decoded::<()>(&call(&host, &mayor, &forward).await?)
        });
    }
    let mut reads = Vec::new();
    while !charges.is_empty() {
        let seen = realm.ask(&ReadWorld).await.unwrap();
        reads.push((seen.revision, spent(&seen)));
        while let Some(charged) = charges.try_join_next() {
            charged.unwrap().unwrap();
        }
    }
    let last = realm.ask(&ReadWorld).await.unwrap();
    reads.push((last.revision, spent(&last)));

    assert!(
        reads.is_sorted_by(|before, after| before.0 <= after.0
            && before.1 <= after.1),
        "{reads:?}"
    );
    assert_eq!(spent(&last), Some(20 * 7));
    assert_eq!(
        last.hamlets
            .iter()
            .map(|seen| (&seen.hamlet, seen.mayor))
            .collect::<Vec<_>>(),
        [(&hamlet, MAYOR)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_end_keeps_the_revision() {
    // the chief ends 3 and its ended reach is applied. The same
    // request again sends the reach again, which the workspace
    // already holds: no row changes, so the revision stays for the
    // second the repeat's push takes at most.
    let (realm, _) = hamlet_with(1).await;
    let end = Asked {
        id: RequestId::new(),
        act: End {
            hire: Named::Agent { agent: THREE },
            why: None,
        },
    };
    let ended = realm.ask(&end).await.unwrap();
    let applied =
        seen_until(&realm, "3's ended reach applied", |seen| {
            seen.agents.iter().any(|agent| {
                agent.agent == THREE
                    && agent.reach
                        == ReachSeen::Applied {
                            version: ReachVersion(2),
                        }
            })
        })
        .await;
    let repeated = realm.ask(&end).await.unwrap();
    let begun = Instant::now();
    let mut after = realm.ask(&ReadWorld).await.unwrap();
    while after.revision == applied.revision
        && begun.elapsed() < Duration::from_secs(1)
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
        after = realm.ask(&ReadWorld).await.unwrap();
    }

    assert_eq!([ended, repeated], [Done::Done, Done::Done]);
    assert_eq!(after.revision, applied.revision);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answered_wait_leaves_the_world() {
    // a wait for the Hand shows; the chief allows it: no wait shows.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;

    realm
        .call_as_workspace(&needs(Hand::NUMBER, "c1"))
        .await
        .unwrap();
    let open = realm.ask(&ReadWorld).await.unwrap();
    let allowed =
        realm.ask(&answer("1", Choice::Allow, None)).await.unwrap();
    let answered = realm.ask(&ReadWorld).await.unwrap();

    assert_eq!(allowed, Done::Done);
    assert_eq!(
        open.waits.iter().map(|wait| wait.wait).collect::<Vec<_>>(),
        [WaitId(1)]
    );
    assert_eq!(answered.waits, []);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_that_does_not_read_is_left_out_of_the_world() {
    let (realm, _) = hamlet_with(1).await;
    realm.call_as_workspace(&needs(THREE, "c1")).await.unwrap();
    let started = super::stopped_open(
        &realm.host,
        &realm.scratch,
        &address::Realm.address().unwrap(),
        rusqlite::OpenFlags::default(),
        |db| {
            db.execute_batch(
                "INSERT INTO wait (agent, kind, task, conversation, \
                 at) VALUES (3, 'question', 1, 'c', 1); INSERT INTO \
                 wait (agent, kind, call, subject, needs, \
                 reach_version, at) VALUES (3, 'tool', 'c2', 'ls', \
                 '[]', 1, 1)",
            )
        },
    )
    .await;
    let seen = realm.ask(&ReadWorld).await.unwrap();

    assert!(started.is_ok(), "{started:?}");
    assert_eq!(
        seen.waits.iter().map(|wait| wait.wait).collect::<Vec<_>>(),
        [WaitId(1)]
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_wait_holds_its_call() {
    let (realm, _) = hamlet_with(1).await;
    let written = super::stopped_open(
        &realm.host,
        &realm.scratch,
        &address::Realm.address().unwrap(),
        rusqlite::OpenFlags::default(),
        |db| {
            db.execute_batch(
                "INSERT INTO wait (agent, at) VALUES (3, 1)",
            )
        },
    )
    .await;
    assert!(written.is_err(), "a tool wait with no call was kept");
    realm.host.shutdown().await.unwrap();
}

fn worker_of(starter: Option<AgentNumber>) -> Option<Starter> {
    Some(
        starter
            .map_or(Starter::Chief, |agent| Starter::Agent { agent }),
    )
}

fn start(brief: &str) -> StartWorker {
    StartWorker {
        brief: brief.to_owned(),
        tools: Vec::new(),
        task: None,
        hamlet: None,
    }
}

fn network(agent: AgentNumber, call: &str) -> NeedsChief {
    NeedsChief {
        needs: vec![Need::Network],
        ..needs(agent, call)
    }
}

/// Hire 3 granted the network by a chief's always, with the reach it
/// gives applied.
async fn granted(realm: &Realm) -> Seen {
    realm
        .call_as_workspace(&network(THREE, "n1"))
        .await
        .unwrap();
    let always =
        realm.ask(&answer("1", Choice::Always, None)).await.unwrap();
    assert_eq!(always, Done::Done);
    seen_until(realm, "3's grant shows, its reach applied", |seen| {
        !seen.grants.is_empty()
            && agent(seen, THREE).is_some_and(|hire| {
                hire.reach
                    == ReachSeen::Applied {
                        version: ReachVersion(2),
                    }
            })
    })
    .await
}

/// A worker hire 3 starts, made and with its reach applied.
async fn started_by_three(realm: &Realm, call: &str) -> AgentNumber {
    let started = realm
        .tool(THREE, &tool(call, start("count the files")))
        .await
        .unwrap();
    let Started::Started { worker } = started else {
        panic!("hire 3 starts a worker: {started:?}");
    };
    realm.live(worker.agent).await;
    worker.agent
}

async fn chief<A: ChiefRequest>(realm: &Realm, act: A) -> A::Answer {
    realm
        .ask(&Asked {
            id: RequestId::new(),
            act,
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_world_shows_grants_waits_and_workers() {
    let (realm, hamlet) = hamlet_with(1).await;
    let mut revisions = Vec::new();

    let granted = granted(&realm).await;
    revisions.push(granted.revision);
    let four = started_by_three(&realm, "w1").await;
    let mut chiefs = start("tidy the shed");
    chiefs.hamlet = Some(hamlet.clone());
    let Started::Started { worker } = chief(&realm, chiefs).await
    else {
        panic!("the chief starts a worker");
    };
    let five = worker.agent;
    realm.live(five).await;
    revisions.push(realm.ask(&ReadWorld).await.unwrap().revision);
    let named = SetNickname {
        hire: Named::Agent { agent: THREE },
        name: Some("Spade".to_owned()),
        why: None,
    };
    let named = realm.tool(MAYOR, &tool("s1", named)).await.unwrap();
    revisions.push(realm.ask(&ReadWorld).await.unwrap().revision);
    let model = SetModel {
        of: ModelOf::Hire {
            hire: Named::Agent { agent: THREE },
        },
        model: Some("faux::faux-2".to_owned()),
        effort: Some(Effort::High),
        fast: Some(true),
        why: None,
    };
    let model = chief(&realm, model).await;
    revisions.push(realm.ask(&ReadWorld).await.unwrap().revision);
    let allowance = SetAllowance {
        hire: Named::Agent { agent: THREE },
        micros: Some(Count(500_000)),
        why: None,
    };
    let allowance = chief(&realm, allowance).await;
    let shown = realm.ask(&ReadWorld).await.unwrap();
    revisions.push(shown.revision);
    let revoked = chief(
        &realm,
        Revoke {
            agent: THREE,
            permission: Permission::Network,
        },
    )
    .await;
    let unrevoked = seen_until(&realm, "the grant is gone", |seen| {
        seen.grants.is_empty()
    })
    .await;
    revisions.push(unrevoked.revision);
    realm
        .tool(
            four,
            &WorkerDone {
                ended: Ended::Answered {
                    text: "Counted.".to_owned(),
                },
            },
        )
        .await
        .unwrap();
    let ended = seen_until(&realm, "worker 4 ended", |seen| {
        agent(seen, four)
            .is_some_and(|worker| worker.status != Status::Live)
    })
    .await;
    revisions.push(ended.revision);

    assert_eq!(
        granted.grants,
        [GrantSeen {
            agent: THREE,
            permission: Permission::Network,
            source: GrantSource::Grant,
        }]
    );
    assert_eq!(
        (named, model, allowance),
        (done_to(THREE), Done::Done, Done::Done)
    );
    assert!(
        matches!(revoked, Revoked::Revoked { .. }),
        "{revoked:?}"
    );
    let three = agent(&shown, THREE).unwrap();
    assert_eq!(
        (
            three.nickname.as_deref(),
            three.model.as_deref(),
            three.effort,
            three.fast,
            three.allowance_micros,
        ),
        (
            Some("Spade"),
            Some("faux::faux-2"),
            Some(Effort::High),
            Some(true),
            Some(500_000)
        )
    );
    let workers = |seen: &Seen| {
        [four, five].map(|worker| {
            agent(seen, worker).map(|seen| {
                (seen.hamlet.clone(), seen.worker.clone())
            })
        })
    };
    assert_eq!(
        workers(&shown),
        [
            Some((Some(hamlet.clone()), worker_of(Some(THREE)))),
            Some((Some(hamlet.clone()), worker_of(None))),
        ]
    );
    assert_eq!(
        agent(&ended, four)
            .map(|worker| (&worker.status, &worker.worker)),
        Some((
            &Status::Ended {
                left: Left {
                    how: HowEnded::Answered,
                    why: None,
                },
            },
            &worker_of(Some(THREE)),
        ))
    );
    assert!(revisions.is_sorted_by(|a, b| a < b), "{revisions:?}");
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_wait_kind_reads_in_the_world() {
    let (realm, hamlet) = hamlet_with(1).await;
    realm.call_as_workspace(&needs(THREE, "c1")).await.unwrap();
    let said = realm
        .ask(&say(&RequestId::new(), THREE, "Make the file."))
        .await
        .unwrap();
    let SayAnswer::Taken { task } = said else {
        panic!("hire 3 takes the chief's line: {said:?}");
    };
    let question = Question {
        question: "Which seed?".to_owned(),
        description: None,
        options: Vec::new(),
        text_input: true,
    };
    let asked = AskChief {
        task: task.clone(),
        conversation: ConversationId("c".to_owned()),
        message: None,
        questions: vec![question.clone()],
    };
    let asked = realm.tool(THREE, &tool("q1", asked)).await.unwrap();
    let objected = Object {
        task,
        conversation: ConversationId("c".to_owned()),
        reason: "the field is better left alone".to_owned(),
    };
    let objected =
        realm.tool(THREE, &tool("o1", objected)).await.unwrap();
    let reserved = SetLimits {
        hamlet: Some(hamlet),
        budget: Vec::new(),
        limits: vec![Limit::Reserved {
            act: ReservedAct::Hire,
            reserved: true,
        }],
    };
    assert_eq!(chief(&realm, reserved).await, Done::Done);
    let hired = realm.tool(MAYOR, &tool("h9", hire(None))).await;
    assert!(
        matches!(hired, Ok(HireAnswer::Refused { .. })),
        "{hired:?}"
    );
    let failed = TurnFailed {
        task: standing(&realm, MAYOR),
        conversation: ConversationId("c".to_owned()),
        words: "the model fell over".to_owned(),
        cause: Cause::Model,
    };
    realm.tool(MAYOR, &failed).await.unwrap();
    let committed = realm.ask(&ReadWorld).await.unwrap();
    let mut sending = Vec::new();
    ciborium::into_writer(
        &Sending {
            parcel: ParcelId(1),
            measure: Measure {
                files: 1_001,
                bytes: 1_001,
            },
        },
        &mut sending,
    )
    .unwrap();
    realm
        .write(&format!(
            "INSERT INTO wait (agent, kind, conversation, subject, \
             detail, at) VALUES (2, 'send', 'c', 'too many files', \
             X'{}', 1)",
            hex(&sending)
        ))
        .await;

    let seen = realm.ask(&ReadWorld).await.unwrap();
    let realm = realm.restart().await;
    let restarted = realm.ask(&ReadWorld).await.unwrap();

    assert_eq!((asked, objected), (Done::Done, Done::Done));
    let shown = seen
        .waits
        .iter()
        .map(|wait| (wait.wait.0, wait.agent, wait.kind.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        shown,
        [
            (
                1,
                THREE,
                WaitKind::Tool {
                    call: CallId("c1".to_owned()),
                    danger: Need::Command,
                    always: Some(Lasting::Session),
                    change: None,
                }
            ),
            (
                2,
                THREE,
                WaitKind::Question {
                    message: None,
                    questions: vec![question],
                }
            ),
            (
                3,
                THREE,
                WaitKind::Objection {
                    reason: "the field is better left alone"
                        .to_owned(),
                    source: ObjectionSource::Agent,
                }
            ),
            (4, MAYOR, WaitKind::Reserved),
            (5, MAYOR, WaitKind::Fault),
            (
                6,
                MAYOR,
                WaitKind::Send {
                    measure: Measure {
                        files: 1_001,
                        bytes: 1_001,
                    },
                }
            ),
        ]
    );
    let subjects = seen
        .waits
        .iter()
        .map(|wait| wait.subject.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        [subjects[0], subjects[1], subjects[4], subjects[5]],
        [
            "rm -rf build",
            "Which seed?",
            "the model fell over",
            "too many files"
        ]
    );
    assert!(
        subjects[2]
            .ends_with("objects: the field is better left alone"),
        "{}",
        subjects[2]
    );
    assert!(subjects[3].starts_with("hire {"), "{}", subjects[3]);
    assert_eq!(committed.waits[..], seen.waits[..5]);
    assert_eq!(restarted.waits, seen.waits);
    realm.host.shutdown().await.unwrap();
}

/// A send wait's detail, as the parcel's step keeps it.
#[derive(Serialize)]
struct Sending {
    parcel: ParcelId,
    measure: Measure,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unrelated_commit_leaves_the_world_as_it_was() {
    let (realm, _) = hamlet_with(1).await;
    granted(&realm).await;
    let said = realm
        .ask(&say(&RequestId::new(), THREE, "Make the file."))
        .await
        .unwrap();
    let SayAnswer::Taken { task } = said else {
        panic!("hire 3 takes the chief's line: {said:?}");
    };
    let asked = tool(
        "q1",
        AskChief {
            task,
            conversation: ConversationId("c".to_owned()),
            message: None,
            questions: vec![Question {
                question: "Which seed?".to_owned(),
                description: None,
                options: Vec::new(),
                text_input: true,
            }],
        },
    );
    realm.tool(THREE, &asked).await.unwrap();
    let four = started_by_three(&realm, "w1").await;
    let done = WorkerDone {
        ended: Ended::Answered {
            text: "Counted.".to_owned(),
        },
    };
    realm.tool(four, &done).await.unwrap();
    let before =
        seen_until(&realm, "worker 4's end applied", |seen| {
            agent(seen, four).is_some_and(|worker| {
                worker.status != Status::Live
                    && worker.reach
                        == ReachSeen::Applied {
                            version: ReachVersion(2),
                        }
            })
        })
        .await;

    let told = realm
        .tool(
            MAYOR,
            &tool(
                "t1",
                tell(Named::Agent { agent: THREE }, "Hello."),
            ),
        )
        .await
        .unwrap();
    until("the letter reaches 3", || {
        submits_to(&realm, THREE).iter().any(|submit| {
            matches!(submit.input, Input::Letter { .. })
        })
    })
    .await;
    let asked_again = realm.tool(THREE, &asked).await.unwrap();
    realm.tool(four, &done).await.unwrap();
    let revoked = chief(
        &realm,
        Revoke {
            agent: MAYOR,
            permission: Permission::Network,
        },
    )
    .await;
    let after = realm.ask(&ReadWorld).await.unwrap();

    assert_eq!((told, asked_again), (sent(THREE), Done::Done));
    assert!(
        matches!(revoked, Revoked::NoGrant { .. }),
        "{revoked:?}"
    );
    assert_eq!((before.grants.len(), before.waits.len()), (1, 1));
    assert_eq!(after, before);
    realm.host.shutdown().await.unwrap();
}

/// The world once it holds still: the same read twice, 400 ms apart.
async fn settled(realm: &Realm) -> Seen {
    let mut last = realm.ask(&ReadWorld).await.unwrap();
    loop {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let now = realm.ask(&ReadWorld).await.unwrap();
        if now == last {
            return now;
        }
        last = now;
    }
}

/// What `seen` shows, its revision aside.
fn shown(seen: &Seen) -> Seen {
    Seen {
        revision: Revision(0),
        ..seen.clone()
    }
}

/// The world after a step, checked: the copy is a restart's full
/// read, and the revision moved only if what the world shows changed.
async fn stepped(
    realm: Realm,
    step: &str,
    before: &Seen,
    faults: &mut Vec<String>,
) -> (Realm, Seen) {
    let after = settled(&realm).await;
    let realm = realm.restart().await;
    let restarted = settled(&realm).await;
    if after != restarted {
        faults.push(format!(
            "{step}: the copy {after:?} is not a restart's read \
             {restarted:?}"
        ));
    }
    let changed = shown(&after) != shown(before);
    let moved = after.revision != before.revision;
    if changed != moved {
        faults.push(format!(
            "{step}: shown changed {changed}, revision moved {moved}"
        ));
    }
    (realm, restarted)
}

fn asked(
    agent: AgentNumber,
    call: &str,
    need: Need,
    task: Option<&RealmTask>,
) -> NeedsChief {
    NeedsChief {
        realm_task: task.cloned(),
        needs: vec![need],
        subject: format!("run {call}"),
        ..needs(agent, call)
    }
}

fn wait_of(seen: &Seen, kind: fn(&WaitKind) -> bool) -> String {
    seen.waits
        .iter()
        .find(|wait| kind(&wait.kind))
        .unwrap_or_else(|| panic!("no such wait: {:?}", seen.waits))
        .wait
        .0
        .to_string()
}

fn allowance(micros: Option<u32>) -> SetAllowance {
    SetAllowance {
        hire: Named::Agent { agent: THREE },
        micros: micros.map(|micros| Count(micros.into())),
        why: None,
    }
}

fn lapse(why: LapseReason) -> Lapse {
    Lapse {
        conversation: Some(ConversationId("c".to_owned())),
        why,
    }
}

fn failed(task: RealmTask) -> TurnFailed {
    TurnFailed {
        task,
        conversation: ConversationId("c".to_owned()),
        words: "the model fell over".to_owned(),
        cause: Cause::Model,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn each_writer_moves_the_revision_only_with_the_world() {
    let (mut realm, hamlet) = hamlet_with(1).await;
    let mut faults = Vec::new();
    let mut before = settled(&realm).await;
    macro_rules! step {
        ($step:expr, $body:expr) => {{
            $body;
            (realm, before) =
                stepped(realm, $step, &before, &mut faults).await;
        }};
    }
    let is_tool =
        |kind: &WaitKind| matches!(kind, WaitKind::Tool { .. });
    let question = || Question {
        question: "Which?".to_owned(),
        description: None,
        options: Vec::new(),
        text_input: true,
    };

    step!("a network wait opens", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "n1",
                Need::Network,
                None,
            ))
            .await
            .unwrap();
    });
    step!("always grants", {
        let wait = wait_of(&before, is_tool);
        realm
            .ask(&answer(&wait, Choice::Always, None))
            .await
            .unwrap();
    });
    step!("a command wait opens", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "t1",
                Need::Command,
                None,
            ))
            .await
            .unwrap();
    });
    step!("always trusts", {
        let wait = wait_of(&before, is_tool);
        realm
            .ask(&answer(&wait, Choice::Always, None))
            .await
            .unwrap();
    });
    step!("a resume drops the trust", {
        realm
            .tool(THREE, &lapse(LapseReason::Resumed))
            .await
            .unwrap();
    });
    step!("a resume with nothing open", {
        realm
            .tool(THREE, &lapse(LapseReason::Resumed))
            .await
            .unwrap();
    });
    let revoke = Revoke {
        agent: THREE,
        permission: Permission::Network,
    };
    step!("revoke", chief(&realm, revoke.clone()).await);
    step!("revoke again", chief(&realm, revoke.clone()).await);
    let four = std::cell::Cell::new(AgentNumber(0));
    step!("3 starts a worker", {
        four.set(started_by_three(&realm, "w1").await);
    });
    step!("a chief starts a worker", {
        chief(
            &realm,
            StartWorker {
                hamlet: Some(hamlet.clone()),
                ..start("tidy")
            },
        )
        .await;
    });
    let nickname = SetNickname {
        hire: Named::Agent { agent: THREE },
        name: Some("Spade".to_owned()),
        why: None,
    };
    step!("nickname", {
        realm
            .tool(MAYOR, &tool("s1", nickname.clone()))
            .await
            .unwrap();
    });
    step!("nickname again", {
        realm
            .tool(MAYOR, &tool("s2", nickname.clone()))
            .await
            .unwrap();
    });
    let model = SetModel {
        of: ModelOf::Hire {
            hire: Named::Agent { agent: THREE },
        },
        model: Some("faux::faux-2".to_owned()),
        effort: Some(Effort::High),
        fast: Some(true),
        why: None,
    };
    step!("model", chief(&realm, model.clone()).await);
    step!("model again", chief(&realm, model.clone()).await);
    step!("a hamlet's model", {
        chief(
            &realm,
            SetModel {
                of: ModelOf::Hamlet {
                    hamlet: hamlet.clone(),
                },
                ..model.clone()
            },
        )
        .await;
    });
    step!("allowance", chief(&realm, allowance(Some(500_000))).await);
    step!("allowance again", {
        chief(&realm, allowance(Some(500_000))).await;
    });
    step!("allowance cleared", chief(&realm, allowance(None)).await);
    step!("allowance cleared again", {
        chief(&realm, allowance(None)).await;
    });
    let mut task: RealmTask;
    step!("a chief's line", task = chief_task(&realm, THREE).await);
    step!("a question opens", {
        let ask = AskChief {
            task: task.clone(),
            conversation: ConversationId("c".to_owned()),
            message: None,
            questions: vec![question()],
        };
        realm.tool(THREE, &tool("q1", ask)).await.unwrap();
    });
    step!("the question is answered", {
        let wait = wait_of(&before, |kind| {
            matches!(kind, WaitKind::Question { .. })
        });
        realm
            .ask(&answer(&wait, Choice::Reply, Some("oats")))
            .await
            .unwrap();
    });
    step!("an objection opens", {
        let object = Object {
            task: task.clone(),
            conversation: ConversationId("c".to_owned()),
            reason: "leave it".to_owned(),
        };
        realm.tool(THREE, &tool("o1", object)).await.unwrap();
    });
    step!("go", {
        let wait = wait_of(&before, |kind| {
            matches!(kind, WaitKind::Objection { .. })
        });
        realm.ask(&answer(&wait, Choice::Go, None)).await.unwrap();
    });
    let limits = |limits| SetLimits {
        hamlet: Some(hamlet.clone()),
        budget: Vec::new(),
        limits,
    };
    step!("the limits reserve hire", {
        chief(
            &realm,
            limits(vec![Limit::Reserved {
                act: ReservedAct::Hire,
                reserved: true,
            }]),
        )
        .await;
    });
    step!("a reserved hire opens", {
        realm.tool(MAYOR, &tool("h9", hire(None))).await.unwrap();
    });
    step!("the reserved hire is denied", {
        let wait = wait_of(&before, |kind| {
            matches!(kind, WaitKind::Reserved)
        });
        realm.ask(&answer(&wait, Choice::Deny, None)).await.unwrap();
    });
    step!("a fault opens", {
        realm
            .tool(MAYOR, &failed(standing(&realm, MAYOR)))
            .await
            .unwrap();
    });
    step!("the fault is dismissed", {
        let wait =
            wait_of(&before, |kind| matches!(kind, WaitKind::Fault));
        realm
            .ask(&answer(&wait, Choice::Dismiss, None))
            .await
            .unwrap();
    });
    step!("one refusal at most", {
        chief(&realm, limits(vec![Limit::Refusals { most: 1 }]))
            .await;
    });
    step!("a second line", task = chief_task(&realm, THREE).await);
    step!("a command wait on a task opens", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "u1",
                Need::Command,
                Some(&task),
            ))
            .await
            .unwrap();
    });
    step!("refused", {
        let wait = wait_of(&before, is_tool);
        realm
            .ask(&answer(&wait, Choice::Deny, Some("no")))
            .await
            .unwrap();
    });
    step!("asked past the count", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "u2",
                Need::Command,
                Some(&task),
            ))
            .await
            .unwrap();
    });
    let budget = SetLimits {
        hamlet: Some(hamlet.clone()),
        budget: vec![BudgetLimit::Cost { micros: 1_000_000 }],
        limits: Vec::new(),
    };
    step!("a budget", chief(&realm, budget.clone()).await);
    step!(
        "the same budget again",
        chief(&realm, budget.clone()).await
    );
    step!("worker 4 is done", {
        let done = WorkerDone {
            ended: Ended::Answered {
                text: "Counted.".to_owned(),
            },
        };
        realm.tool(four.get(), &done).await.unwrap();
    });
    step!("a tool wait opens in c", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "l1",
                Need::Command,
                None,
            ))
            .await
            .unwrap();
    });
    step!("a fault opens in c", {
        realm
            .tool(THREE, &failed(standing(&realm, THREE)))
            .await
            .unwrap();
    });
    step!("a stop lapses both", {
        realm
            .tool(THREE, &lapse(LapseReason::Stopped))
            .await
            .unwrap();
    });
    step!("a tool wait opens before the end", {
        realm
            .call_as_workspace(&asked(
                THREE,
                "x1",
                Need::Command,
                None,
            ))
            .await
            .unwrap();
    });
    step!("3 ends", {
        chief(
            &realm,
            End {
                hire: Named::Agent { agent: THREE },
                why: None,
            },
        )
        .await;
    });

    assert!(faults.is_empty(), "{}", faults.join("\n"));
    assert!(agent(&before, THREE).is_some_and(|hire| matches!(
        hire.status,
        Status::Ended { .. }
    )));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_world_shows_the_model_each_agent_runs() {
    let (realm, _) = hamlet_with(1).await;
    let set = chief(
        &realm,
        EditSettings {
            set: vec![Setting::Model {
                model: "faux::faux-1".into(),
            }],
        },
    )
    .await;
    let chosen = chief(
        &realm,
        SetModel {
            of: ModelOf::Hire { hire: to(THREE) },
            model: Some("faux::faux-2".to_owned()),
            effort: None,
            fast: None,
            why: None,
        },
    )
    .await;

    let seen =
        seen_until(&realm, "each agent runs a model", |seen| {
            [MAYOR, THREE].iter().all(|&of| {
                agent(seen, of)
                    .is_some_and(|seen| seen.runs.is_some())
            })
        })
        .await;

    assert_eq!((set, chosen), (Done::Done, Done::Done));
    let runs = |of| {
        agent(&seen, of)
            .map(|seen| (seen.runs.clone(), seen.model.clone()))
    };
    assert_eq!(
        runs(MAYOR),
        Some((Some("faux::faux-1".into()), None))
    );
    assert_eq!(
        runs(THREE),
        Some((
            Some("faux::faux-2".into()),
            Some("faux::faux-2".into())
        ))
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_wait_shows_its_change_after_a_restart() {
    let (realm, _) = hamlet_with(0).await;
    let change = EditDiff {
        diff: "1 + a".to_owned(),
        patch: "@@ -0,0 +1 @@\n+a\n".to_owned(),
        first_changed_line: Some(1),
    };
    realm
        .call_as_workspace(&NeedsChief {
            needs: vec![Need::Edit],
            change: Some(change.clone()),
            ..needs(MAYOR, "c1")
        })
        .await
        .unwrap();

    let realm = realm.restart().await;
    let seen = realm.ask(&ReadWorld).await.unwrap();

    assert_eq!(
        seen.waits
            .iter()
            .map(|wait| wait.kind.clone())
            .collect::<Vec<_>>(),
        [WaitKind::Tool {
            call: CallId("c1".to_owned()),
            danger: Need::Edit,
            always: Some(Lasting::Session),
            change: Some(change),
        }]
    );
    realm.host.shutdown().await.unwrap();
}
