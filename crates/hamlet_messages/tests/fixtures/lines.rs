//! The fixtures of a chief's lines as the realm keeps them, and of
//! the realm's stop of agents.

use hamlet_messages::agent::chat::{Stop, Stopped, Withdrawn};
use hamlet_messages::agent::{Input, InputId};
use hamlet_messages::realm::lines::{
    LineIs, ReadLine, StopAgents, Working,
};
use hamlet_messages::workspace::AgentNumber;

use super::Fixtures;
use super::cases::{Cases, named};

pub fn enums() -> Vec<Cases> {
    vec![named::<LineIs>()]
}

pub fn the_lines_and_the_stop(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/read_line",
        &ReadLine(
            "0b5e7c4a-1d2f-4e3a-8b9c-0d1e2f3a4b5c".parse().unwrap(),
        ),
    );
    for (case, is) in [
        ("waiting", LineIs::Waiting),
        ("given", LineIs::Given),
        ("withdrawn", LineIs::Withdrawn),
    ] {
        fixtures.answer(&format!("realm/read_line.{case}"), &is);
    }
    fixtures.args(
        "realm/stop_agents",
        &StopAgents {
            agents: vec![AgentNumber(1), AgentNumber(12)],
        },
    );
    fixtures.answer("realm/stop_agents", &Stopped { turns: 2 });
    fixtures.args("realm/working", &Working { open: true });
    fixtures.answer("realm/working", &());
    fixtures.args(
        "agent/stop",
        &Stop {
            number: 1_790_000_000_000_003,
            withdrawn: vec![Withdrawn {
                id: InputId(
                    "0b5e7c4a-1d2f-4e3a-8b9c-0d1e2f3a4b01".into(),
                ),
                line: Input::Line {
                    text: "Stop after this".into(),
                    images: Vec::new(),
                    files: Vec::new(),
                    note: None,
                    chief: Some("chief".into()),
                },
            }],
        },
    );
    fixtures.answer("agent/stop", &Stopped { turns: 1 });
}
