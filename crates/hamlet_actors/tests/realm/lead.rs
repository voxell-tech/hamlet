use std::time::Duration;

use hamlet_messages::realm::{
    Asked, Done, End, FoundAnswer, HamletId, Hire, HireAnswer,
    HireRefusal, HowEnded, LeadRefusal, Named, Rank, Tool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{AgentNumber, CallId};
use hamlet_protocol::client::RequestId;

use super::{Callee, Realm, error_code, found, until};

const MAYOR: AgentNumber = AgentNumber(2);

async fn founded(realm: &Realm) -> HamletId {
    let answer = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Mill"),
        })
        .await
        .unwrap();
    let FoundAnswer::Founded { hamlet, .. } = answer else {
        panic!("the chief founds a hamlet: {answer:?}");
    };
    hamlet
}

fn under(agent: u32) -> Option<Named> {
    Some(Named::Agent {
        agent: AgentNumber(agent),
    })
}

fn hire(under: Option<Named>) -> Hire {
    Hire {
        hamlet: None,
        under,
        profile: None,
        purpose: None,
    }
}

fn end(agent: u32) -> End {
    End {
        hire: Named::Agent {
            agent: AgentNumber(agent),
        },
        why: None,
    }
}

fn tool<T>(call: &str, act: T) -> Tool<T> {
    Tool {
        call: CallId(call.to_owned()),
        act,
    }
}

fn hired<R>(agent: u32) -> HireAnswer<R> {
    HireAnswer::Hired {
        agent: AgentNumber(agent),
    }
}

fn not_a_lead<R>(agent: u32) -> LeadRefusal<R> {
    LeadRefusal::Rank(Rank::NotALead {
        hire: AgentNumber(agent),
    })
}

