use super::{Demo, Expect, Is, Kill, Probe, Read, Scenario};

/// Whether the command of `workspace-command` touched its file, which
/// it does only if it outlived the kill.
const LATE_FILE: Probe = Probe {
    name: "late.files",
    file: "actors/agent/1.fs/late",
    read: Read::Files,
};

/// The workspace's rows marked cut off.
const CUT_OFF_ROWS: Probe = Probe {
    name: "workspace.cut_off",
    file: "actors/workspace/local.sqlite",
    read: Read::Sql(
        "SELECT COUNT(*) FROM calls WHERE state = 'cut_off'",
    ),
};

/// The ends agent 1 was told.
const TOLD: Probe = Probe {
    name: "agent.finished",
    file: "actors/agent/1.sqlite",
    read: Read::Sql("SELECT COUNT(*) FROM finished"),
};

const COMMAND_CUT_OFF: &[Expect] = &[
    Expect {
        value: "status/1/cut_off",
        is: Is::Exactly(0),
    },
    Expect {
        value: "late.files",
        is: Is::Exactly(0),
    },
    Expect {
        value: "workspace.cut_off",
        is: Is::Exactly(1),
    },
    Expect {
        value: "agent.finished",
        is: Is::Exactly(1),
    },
];

pub const SCENARIOS: &[Scenario] = &[Scenario {
    name: "workspace-command",
    about: "kill the host while a command of the workspace runs, \
            after it answered Running; the command's process is \
            gone under bubblewrap, and after each restart its row \
            reads cut off, a status by its id answers cut off, and \
            the agent was told it was cut off once",
    demo: Demo::Local,
    setup: &["realm reach 1 1"],
    trigger: "agent 1 run c1 sleep 8; touch late",
    trigger_replies: true,
    kill: Kill::After {
        min_ms: 200,
        max_ms: 1000,
    },
    // Past the 8 s at which a command that outlived the kill
    // would touch its file, counted from the trigger.
    down_ms: 11_000,
    due: None,
    restart: [&["agent 1 status 1"], &["agent 1 status 1"]],
    probes: &[LATE_FILE, CUT_OFF_ROWS, TOLD],
    expect: [COMMAND_CUT_OFF, COMMAND_CUT_OFF],
}];
