//! The fixtures of the realm's messages about oversight, workers and
//! errands, a hamlet's settings, the chief's choices and documents,
//! and the tasks an agent opens.

use hamlet_messages::agent::BuiltIn;
use hamlet_messages::agent::copies::{
    Copies, CopyVersion, Versioned,
};
use hamlet_messages::realm::budget::{
    OpenTask, TaskOpened, TaskTurn,
};
use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::documents::{
    CharterRead, EditCharter, EditRefusal, ReadCharter, ReadCopies,
};
use hamlet_messages::realm::hamlets::{
    MoveHamlet, NameHamlet, ReadRecord, Record, RecordLine,
    SetFolder, SetMode,
};
use hamlet_messages::realm::oversight::{
    Hold, Lapse, LapseReason, Permission, Revoke, RevokeRefusal,
};
use hamlet_messages::realm::workers::{
    AllowanceRefusal, Ended, ErrandDone, SetAllowance, StartWorker,
    Started, WorkerDone, WorkerRefusal,
};
use hamlet_messages::realm::{HamletRefusal, HowEnded};
use hamlet_messages::workspace::reach::Mode;
use hamlet_messages::workspace::{AgentNumber, ConversationId};
use hamlet_protocol::Effort;

use super::super::Fixtures;
use super::super::cases::{Cases, named, tagged};
use super::{
    HIRE, WHY, answers, asked, done, hamlet, hire, left,
    standing_task, you_left,
};

pub fn enums() -> Vec<Cases> {
    vec![
        named::<LapseReason>(),
        tagged::<Hold>(),
        named::<Permission>(),
        tagged::<RevokeRefusal>(),
        tagged::<Started<WorkerRefusal>>(),
        tagged::<WorkerRefusal>(),
        tagged::<Ended>(),
        tagged::<AllowanceRefusal>(),
        tagged::<Record>(),
        tagged::<ModelOf>(),
        tagged::<ChoiceRefusal>(),
        tagged::<EditRefusal>(),
        named::<TaskTurn>(),
        tagged::<TaskOpened>(),
    ]
}

pub fn the_oversight_and_the_settings(fixtures: &mut Fixtures) {
    the_oversight(fixtures);
    the_workers(fixtures);
    the_hamlets(fixtures);
    the_choices(fixtures);
    the_documents(fixtures);
}

fn the_oversight(fixtures: &mut Fixtures) {
    for (case, why) in [
        ("stopped", LapseReason::Stopped),
        ("rewound", LapseReason::Rewound),
        ("ended", LapseReason::Ended),
        ("resumed", LapseReason::Resumed),
    ] {
        fixtures.args(
            &format!("realm/lapse.{case}"),
            &Lapse {
                conversation: Some(ConversationId("c1".into())),
                why,
            },
        );
    }
    fixtures.args(
        "realm/lapse.all",
        &Lapse {
            conversation: None,
            why: LapseReason::Ended,
        },
    );
    fixtures.answer("realm/lapse", &());
    fixtures.args(
        "realm/chief_revoke",
        &asked(Revoke {
            agent: HIRE,
            permission: Permission::Network,
        }),
    );
    done(
        fixtures,
        "realm/chief_revoke",
        vec![("no_grant", RevokeRefusal::NoGrant { agent: HIRE })],
    );
    for (case, turn) in [
        ("retry", TaskTurn::Retry),
        ("reflection", TaskTurn::Reflection),
    ] {
        fixtures.args(
            &format!("realm/open_task.{case}"),
            &OpenTask { turn },
        );
    }
    fixtures.answer(
        "realm/open_task.opened",
        &TaskOpened::Opened {
            task: standing_task(),
        },
    );
    fixtures.answer(
        "realm/open_task.refused.you_left",
        &TaskOpened::Refused { why: you_left() },
    );
}

fn the_workers(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_start_worker",
        &asked(StartWorker {
            brief: "Check the links in the docs".into(),
            tools: vec![
                BuiltIn {
                    name: "read".into(),
                },
                BuiltIn {
                    name: "bash".into(),
                },
            ],
            task: None,
            hamlet: Some(hamlet()),
        }),
    );
    answers(
        fixtures,
        "realm/chief_start_worker",
        [(
            "started",
            Started::Started {
                worker: AgentNumber(15),
            },
        )],
        vec![
            ("empty_brief", WorkerRefusal::EmptyBrief),
            (
                "no_such_tool",
                WorkerRefusal::NoSuchTool { tool: "fly".into() },
            ),
            ("a_worker", WorkerRefusal::AWorker),
            (
                "hamlet_from_an_agent",
                WorkerRefusal::HamletFromAnAgent,
            ),
            ("no_such_hamlet", WorkerRefusal::NoSuchHamlet),
        ],
    );
    let ended = || {
        [
            (
                "answered",
                Ended::Answered {
                    text: "Three links are broken".into(),
                },
            ),
            (
                "failed",
                Ended::Failed {
                    why: "the model failed".into(),
                },
            ),
        ]
    };
    for (case, ended) in ended() {
        fixtures.args(
            &format!("realm/worker_done.{case}"),
            &WorkerDone { ended },
        );
    }
    fixtures.answer("realm/worker_done", &());
    for (case, ended) in ended() {
        fixtures.args(
            &format!("realm/errand_done.{case}"),
            &ErrandDone { ended },
        );
    }
    fixtures.answer("realm/errand_done", &());
    fixtures.args(
        "realm/chief_set_allowance",
        &asked(SetAllowance {
            hire: hire(),
            micros: Some(2_500_000),
            why: Some("small".into()),
        }),
    );
    fixtures.args(
        "realm/chief_set_allowance.none",
        &asked(SetAllowance {
            hire: hire(),
            micros: None,
            why: None,
        }),
    );
    done(
        fixtures,
        "realm/chief_set_allowance",
        vec![
            ("no_such_agent", AllowanceRefusal::NoSuchAgent),
            ("not_a_hire", AllowanceRefusal::NotAHire),
            (
                "over_leads",
                AllowanceRefusal::OverLeads {
                    lead_micros: 2_000_000,
                },
            ),
        ],
    );
}

