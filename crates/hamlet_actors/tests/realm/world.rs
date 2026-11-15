use std::time::{Duration, Instant};

use hamlet_actor::Action as _;
use hamlet_messages::address;
use hamlet_messages::realm::budget::{Charge, Payer};
use hamlet_messages::realm::client::ReadWorld;
use hamlet_messages::realm::world::{
    AgentSeen, ReachSeen, Revision, Seen, Status, WaitId, WaitSeen,
};
use hamlet_messages::realm::{Asked, Done, End, Named};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::{AgentNumber, CallId, ReachVersion};
use hamlet_protocol::client::{Choice, RequestId};
use tokio::task::JoinSet;

use super::{
    Callee, DEADLINE, Forward, MAYOR, Realm, THREE, answer, call,
    decoded, founded, hamlet_with, needs, standing, tool, until,
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
    assert_eq!(ended, Done::Done);
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
            call: CallId("c1".to_owned()),
            subject: "rm -rf build".to_owned(),
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
