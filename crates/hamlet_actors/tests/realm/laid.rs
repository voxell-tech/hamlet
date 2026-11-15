use hamlet_messages::agent::Place;
use hamlet_messages::realm::{
    AgentRefusal, Asked, Done, Gate, HamletId, Hire, HireAnswer,
    LeadRefusal, Named, SetTools, SocietyTool, Tell, ToolsRefusal,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use hamlet_protocol::client::RequestId;

use super::{MAYOR, Realm, founded, tool, until};

const THREE: AgentNumber = AgentNumber(3);

fn to(agent: AgentNumber) -> Named {
    Named::Agent { agent }
}

fn tell(to: AgentNumber) -> Tell {
    Tell {
        to: self::to(to),
        text: "hi".to_owned(),
        task: None,
    }
}

fn tools(off: &[SocietyTool], on: &[SocietyTool]) -> SetTools {
    SetTools {
        hire: to(THREE),
        off: off.to_vec(),
        on: on.to_vec(),
    }
}

/// A realm with a hamlet whose mayor has `count` live hires, from
/// agent 3 up.
async fn hamlet_with(count: u32) -> (Realm, HamletId) {
    let realm = Realm::start().await;
    realm.live(Hand::NUMBER).await;
    let hamlet = founded(&realm).await;
    realm.live(MAYOR).await;
    for n in 0..count {
        let hire = Hire {
            hamlet: None,
            under: None,
            profile: None,
            purpose: Some("Write the docs".to_owned()),
        };
        let hired =
            realm.tool(MAYOR, &tool(&format!("h{n}"), hire)).await;
        assert!(
            matches!(hired, Ok(HireAnswer::Hired { .. })),
            "{hired:?}"
        );
        realm.live(AgentNumber(3 + n)).await;
    }
    (realm, hamlet)
}

/// Waits until the last place sent to `agent` holds.
async fn placed(
    realm: &Realm,
    agent: AgentNumber,
    holds: impl Fn(&Place) -> bool,
) {
    until(&format!("agent {}'s place", agent.0), || {
        realm
            .seen
            .places
            .lock()
            .unwrap()
            .iter()
            .rfind(|(to, _)| *to == agent)
            .is_some_and(|(_, place)| holds(place))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_turned_off_is_refused() {
    // the mayor turns `tell` off for its hire 3: 3's tell is refused
    // `ToolOff`, and its place lists the tool. Turned on again, the
    // tell goes through. The Hand's and a mayor's tools are not
    // turned off, by the Hand, a hire or the chief.
    let (realm, _) = hamlet_with(1).await;
    let set = async |by, call, act| {
        realm.tool(by, &tool(call, act)).await.unwrap()
    };

    let off =
        set(MAYOR, "s1", tools(&[SocietyTool::Tell], &[])).await;
    let refused = realm.tool(THREE, &tool("t1", tell(MAYOR))).await;
    placed(&realm, THREE, |place| {
        place.tools_off == [SocietyTool::Tell]
    })
    .await;
    let on = set(MAYOR, "s2", tools(&[], &[SocietyTool::Tell])).await;
    placed(&realm, THREE, |place| place.tools_off.is_empty()).await;
    let told = realm.tool(THREE, &tool("t2", tell(MAYOR))).await;
    let of_hand = SetTools {
        hire: Named::Hand,
        ..tools(&[SocietyTool::Hire], &[])
    };
    let of_mayor = SetTools {
        hire: Named::Mayor,
        ..tools(&[SocietyTool::Hire], &[])
    };
    let hand_by_hand = set(Hand::NUMBER, "s1", of_hand.clone()).await;
    let mayor_by_hire = set(THREE, "s1", of_mayor).await;
    let chief = realm
        .ask(&Asked {
            id: RequestId::new(),
            act: of_hand,
        })
        .await
        .unwrap();

    assert_eq!([off, on], [Done::Done, Done::Done]);
    assert_eq!(
        refused.unwrap(),
        Done::Refused {
            why: AgentRefusal::Gate(Gate::ToolOff {
                tool: SocietyTool::Tell
            })
        }
    );
    assert_eq!(told.unwrap(), Done::Done);
    let not_a_hire = Done::Refused {
        why: LeadRefusal::Act(ToolsRefusal::NotAHire),
    };
    assert_eq!(
        [hand_by_hand, mayor_by_hire],
        [not_a_hire.clone(), not_a_hire]
    );
    assert_eq!(
        chief,
        Done::Refused {
            why: ToolsRefusal::NotAHire
        }
    );
    realm.host.shutdown().await.unwrap();
}
