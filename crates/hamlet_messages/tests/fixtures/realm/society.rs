//! The fixtures of the realm's messages about oversight, workers and
//! errands, a hamlet's settings, the chief's choices and documents,
//! and the tasks an agent opens.

use hamlet_messages::agent::chat::Count;
use hamlet_messages::agent::copies::{
    Copies, CopyVersion, Price, Versioned,
};
use hamlet_messages::realm::budget::{
    BudgetRead, OpenTask, Payer, Period, ReadBudget,
    ReadBudgetRefusal, TaskOpened, TaskTurn,
};
use hamlet_messages::realm::choices::{
    ChoiceRefusal, ModelOf, SetModel, SetNickname,
};
use hamlet_messages::realm::documents::{
    CardSetting, CatalogueRefusal, CharterRead, EditCatalogue,
    EditCharter, EditRefusal, EditSettings, Endpoint, ImageLimits,
    Newcomers, ReadCharter, ReadCopies, RemoteWorkspace, Setting,
    SettingRefusal,
};
use hamlet_messages::realm::hamlets::{
    MoveHamlet, NameHamlet, ReadRecord, Record, RecordLine,
    SetFolder, SetMode,
};
use hamlet_messages::realm::oversight::{
    AskChief, AskChiefRefusal, Hold, Lapse, LapseReason, Object,
    ObjectRefusal, Offer, Permission, Question, Revoke,
    RevokeRefusal,
};
use hamlet_messages::realm::workers::{
    Ended, ErrandDone, SetAllowance, StartWorker, Started,
    WorkerDone, WorkerRefusal,
};
use hamlet_messages::realm::world::Spend;
use hamlet_messages::realm::{
    HamletRefusal, HowEnded, OnlyAHire, SocietyTool,
};
use hamlet_messages::workspace::reach::Mode;
use hamlet_messages::workspace::{AgentNumber, ConversationId};
use hamlet_protocol::Effort;

use super::super::Fixtures;
use super::super::cases::{Cases, Union, named, tagged};
use super::{
    HIRE, WHY, agent_refusal, answers, asked, by_a_lead, by_an_agent,
    done, hamlet, hire, lead_refusal, left, named_tool,
    standing_task, tool, you_left,
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
        tagged::<Record>(),
        tagged::<ModelOf>(),
        tagged::<ChoiceRefusal>(),
        tagged::<EditRefusal>(),
        named::<TaskTurn>(),
        tagged::<TaskOpened>(),
        tagged::<AskChiefRefusal>(),
        tagged::<ObjectRefusal>(),
        tagged::<BudgetRead<ReadBudgetRefusal>>(),
        tagged::<ReadBudgetRefusal>(),
        named::<Period>(),
        tagged::<Setting>(),
        named::<Newcomers>(),
        tagged::<SettingRefusal>(),
        tagged::<CardSetting>(),
        tagged::<CatalogueRefusal>(),
    ]
}

pub fn unions() -> Vec<Union> {
    vec![
        agent_refusal::<AskChiefRefusal>(),
        agent_refusal::<ObjectRefusal>(),
        agent_refusal::<ReadBudgetRefusal>(),
        agent_refusal::<WorkerRefusal>(),
        lead_refusal::<ChoiceRefusal>(),
    ]
}

pub fn the_oversight_and_the_settings(fixtures: &mut Fixtures) {
    the_oversight(fixtures);
    the_workers(fixtures);
    the_hamlets(fixtures);
    the_choices(fixtures);
    the_documents(fixtures);
    the_settings_and_the_catalogue(fixtures);
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
        vec![
            (
                "no_such_agent",
                RevokeRefusal::NoSuchAgent {
                    address: "9".into(),
                },
            ),
            (
                "no_grant",
                RevokeRefusal::NoGrant {
                    name: "hire#12 (Mira)".into(),
                    permission: Permission::Network,
                },
            ),
            (
                "allowed_to_all",
                RevokeRefusal::AllowedToAll {
                    name: "hire#12 (Mira)".into(),
                    permission: Permission::Network,
                },
            ),
        ],
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
    the_questions_and_the_objections(fixtures);
    the_budget_read(fixtures);
}

