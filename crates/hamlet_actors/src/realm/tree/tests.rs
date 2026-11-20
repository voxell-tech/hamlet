use hamlet_actor::BindParam;
use hamlet_messages::realm::{HowEnded, Left};

use super::super::testing::Started;
use super::super::versions;
use super::*;

fn agents(nodes: &[Node]) -> Vec<AgentNumber> {
    nodes.iter().map(|node| node.agent).collect()
}

/// An ending by a lead keeps the lead's number, and reads back
/// naming it.
#[tokio::test(flavor = "multi_thread")]
async fn an_end_by_a_lead_is_kept_by_the_leads_number() {
    let started = Started::new().await;
    let hamlet = HamletId("h".to_owned());
    started.found(&hamlet, AgentNumber(2)).await;
    started
        .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
        .await;
    let rows = started.rows();
    let by_three = r#"{"type":"by_a_lead","lead":3}"#;
    let how = HowEnded::ByALead {
        lead: record::agent_name(&rows, AgentNumber(3))
            .await
            .unwrap(),
    };

    let kept: serde_json::Value =
        serde_json::from_str(&kept_how(&how).unwrap()).unwrap();
    assert_eq!(read_how(&rows, by_three).await.unwrap(), how);
    assert_eq!(
        kept,
        serde_json::from_str::<serde_json::Value>(by_three).unwrap()
    );
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lineage_reads_the_leads_up_to_the_mayor() {
    let started = Started::new().await;
    let hamlet = HamletId("h".to_owned());
    started.found(&hamlet, AgentNumber(2)).await;
    started
        .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
        .await;
    started
        .hire(&hamlet, AgentNumber(4), Some(AgentNumber(3)))
        .await;
    let rows = started.rows();

    let hire = lineage(&rows, AgentNumber(4)).await.unwrap().unwrap();
    assert_eq!(agents(&hire.leads), [AgentNumber(3), AgentNumber(2)]);
    assert!(hire.is_led_by(AgentNumber(2)));
    assert!(hire.is_led_by(Hand::NUMBER));
    assert!(!hire.is_led_by(AgentNumber(4)));
    assert!(!hire.is_mayor());
    let mayor =
        lineage(&rows, AgentNumber(2)).await.unwrap().unwrap();
    assert!(mayor.is_mayor());
    assert_eq!(agents(&mayor.hires), [AgentNumber(3)]);
    assert_eq!(hire.hamlet, mayor.hamlet);
    let hand = lineage(&rows, Hand::NUMBER).await.unwrap().unwrap();
    assert_eq!(
        (hand.agent.kind, hand.hamlet),
        (AgentKind::Hand, None)
    );
    assert_eq!(lineage(&rows, AgentNumber(9)).await.unwrap(), None);
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_ended_lead_leads_nothing_and_holds_no_place_in_the_tier()
{
    let started = Started::new().await;
    let hamlet = HamletId("h".to_owned());
    started.found(&hamlet, AgentNumber(2)).await;
    started
        .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
        .await;
    started
        .hire(&hamlet, AgentNumber(4), Some(AgentNumber(3)))
        .await;
    let left = Left {
        how: HowEnded::ByTheChief,
        why: Some("done".to_owned()),
    };
    started
        .ctx
        .sql()
        .execute(
            "UPDATE agent SET status = 'ended', ended_how = ?1, \
             ended_why = ?2 WHERE id = 3",
            Some(vec![
                BindParam::Text(
                    serde_json::to_string(&left.how).unwrap(),
                ),
                BindParam::Text("done".to_owned()),
            ]),
        )
        .await
        .unwrap();
    let rows = started.rows();

    assert_eq!(left_of(&rows, 3).await, Some(left.clone()));
    assert_eq!(left_of(&rows, 4).await, None);
    let hire = lineage(&rows, AgentNumber(4)).await.unwrap().unwrap();
    assert_eq!(hire.leads[0].status, Status::Ended(left));
    assert!(!hire.is_led_by(AgentNumber(3)));
    assert_eq!(living(&rows, &hamlet).await.unwrap(), 2);
    started.stop().await;
}

async fn left_of(rows: &Rows<'_>, agent: u32) -> Option<Left> {
    left(rows, AgentNumber(agent)).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_name_resolves_to_its_agent() {
    let started = Started::new().await;
    let hamlet = HamletId("h".to_owned());
    started.found(&hamlet, AgentNumber(2)).await;
    started
        .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
        .await;
    let rows = started.rows();
    let named = async |hamlet: Option<&HamletId>, named: Named| {
        resolve(&rows, hamlet, &named).await.unwrap()
    };

    assert_eq!(named(None, Named::Hand).await, Some(Hand::NUMBER));
    assert_eq!(
        named(Some(&hamlet), Named::Mayor).await,
        Some(AgentNumber(2))
    );
    assert_eq!(named(None, Named::Mayor).await, None);
    let mayor_of = |hamlet: &str| Named::MayorOf {
        hamlet: HamletId(hamlet.to_owned()),
    };
    assert_eq!(
        named(None, mayor_of("h")).await,
        Some(AgentNumber(2))
    );
    assert_eq!(named(None, mayor_of("x")).await, None);
    let agent = |n| Named::Agent {
        agent: AgentNumber(n),
    };
    assert_eq!(named(None, agent(3)).await, Some(AgentNumber(3)));
    assert_eq!(named(None, agent(9)).await, None);
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_is_made_of_a_shared_profile_named_else_a_blank_one() {
    let started = Started::new().await;
    let rows = started.rows();
    let of = async |named| {
        made_of(&rows, named, None, None, &record::By::Chief)
            .await
            .unwrap()
    };
    let no_such = |profile| ProfileRefusal::NoSuchProfile {
        profile: ProfileId(profile),
    };

    assert_eq!(
        of(None).await,
        Ok(Of::Blank {
            profile: ProfileId(2),
            name: "Mayor".to_owned(),
            kind: profiles::Kind::Empty,
        })
    );
    assert_eq!(of(Some(HAND_PROFILE)).await, Err(no_such(1)));
    assert_eq!(of(Some(ProfileId(9))).await, Err(no_such(9)));
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_is_live_at_once_and_ends_once() {
    let started = Started::new().await;
    let worker = fresh(&started.rows()).await.unwrap();
    started
        .commit(Change::Worker { agent: worker })
        .await
        .unwrap();
    let rows = started.rows();
    let node = lineage(&rows, worker).await.unwrap().unwrap();
    assert_eq!(
        (node.agent.kind, node.agent.status, node.hamlet),
        (AgentKind::Worker, Status::Live, None)
    );
    assert_eq!(
        fresh(&rows).await.unwrap(),
        AgentNumber(worker.0 + 1)
    );

    for how in [HowEnded::Answered, HowEnded::Stopped] {
        started
            .commit(Change::WorkerEnded { agent: worker, how })
            .await
            .unwrap();
    }
    let left = left_of(&started.rows(), worker.0).await;
    assert_eq!(
        left,
        Some(Left {
            how: HowEnded::Answered,
            why: None,
        })
    );
    started.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_goes_live_and_keeps_its_first_purpose() {
    let started = Started::new().await;
    let hamlet = HamletId("h".to_owned());
    started.found(&hamlet, AgentNumber(2)).await;
    started
        .hire(&hamlet, AgentNumber(3), Some(AgentNumber(2)))
        .await;
    let hire = AgentNumber(3);
    let lead_place = versions::place(&started.rows(), AgentNumber(2))
        .await
        .unwrap();
    started.commit(Change::Live { agent: hire }).await.unwrap();
    for text in ["first", "second"] {
        started
            .commit(Change::Purpose {
                hire,
                text: text.to_owned(),
            })
            .await
            .unwrap();
    }
    let rows = started.rows();

    let node = lineage(&rows, hire).await.unwrap().unwrap().agent;
    assert_eq!((node.status, node.has_purpose), (Status::Live, true));
    let purpose = rows
        .value::<String>(
            "SELECT purpose FROM hire WHERE agent = 3",
            Vec::new(),
        )
        .await
        .unwrap();
    assert_eq!(purpose.as_deref(), Some("first"));
    assert_eq!(
        versions::place(&rows, AgentNumber(2)).await.unwrap(),
        lead_place + 1
    );
    assert_eq!(profile_of(&rows, hire).await.unwrap(), HAND_PROFILE);
    assert_eq!(
        profile_of(&rows, Hand::NUMBER).await.unwrap(),
        HAND_PROFILE
    );
    assert_eq!(kind_of(&rows, hire).await.unwrap(), AgentKind::Hire);
    assert_eq!(
        kind_of(&rows, Hand::NUMBER).await.unwrap(),
        AgentKind::Hand
    );
    started.stop().await;
}
