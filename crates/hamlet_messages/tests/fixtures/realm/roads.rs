//! The fixtures of roads and of a hire's tools.

use hamlet_messages::realm::choices::SetTools;
use hamlet_messages::realm::roads::{
    AgreeRefusal, AgreeRoad, Agreed, LayRefusal, LayRoad,
    RequestRoad, RequestRoadRefusal, TakeUpRefusal, TakeUpRoad,
};
use hamlet_messages::realm::{HowEnded, Named};
use hamlet_messages::workspace::AgentNumber;

use super::super::Fixtures;
use super::{
    HIRE, MAYOR, WHY, answers, asked, by_a_lead, by_an_agent, done,
    done_to, hamlet, hire, left, named_tool, only_a_hire, paid,
    road_done, shown, tool,
};

pub(super) fn the_roads_and_the_tools(fixtures: &mut Fixtures) {
    let lay = || LayRoad {
        a: hire(),
        b: Named::Agent {
            agent: AgentNumber(13),
        },
        why: None,
    };
    fixtures.args("realm/chief_lay_road", &asked(lay()));
    fixtures.args("realm/lay_road", &tool(lay()));
    fixtures.args(
        "realm/lay_road.why",
        &tool(LayRoad {
            why: Some(WHY.into()),
            ..lay()
        }),
    );
    let lay_refusals = || {
        vec![
            (
                "no_such_agent",
                LayRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("same_ends", LayRefusal::SameEnds),
            ("not_hires", LayRefusal::NotHires),
            (
                "other_hamlet",
                LayRefusal::OtherHamlet { hire: shown(MAYOR) },
            ),
            (
                "left",
                LayRefusal::Left {
                    hire: shown(HIRE),
                    left: left(HowEnded::ByTheHand),
                },
            ),
            (
                "road_exists",
                LayRefusal::RoadExists {
                    a: shown(HIRE),
                    b: shown(AgentNumber(13)),
                },
            ),
            (
                "full",
                LayRefusal::Full {
                    hamlet: hamlet(),
                    most: 100,
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_lay_road", lay_refusals());
    road_done(fixtures, "realm/lay_road", by_a_lead(lay_refusals()));
    fixtures.args(
        "realm/agree_road",
        &tool(AgreeRoad {
            mine: hire(),
            theirs: Named::Agent {
                agent: AgentNumber(31),
            },
            why: Some(WHY.into()),
        }),
    );
    fixtures.args(
        "realm/agree_road.mayor_of",
        &tool(AgreeRoad {
            mine: hire(),
            theirs: Named::MayorOf { hamlet: hamlet() },
            why: None,
        }),
    );
    answers(
        fixtures,
        "realm/agree_road",
        [
            (
                "waiting",
                Agreed::Waiting {
                    mine: shown(HIRE),
                    theirs: shown(AgentNumber(31)),
                },
            ),
            (
                "laid",
                Agreed::Laid {
                    mine: shown(HIRE),
                    theirs: shown(AgentNumber(31)),
                },
            ),
        ],
        by_an_agent(vec![
            ("not_a_mayor", AgreeRefusal::NotAMayor),
            (
                "no_such_agent",
                AgreeRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("no_roads_between", AgreeRefusal::NoRoadsBetween),
            ("not_hires", AgreeRefusal::NotHires),
            ("same_hamlet", AgreeRefusal::SameHamlet),
            (
                "road_exists",
                AgreeRefusal::RoadExists {
                    a: shown(HIRE),
                    b: shown(AgentNumber(31)),
                },
            ),
        ]),
    );
    let take_up = || TakeUpRoad {
        a: hire(),
        b: Named::Mayor,
        why: None,
    };
    fixtures.args("realm/chief_take_up_road", &asked(take_up()));
    fixtures.args("realm/take_up_road", &tool(take_up()));
    fixtures.args(
        "realm/take_up_road.why",
        &tool(TakeUpRoad {
            why: Some(WHY.into()),
            ..take_up()
        }),
    );
    let take_up_refusals = || {
        vec![
            (
                "no_such_agent",
                TakeUpRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            (
                "no_road",
                TakeUpRefusal::NoRoad {
                    a: shown(HIRE),
                    b: shown(MAYOR),
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_take_up_road", take_up_refusals());
    road_done(
        fixtures,
        "realm/take_up_road",
        by_a_lead(take_up_refusals()),
    );
    fixtures.args(
        "realm/request_road",
        &paid(RequestRoad {
            to: Named::Agent {
                agent: AgentNumber(13),
            },
            why: Some("To ask about the tests".into()),
        }),
    );
    fixtures.args(
        "realm/request_road.no_why",
        &tool(RequestRoad {
            to: Named::Mayor,
            why: None,
        }),
    );
    done(
        fixtures,
        "realm/request_road",
        by_an_agent(vec![
            (
                "no_such_agent",
                RequestRoadRefusal::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            ("no_lead", RequestRoadRefusal::NoLead),
            (
                "still_hiring",
                RequestRoadRefusal::StillHiring { lead: shown(HIRE) },
            ),
        ]),
    );
    fixtures.args(
        "realm/chief_set_tools",
        &asked(SetTools {
            hire: hire(),
            off: vec![named_tool("tell"), named_tool("bash")],
            on: vec![named_tool("$Set")],
            why: None,
        }),
    );
    fixtures.args(
        "realm/set_tools",
        &tool(SetTools {
            hire: hire(),
            off: vec![named_tool("tell")],
            on: Vec::new(),
            why: None,
        }),
    );
    fixtures.args(
        "realm/set_tools.why",
        &tool(SetTools {
            hire: hire(),
            off: Vec::new(),
            on: vec![named_tool("tell")],
            why: Some(WHY.into()),
        }),
    );
    done(fixtures, "realm/chief_set_tools", only_a_hire());
    done_to(fixtures, "realm/set_tools", by_a_lead(only_a_hire()));
}