/// Whether the workspace stand-in was sent `agent`'s end.
fn ended_in_workspace(realm: &Realm, agent: AgentNumber) -> bool {
    realm
        .seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .any(|set| set.agent == agent && set.reach == Reach::Ended)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lead_hires_and_ends_only_under_itself() {
    // mayor 2 hires 3 and 4 under 3; 4 may neither hire under 3 nor
    // end it (`NotALead`); 3 ends 4.
    let realm = Realm::start().await;
    founded(&realm).await;

    let three = realm.tool(MAYOR, &tool("h1", hire(None))).await;
    let four = realm.tool(MAYOR, &tool("h2", hire(under(3)))).await;
    let four_hires = realm
        .tool(AgentNumber(4), &tool("h3", hire(under(3))))
        .await;
    let four_ends =
        realm.tool(AgentNumber(4), &tool("e1", end(3))).await;
    let three_ends =
        realm.tool(AgentNumber(3), &tool("e2", end(4))).await;
    until("the workspace holds the end of agent 4", || {
        ended_in_workspace(&realm, AgentNumber(4))
    })
    .await;

    assert_eq!(three.unwrap(), hired(3));
    assert_eq!(four.unwrap(), hired(4));
    assert_eq!(
        four_hires.unwrap(),
        HireAnswer::Refused {
            why: not_a_lead::<HireRefusal>(3)
        }
    );
    assert_eq!(
        four_ends.unwrap(),
        Done::Refused { why: not_a_lead(3) }
    );
    assert_eq!(three_ends.unwrap(), Done::Done);
    assert!(!ended_in_workspace(&realm, AgentNumber(3)));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_moves_the_children_and_drops_the_reach() {
    // end 3 with 4 under it: 4's lead is 2; the workspace stand-in
    // kept an `Ended` reach for 3.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    let chief_hires = async |under| {
        realm
            .ask(&Asked {
                id: RequestId::new(),
                act: Hire {
                    hamlet: Some(hamlet.clone()),
                    ..hire(under)
                },
            })
            .await
            .unwrap()
    };
    assert_eq!(chief_hires(None).await, hired(3));
    assert_eq!(chief_hires(under(3)).await, hired(4));
    realm.reach_held(AgentNumber(4)).await;

    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: End {
                why: Some("done".to_owned()),
                ..end(3)
            },
        })
        .await
        .unwrap();
    until("the workspace holds the end of agent 3", || {
        ended_in_workspace(&realm, AgentNumber(3))
    })
    .await;
    let (lead_of_4, (status, how, why)) = realm
        .rows(|db| {
            let lead = db.query_row(
                "SELECT lead FROM hire WHERE agent = 4",
                [],
                |row| row.get::<_, i64>(0),
            )?;
            let three = db.query_row(
                "SELECT status, ended_how, ended_why FROM agent \
                 WHERE id = 3",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )?;
            Ok((lead, three))
        })
        .await;

    assert_eq!(ended, Done::Done);
    assert_eq!(lead_of_4, 2);
    assert_eq!(
        (
            status.as_str(),
            serde_json::from_str::<HowEnded>(&how).unwrap(),
            why.as_deref()
        ),
        ("ended", HowEnded::ByTheChief, Some("done"))
    );
    assert!(!ended_in_workspace(&realm, AgentNumber(4)));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_of_a_hire_still_hiring_closes_its_steps() {
    // hold the branch; mayor 2 hires 3; the chief ends 3; release:
    // the workspace holds 3's end, `agent/3` is never made, and its
    // row is closed before the branch.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.reach_held(MAYOR).await;
    realm.seen.hold(Callee::Branch);

    let hired_3 = realm.tool(MAYOR, &tool("h1", hire(None))).await;
    let ended = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: end(3),
        })
        .await
        .unwrap();
    until("the workspace holds the end of agent 3", || {
        ended_in_workspace(&realm, AgentNumber(3))
    })
    .await;
    realm.seen.release(Callee::Branch);
    let (status, done, closed) = realm
        .rows(|db| {
            db.query_row(
                "SELECT agent.status, step.done, step.closed FROM \
                 agent JOIN step ON step.kind = 'hire' AND step.n = \
                 agent.id WHERE agent.id = 3",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                },
            )
        })
        .await;
    let made_3 = realm
        .seen
        .made
        .lock()
        .unwrap()
        .iter()
        .any(|(agent, _)| *agent == AgentNumber(3));

    assert_eq!(hired_3.unwrap(), hired(3));
    assert_eq!(ended, Done::Done);
    assert_eq!(status, "ended");
    assert!(closed);
    assert!(
        matches!(done.as_deref(), None | Some("profile")),
        "{done:?}"
    );
    assert!(!made_3);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_hire_hires_once() {
    // the same Tool<Hire> call id twice: one agent, the same answer.
    // The first drive fails at the branch, and a repeat drives the
    // row with no restart; the same id with another hire is refused.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.reach_held(MAYOR).await;
    realm.seen.fail_once.lock().unwrap().insert(Callee::Branch);
    let call = tool("h1", hire(None));

    let first = realm.tool(MAYOR, &call).await.unwrap();
    until("the profile fails the branch once", || {
        !realm
            .seen
            .fail_once
            .lock()
            .unwrap()
            .contains(&Callee::Branch)
    })
    .await;
    let other = realm
        .tool(
            MAYOR,
            &tool(
                "h1",
                Hire {
                    purpose: Some("other".to_owned()),
                    ..hire(None)
                },
            ),
        )
        .await;
    // A repeat while the failed drive still holds the row drives
    // nothing, so repeat until one does.
    let mut again = Vec::new();
    while !realm.holds_reach(AgentNumber(3)) {
        again.push(realm.tool(MAYOR, &call).await.unwrap());
        assert!(again.len() < 100, "no repeat drives the hire");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let made = realm
        .seen
        .made
        .lock()
        .unwrap()
        .iter()
        .filter(|(agent, _)| *agent == AgentNumber(3))
        .count();
    let (agents, hires) = realm
        .rows(|db| {
            let count = |table: &str| {
                db.query_row(
                    &format!("SELECT COUNT(*) FROM {table}"),
                    [],
                    |row| row.get::<_, i64>(0),
                )
            };
            Ok((count("agent")?, count("hire")?))
        })
        .await;

    assert_eq!(first, hired(3));
    assert!(!again.is_empty());
    assert!(again.iter().all(|answer| *answer == first), "{again:?}");
    assert_eq!(error_code(other), "message.id_reused");
    assert_eq!((agents, hires, made), (3, 2, 1));
    realm.host.shutdown().await.unwrap();
}
