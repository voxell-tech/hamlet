use std::time::Duration;

use hamlet_messages::realm::{
    Asked, Done, End, EndRefusal, FoundAnswer, HamletId, Hire,
    HireAnswer, HireRefusal, HowEnded, LeadRefusal, Left, Named,
    Rank, Tool,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
use hamlet_protocol::client::RequestId;

use super::{
    Callee, MAYOR, Realm, chief_hires, error_code, found, founded,
    hire, hired, paid, standing, tool, until,
};

fn under(agent: u32) -> Option<Named> {
    Some(Named::Agent {
        agent: AgentNumber(agent),
    })
}

fn end(agent: u32) -> End {
    End {
        hire: Named::Agent {
            agent: AgentNumber(agent),
        },
        why: None,
    }
}

fn not_a_lead<R>(agent: u32) -> LeadRefusal<R> {
    LeadRefusal::Rank(Rank::NotALead {
        hire: AgentNumber(agent),
    })
}

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
    // kept an `Ended` reach for 3. The same end again answers the
    // same, and another end is refused: 3 has left.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    let chief_hires =
        async |under| chief_hires(&realm, &hamlet, hire(under)).await;
    assert_eq!(chief_hires(None).await, hired(3));
    assert_eq!(chief_hires(under(3)).await, hired(4));
    realm.reach_held(AgentNumber(4)).await;

    let end_3 = Asked {
        id: RequestId::new(),
        act: End {
            why: Some("done".to_owned()),
            ..end(3)
        },
    };
    let ended = realm.ask(&end_3).await.unwrap();
    until("the workspace holds the end of agent 3", || {
        ended_in_workspace(&realm, AgentNumber(3))
    })
    .await;
    let again = realm.ask(&end_3).await.unwrap();
    let other = realm
        .ask(&Asked {
            id: RequestId::new(),
            ..end_3.clone()
        })
        .await
        .unwrap();
    let (lead_of_4, (status, how, why)) = realm
        .stopped_rows(|db| {
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

    assert_eq!([ended, again], [Done::Done, Done::Done]);
    let left = Left {
        how: HowEnded::ByTheChief,
        why: Some("done".to_owned()),
    };
    assert_eq!(
        other,
        Done::Refused {
            why: EndRefusal::Left {
                hire: AgentNumber(3),
                left,
            }
        }
    );
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
async fn a_repeated_end_sends_the_end_again() {
    // the workspace fails the end's SetReach once; the same End
    // again sends it, with no restart.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    let hired_3 = realm.tool(MAYOR, &tool("h1", hire(None))).await;
    realm.reach_held(MAYOR).await;
    realm.reach_held(AgentNumber(3)).await;
    realm.seen.fail_once(Callee::SetReach, AgentNumber(3));
    let end_3 = tool("e1", end(3));

    let ended = realm.tool(MAYOR, &end_3).await.unwrap();
    until("the workspace fails the end once", || {
        realm.seen.has_failed(Callee::SetReach, AgentNumber(3))
    })
    .await;
    let after_the_failure =
        ended_in_workspace(&realm, AgentNumber(3));
    let again = realm.tool(MAYOR, &end_3).await.unwrap();
    until("the workspace holds the end of agent 3", || {
        ended_in_workspace(&realm, AgentNumber(3))
    })
    .await;

    assert_eq!(hired_3.unwrap(), hired(3));
    assert_eq!([ended, again], [Done::Done, Done::Done]);
    assert!(!after_the_failure);
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_end_of_a_hire_still_hiring_closes_its_steps() {
    // hold the branch; mayor 2 hires 3; the chief ends 3 while its
    // drive waits on the branch; release: the branch is taken and no
    // step after it, so `agent/3` is never made or filled and its one
    // reach is its end; its row is closed after the profile.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.reach_held(MAYOR).await;
    realm.seen.hold(Callee::Branch);

    let hired_3 = realm.tool(MAYOR, &tool("h1", hire(None))).await;
    until("the drive of 3 waits on the held branch", || {
        realm.seen.is_waiting(Callee::Branch, AgentNumber(3))
    })
    .await;
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
    until("the profile takes the branch of 3", || {
        realm
            .seen
            .branches
            .lock()
            .unwrap()
            .iter()
            .any(|(_, branch)| branch.name == "hires/3")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let three = AgentNumber(3);
    let made_3 = realm
        .seen
        .made
        .lock()
        .unwrap()
        .iter()
        .any(|(agent, _)| *agent == three);
    let filled_3 = realm
        .seen
        .fills
        .lock()
        .unwrap()
        .iter()
        .any(|(agent, _)| *agent == three);
    let reaches_3 = realm
        .seen
        .reaches
        .lock()
        .unwrap()
        .iter()
        .filter(|set| set.agent == three)
        .map(|set| set.reach.clone())
        .collect::<Vec<_>>();

    assert_eq!(hired_3.unwrap(), hired(3));
    assert_eq!(ended, Done::Done);
    assert_eq!((made_3, filled_3), (false, false));
    assert_eq!(reaches_3, [Reach::Ended]);
    let (status, done, closed) = realm
        .stopped_rows(|db| {
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
    assert_eq!(
        (status.as_str(), done.as_deref(), closed),
        ("ended", Some("profile"), true)
    );
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
    realm.seen.fail_once(Callee::Branch, AgentNumber(3));
    let call = tool("h1", hire(None));

    let first = realm.tool(MAYOR, &call).await.unwrap();
    until("the profile fails the branch once", || {
        realm.seen.has_failed(Callee::Branch, AgentNumber(3))
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
        .stopped_rows(|db| {
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

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_call_repeated_from_another_turn_is_a_reused_id() {
    // the same hire under one call id from the mayor's turn on its
    // own task, then on the Hand's, then in another conversation:
    // each later one is refused, and one agent is hired.
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    founded(&realm).await;
    realm.reach_held(MAYOR).await;
    let own = standing(&realm, MAYOR);
    let other = standing(&realm, Hand::NUMBER);
    let in_turn = |task: &RealmTask, conversation: &str| Tool {
        conversation: Some(ConversationId(conversation.to_owned())),
        ..paid("h1", task, hire(None))
    };

    let first = realm.tool(MAYOR, &in_turn(&own, "c1")).await;
    let again = realm.tool(MAYOR, &in_turn(&own, "c1")).await;
    let other_task = realm.tool(MAYOR, &in_turn(&other, "c1")).await;
    let other_conversation =
        realm.tool(MAYOR, &in_turn(&own, "c2")).await;

    assert_eq!(first.unwrap(), hired(3));
    assert_eq!(again.unwrap(), hired(3));
    assert_eq!(error_code(other_task), "message.id_reused");
    assert_eq!(error_code(other_conversation), "message.id_reused");
    realm.host.shutdown().await.unwrap();
}

fn refused_to_an_agent(
    why: HireRefusal,
) -> HireAnswer<LeadRefusal<HireRefusal>> {
    HireAnswer::Refused {
        why: LeadRefusal::Act(why),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agents_hire_says_what_it_is_for() {
    // mayor 2 hires with no purpose, an empty one and a blank one,
    // and the Hand with none: each is refused `NoPurpose`. The chief
    // hires with none, and hires 3.
    let realm = Realm::start().await;
    let hamlet = founded(&realm).await;
    let hire_for = |purpose: Option<&str>| Hire {
        purpose: purpose.map(str::to_owned),
        ..hire(None)
    };

    let mut by_the_mayor = Vec::new();
    for (call, purpose) in
        [("h1", None), ("h2", Some("")), ("h3", Some("  "))]
    {
        by_the_mayor.push(
            realm.tool(MAYOR, &tool(call, hire_for(purpose))).await,
        );
    }
    let by_the_chief =
        chief_hires(&realm, &hamlet, hire_for(None)).await;
    let by_the_hand = realm
        .tool(
            Hand::NUMBER,
            &tool(
                "h4",
                Hire {
                    hamlet: Some(hamlet.clone()),
                    ..hire_for(None)
                },
            ),
        )
        .await;

    for answer in by_the_mayor.into_iter().chain([by_the_hand]) {
        assert_eq!(
            answer.unwrap(),
            refused_to_an_agent(HireRefusal::NoPurpose)
        );
    }
    assert_eq!(by_the_chief, hired(3));
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lead_in_another_hamlet_is_refused() {
    // found Mill (mayor 2) and Pond (mayor 3); a hire into Mill under
    // 3: the chief and the Hand hear `OtherHamlet`, and mayor 2,
    // which does not lead 3, hears `NotALead` first.
    let realm = Realm::start().await;
    let mill = founded(&realm).await;
    let pond = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: found("Pond"),
        })
        .await
        .unwrap();
    let under_3 = Hire {
        hamlet: Some(mill.clone()),
        ..hire(under(3))
    };

    let by_the_chief =
        chief_hires(&realm, &mill, hire(under(3))).await;
    let by_the_hand =
        realm.tool(Hand::NUMBER, &tool("h1", under_3)).await;
    let by_the_mayor =
        realm.tool(MAYOR, &tool("h2", hire(under(3)))).await;

    assert!(
        matches!(
            pond,
            FoundAnswer::Founded {
                mayor: AgentNumber(3),
                ..
            }
        ),
        "{pond:?}"
    );
    let other = HireRefusal::OtherHamlet {
        lead: AgentNumber(3),
    };
    assert_eq!(
        by_the_chief,
        HireAnswer::Refused { why: other.clone() }
    );
    assert_eq!(by_the_hand.unwrap(), refused_to_an_agent(other));
    assert_eq!(
        by_the_mayor.unwrap(),
        HireAnswer::Refused { why: not_a_lead(3) }
    );
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hire_under_the_hand_names_no_agent() {
    // a hire under the Hand, by the chief, the Hand and mayor 2: each
    // is refused `NoSuchAgent`.
    let realm = Realm::start().await;
    let hamlet = founded(&realm).await;
    let under_the_hand = Hire {
        hamlet: Some(hamlet.clone()),
        ..hire(Some(Named::Hand))
    };

    let by_the_chief =
        chief_hires(&realm, &hamlet, hire(Some(Named::Hand))).await;
    let by_the_hand =
        realm.tool(Hand::NUMBER, &tool("h1", under_the_hand)).await;
    let by_the_mayor = realm
        .tool(MAYOR, &tool("h2", hire(Some(Named::Hand))))
        .await;

    assert_eq!(
        by_the_chief,
        HireAnswer::Refused {
            why: HireRefusal::NoSuchAgent {
                address: "hand".to_owned()
            }
        }
    );
    for answer in [by_the_hand, by_the_mayor] {
        assert_eq!(
            answer.unwrap(),
            refused_to_an_agent(HireRefusal::NoSuchAgent {
                address: "hand".to_owned()
            })
        );
    }
    realm.host.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hands_hire_names_its_hamlet_in_todays_words() {
    // As today's hire by the Hand (calls/society.rs:860-874).
    let realm = Realm::start().await;
    founded(&realm).await;
    let unnamed = hire(None);
    let nowhere = Hire {
        hamlet: Some(HamletId("nowhere".to_owned())),
        ..hire(None)
    };

    let unnamed =
        realm.tool(Hand::NUMBER, &tool("h1", unnamed)).await;
    let nowhere =
        realm.tool(Hand::NUMBER, &tool("h2", nowhere)).await;

    let words = |answer: HireAnswer<LeadRefusal<HireRefusal>>| {
        let HireAnswer::Refused { why } = answer else {
            panic!("{answer:?}");
        };
        why.to_string()
    };
    assert_eq!(
        words(unnamed.unwrap()),
        "Say which hamlet to hire into."
    );
    assert_eq!(
        words(nowhere.unwrap()),
        "no hamlet has the id `nowhere`"
    );
    realm.host.shutdown().await.unwrap();
}