fn the_hamlets(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_name_hamlet",
        &asked(NameHamlet {
            hamlet: hamlet(),
            name: "Orchard".into(),
        }),
    );
    fixtures.args(
        "realm/chief_move_hamlet",
        &asked(MoveHamlet {
            hamlet: hamlet(),
            x: 12.5,
            z: -40.0,
        }),
    );
    for (case, mode) in
        [("manual", Mode::Manual), ("auto", Mode::Auto)]
    {
        fixtures.args(
            &format!("realm/chief_set_mode.{case}"),
            &asked(SetMode {
                hamlet: hamlet(),
                mode,
            }),
        );
    }
    fixtures.args(
        "realm/chief_set_folder",
        &asked(SetFolder {
            hamlet: hamlet(),
            folder: Some("/work/orchard".into()),
        }),
    );
    fixtures.args(
        "realm/chief_set_folder.none",
        &asked(SetFolder {
            hamlet: hamlet(),
            folder: None,
        }),
    );
    for name in [
        "realm/chief_name_hamlet",
        "realm/chief_move_hamlet",
        "realm/chief_set_mode",
    ] {
        done(
            fixtures,
            name,
            vec![("no_such_hamlet", HamletRefusal::NoSuchHamlet)],
        );
    }
    done(
        fixtures,
        "realm/chief_set_folder",
        vec![
            ("no_such_hamlet", HamletRefusal::NoSuchHamlet),
            (
                "no_such_folder",
                HamletRefusal::NoSuchFolder {
                    path: "/work/missing".into(),
                },
            ),
        ],
    );
    fixtures.args(
        "realm/chief_read_record",
        &asked(ReadRecord {
            hamlet: Some(hamlet()),
        }),
    );
    fixtures.args(
        "realm/chief_read_record.realm",
        &asked(ReadRecord { hamlet: None }),
    );
    answers(
        fixtures,
        "realm/chief_read_record",
        [(
            "read",
            Record::Read {
                lines: vec![RecordLine {
                    text: "- hire#2 (ada) laid a road from hire#3 \
                           to hire#4: to share the tests"
                        .into(),
                }],
            },
        )],
        vec![("no_such_hamlet", HamletRefusal::NoSuchHamlet)],
    );
}

fn the_choices(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_set_nickname",
        &asked(SetNickname {
            hire: hire(),
            name: Some("Spade".into()),
            why: Some(WHY.into()),
        }),
    );
    fixtures.args(
        "realm/chief_set_nickname.none",
        &asked(SetNickname {
            hire: hire(),
            name: None,
            why: None,
        }),
    );
    fixtures.args(
        "realm/chief_set_model",
        &asked(SetModel {
            of: ModelOf::Hire { hire: hire() },
            model: Some("anthropic::claude-opus".into()),
            effort: Some(Effort::High),
            fast: Some(true),
            why: Some(WHY.into()),
        }),
    );
    fixtures.args(
        "realm/chief_set_model.hamlet",
        &asked(SetModel {
            of: ModelOf::Hamlet { hamlet: hamlet() },
            model: None,
            effort: None,
            fast: None,
            why: None,
        }),
    );
    let refusals = || {
        let left = |how| ChoiceRefusal::Left {
            hire: HIRE,
            left: left(how),
        };
        vec![
            ("no_such_agent", ChoiceRefusal::NoSuchAgent),
            ("no_such_hamlet", ChoiceRefusal::NoSuchHamlet),
            ("left", left(HowEnded::ByTheChief)),
            ("left.answered", left(HowEnded::Answered)),
            ("left.failed", left(HowEnded::Failed)),
            ("left.stopped", left(HowEnded::Stopped)),
            ("bad_nickname", ChoiceRefusal::BadNickname),
            (
                "no_such_model",
                ChoiceRefusal::NoSuchModel {
                    model: "missing::model".into(),
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_set_nickname", refusals());
    done(fixtures, "realm/chief_set_model", refusals());
}

fn the_documents(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_edit_charter",
        &asked(EditCharter {
            text: Some("Be brief.".into()),
        }),
    );
    fixtures.args(
        "realm/chief_edit_charter.none",
        &asked(EditCharter { text: None }),
    );
    done(
        fixtures,
        "realm/chief_edit_charter",
        vec![
            ("no_such_hamlet", EditRefusal::NoSuchHamlet),
            (
                "no_such_model",
                EditRefusal::NoSuchModel {
                    model: "missing::model".into(),
                },
            ),
            ("a_profiles", EditRefusal::AProfiles),
        ],
    );
    fixtures.args("realm/read_charter", &ReadCharter);
    fixtures.answer(
        "realm/read_charter",
        &CharterRead {
            own: true,
            text: "Be brief.".into(),
        },
    );
    fixtures.args(
        "realm/read_copies",
        &ReadCopies {
            charter: Some(CopyVersion(3)),
            character: Some(CopyVersion(2)),
            model: Some(CopyVersion(5)),
            settings: Some(CopyVersion(1)),
        },
    );
    fixtures.args("realm/read_copies.none", &ReadCopies::default());
    fixtures.answer(
        "realm/read_copies",
        &Copies {
            charter: Some(Versioned {
                version: CopyVersion(4),
                value: "Be brief.".into(),
            }),
            ..Copies::default()
        },
    );
}