fn the_questions_and_the_objections(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/ask_chief",
        &tool(AskChief {
            task: standing_task(),
            conversation: ConversationId("c1".into()),
            message: Some("Before planting".into()),
            questions: vec![
                Question {
                    question: "Which seed?".into(),
                    description: Some("The field is wet".into()),
                    options: vec![
                        Offer {
                            title: "Oats".into(),
                            description: None,
                        },
                        Offer {
                            title: "Barley".into(),
                            description: Some("Slower".into()),
                        },
                    ],
                    text_input: false,
                },
                Question {
                    question: "When?".into(),
                    description: None,
                    options: Vec::new(),
                    text_input: true,
                },
            ],
        }),
    );
    fixtures.args(
        "realm/ask_chief.no_message",
        &tool(AskChief {
            task: standing_task(),
            conversation: ConversationId("c1".into()),
            message: None,
            questions: vec![Question {
                question: "Go on?".into(),
                description: None,
                options: Vec::new(),
                text_input: true,
            }],
        }),
    );
    done(
        fixtures,
        "realm/ask_chief",
        by_an_agent(
            SocietyTool::AskChief,
            vec![
                ("no_chief", AskChiefRefusal::NoChief),
                ("too_many", AskChiefRefusal::TooMany { most: 3 }),
                (
                    "too_many.once",
                    AskChiefRefusal::TooMany { most: 1 },
                ),
                ("no_questions", AskChiefRefusal::NoQuestions),
                (
                    "too_many_questions",
                    AskChiefRefusal::TooManyQuestions,
                ),
                ("blank", AskChiefRefusal::Blank { question: 2 }),
                ("crowded", AskChiefRefusal::Crowded { question: 1 }),
                (
                    "untitled",
                    AskChiefRefusal::Untitled { question: 1 },
                ),
                (
                    "twice",
                    AskChiefRefusal::Twice {
                        question: 1,
                        title: "Oats".into(),
                    },
                ),
            ],
        ),
    );
    fixtures.args(
        "realm/object",
        &tool(Object {
            task: standing_task(),
            conversation: ConversationId("c1".into()),
            reason: "The field is better left alone".into(),
        }),
    );
    done(
        fixtures,
        "realm/object",
        by_an_agent(
            SocietyTool::Object,
            vec![
                ("not_a_chiefs_line", ObjectRefusal::NotAChiefsLine),
                ("no_reason", ObjectRefusal::NoReason),
                ("already_held", ObjectRefusal::AlreadyHeld),
            ],
        ),
    );
}

fn the_budget_read(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/read_budget",
        &tool(ReadBudget {
            task: Some(standing_task()),
        }),
    );
    fixtures.args(
        "realm/read_budget.own",
        &tool(ReadBudget { task: None }),
    );
    answers(
        fixtures,
        "realm/read_budget",
        [
            (
                "budget",
                BudgetRead::Budget {
                    spend: Spend {
                        payer: Payer::Hamlet { hamlet: hamlet() },
                        spent_micros: 250_000,
                        budget_micros: Some(5_000_000),
                        spent_ms: 12_000,
                        budget_ms: None,
                    },
                    period: Period::Day,
                    own_micros: 40_000,
                    allowance_micros: Some(2_000_000),
                },
            ),
            (
                "budget.month",
                BudgetRead::Budget {
                    spend: Spend {
                        payer: Payer::Realm,
                        spent_micros: 1_250_000,
                        budget_micros: None,
                        spent_ms: 90_000,
                        budget_ms: None,
                    },
                    period: Period::Month,
                    own_micros: 0,
                    allowance_micros: None,
                },
            ),
        ],
        by_an_agent(
            SocietyTool::ReadBudget,
            vec![("no_such_task", ReadBudgetRefusal::NoSuchTask)],
        ),
    );
}

fn the_workers(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_start_worker",
        &asked(StartWorker {
            brief: "Check the links in the docs".into(),
            tools: vec![named_tool("read"), named_tool("bash")],
            task: None,
            hamlet: Some(hamlet()),
        }),
    );
    fixtures.args(
        "realm/start_worker",
        &tool(StartWorker {
            brief: "Check the links in the docs".into(),
            tools: vec![named_tool("read")],
            task: Some(standing_task()),
            hamlet: None,
        }),
    );
    fn started<W>() -> [(&'static str, Started<W>); 1] {
        [(
            "started",
            Started::Started {
                worker: AgentNumber(15),
            },
        )]
    }
    let worker_refusals = || {
        vec![
            ("empty_brief", WorkerRefusal::EmptyBrief),
            (
                "no_such_tool",
                WorkerRefusal::NoSuchTool {
                    name: "hire#2".into(),
                    tool: "fly".into(),
                },
            ),
            ("a_society_tool", WorkerRefusal::ASocietyTool),
            ("a_worker", WorkerRefusal::AWorker),
            (
                "hamlet_from_an_agent",
                WorkerRefusal::HamletFromAnAgent,
            ),
            ("no_such_hamlet", WorkerRefusal::NoSuchHamlet),
        ]
    };
    answers(
        fixtures,
        "realm/chief_start_worker",
        started(),
        worker_refusals(),
    );
    answers(
        fixtures,
        "realm/start_worker",
        started(),
        by_an_agent(SocietyTool::StartWorker, worker_refusals()),
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
            micros: Some(Count(2_500_000)),
            why: Some("small".into()),
        }),
    );
    fixtures.args(
        "realm/set_allowance",
        &tool(SetAllowance {
            hire: hire(),
            micros: Some(Count(5_000_000_000)),
            why: Some(WHY.into()),
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
    let allowance_refusals = || {
        vec![
            (
                "no_such_agent",
                OnlyAHire::NoSuchAgent {
                    address: "hire#9".into(),
                },
            ),
            (
                "not_a_hire",
                OnlyAHire::NotAHire {
                    name: "hand".into(),
                },
            ),
            (
                "left",
                OnlyAHire::Left {
                    hire: HIRE,
                    left: left(HowEnded::ByTheChief),
                },
            ),
        ]
    };
    done(fixtures, "realm/chief_set_allowance", allowance_refusals());
    done(
        fixtures,
        "realm/set_allowance",
        by_a_lead(SocietyTool::SetAllowance, allowance_refusals()),
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
    let no_such_hamlet = || {
        (
            "no_such_hamlet",
            HamletRefusal::NoSuchHamlet { id: hamlet() },
        )
    };
    done(
        fixtures,
        "realm/chief_name_hamlet",
        vec![
            no_such_hamlet(),
            (
                "bad_name",
                HamletRefusal::BadName {
                    name: "Mill: east".into(),
                },
            ),
        ],
    );
    done(
        fixtures,
        "realm/chief_move_hamlet",
        vec![
            no_such_hamlet(),
            (
                "bad_site",
                HamletRefusal::BadSite {
                    site: "NaN, 4".into(),
                },
            ),
        ],
    );
    done(fixtures, "realm/chief_set_mode", vec![no_such_hamlet()]);
    done(
        fixtures,
        "realm/chief_set_folder",
        vec![
            no_such_hamlet(),
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
    fixtures.args(
        "realm/read_record",
        &tool(ReadRecord {
            hamlet: Some(hamlet()),
        }),
    );
    for name in ["realm/chief_read_record", "realm/read_record"] {
        answers(
            fixtures,
            name,
            [(
                "read",
                Record::Read {
                    lines: vec![RecordLine {
                        text: "- hire#2 (ada) laid a road from \
                               hire#3 to hire#4: to share the tests"
                            .into(),
                    }],
                },
            )],
            vec![no_such_hamlet()],
        );
    }
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
                "not_a_hire",
                ChoiceRefusal::NotAHire {
                    name: "hand".into(),
                },
            ),
            (
                "no_such_model",
                ChoiceRefusal::NoSuchModel {
                    model: "missing::model".into(),
                },
            ),
            ("chief_chose", ChoiceRefusal::ChiefChose { hire: HIRE }),
            (
                "hamlet_from_an_agent",
                ChoiceRefusal::HamletFromAnAgent,
            ),
        ]
    };
    done(fixtures, "realm/chief_set_nickname", refusals());
    done(fixtures, "realm/chief_set_model", refusals());
    fixtures.args(
        "realm/set_nickname",
        &tool(SetNickname {
            hire: hire(),
            name: Some("Spade".into()),
            why: None,
        }),
    );
    fixtures.args(
        "realm/set_model",
        &tool(SetModel {
            of: ModelOf::Hire { hire: hire() },
            model: Some("anthropic::claude-opus".into()),
            effort: Some(Effort::Low),
            fast: Some(false),
            why: Some(WHY.into()),
        }),
    );
    for (name, tool) in [
        ("realm/set_nickname", SocietyTool::SetNickname),
        ("realm/set_model", SocietyTool::SetModel),
    ] {
        done(fixtures, name, by_a_lead(tool, refusals()));
    }
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

fn the_settings_and_the_catalogue(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/chief_edit_settings",
        &asked(EditSettings {
            set: vec![
                Setting::Model {
                    model: "anthropic::claude-opus".into(),
                },
                Setting::Effort {
                    effort: Some(Effort::Medium),
                },
                Setting::Effort { effort: None },
                Setting::ReflectEvery { turns: 20 },
                Setting::AllowNet { allow: true },
                Setting::Newcomers {
                    newcomers: Newcomers::Open,
                },
                Setting::Newcomers {
                    newcomers: Newcomers::Chief,
                },
                Setting::Newcomers {
                    newcomers: Newcomers::Hand,
                },
                Setting::Character {
                    text: "You are careful.".into(),
                },
                Setting::Endpoints {
                    endpoints: vec![Endpoint {
                        name: "lab".into(),
                        base_url: "http://10.0.0.5:8080/v1".into(),
                    }],
                },
                Setting::Workspace {
                    workspace: Some(RemoteWorkspace {
                        url: "https://builder.example".into(),
                        token: "made-up-token".into(),
                    }),
                },
                Setting::Workspace { workspace: None },
            ],
        }),
    );
    done(
        fixtures,
        "realm/chief_edit_settings",
        vec![
            ("blank_character", SettingRefusal::BlankCharacter),
            ("long_character", SettingRefusal::LongCharacter),
            ("endpoint_name", SettingRefusal::EndpointName),
            (
                "endpoint_twice",
                SettingRefusal::EndpointTwice { name: "lab".into() },
            ),
            (
                "not_an_address",
                SettingRefusal::NotAnAddress {
                    address: "lab".into(),
                },
            ),
            (
                "not_http",
                SettingRefusal::NotHttp {
                    address: "ftp://lab".into(),
                },
            ),
            ("address_holds_key", SettingRefusal::AddressHoldsKey),
        ],
    );
    fixtures.args(
        "realm/chief_edit_catalogue",
        &asked(EditCatalogue {
            model: "lab::qwen".into(),
            set: vec![
                CardSetting::Images {
                    images: Some(ImageLimits {
                        max_bytes: Some(1_048_576),
                        max_height: Some(1_024),
                        max_width: Some(1_024),
                        per_line: Some(4),
                    }),
                },
                CardSetting::Images { images: None },
                CardSetting::Note {
                    note: "Fast but forgetful".into(),
                },
                CardSetting::Price {
                    price: Some(Price {
                        input: 0.5,
                        output: 1.5,
                        cache_read: 0.05,
                        cache_write: 0.6,
                    }),
                },
                CardSetting::Price { price: None },
            ],
        }),
    );
    done(
        fixtures,
        "realm/chief_edit_catalogue",
        vec![
            (
                "no_such_model",
                CatalogueRefusal::NoSuchModel {
                    model: "lab::qwen".into(),
                },
            ),
            ("negative_cost", CatalogueRefusal::NegativeCost),
            ("zero_image_limit", CatalogueRefusal::ZeroImageLimit),
        ],
    );
}
