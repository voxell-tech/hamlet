//! The fixtures of a client's calls to an agent about its chat, of
//! the realm's copies to an agent, and of the agent's calls to the
//! realm about its model.

use hamlet_messages::agent::chat::{
    Began, ChatEvent, ChatItem, ChatOp, ChatPage, History, Login,
    LoginAnswer, NewSession, Prompt, ReadChat, ReadHistory,
    ReadPrompt, Reflect, Reflecting, Resume, ResumeAnswer,
    ResumeRefusal, Retry, RetryRefusal, Rewind, RewindAnswer,
    RewindRefusal, Session, Stop, Stopped,
};
use hamlet_messages::agent::copies::{
    AgentSettings, Copies, ModelChoice, Price, Versioned,
};
use hamlet_messages::realm::Done;
use hamlet_messages::realm::models::{
    Cause, Credential, CredentialAnswer, CredentialRefusal,
    KeepCredential, Kept, OauthLogin, TurnFailed,
};
use hamlet_messages::workspace::{ConversationId, RealmTask};
use hamlet_protocol::Effort;
use hamlet_protocol::client::{self as protocol, RequestId};

use super::Fixtures;
use super::cases::{Cases, named, tagged};

pub fn enums() -> Vec<Cases> {
    vec![
        tagged::<ResumeAnswer>(),
        tagged::<ResumeRefusal>(),
        tagged::<RetryRefusal>(),
        tagged::<RewindAnswer>(),
        tagged::<RewindRefusal>(),
        tagged::<ChatOp>(),
        tagged::<LoginAnswer>(),
        tagged::<CredentialAnswer>(),
        tagged::<CredentialRefusal>(),
        tagged::<Kept>(),
        named::<Reflecting>(),
        named::<Cause>(),
        named::<protocol::ItemKind>(),
        named::<protocol::ChatRefKind>(),
        named::<Effort>(),
    ]
}

pub fn the_agents_chat(fixtures: &mut Fixtures) {
    the_clients_calls(fixtures);
    the_copies(fixtures);
    the_calls_about_the_model(fixtures);
}

fn id(n: u8) -> RequestId {
    format!("0b5e7c4a-1d2f-4e3a-8b9c-0d1e2f3a4b{n:02x}")
        .parse()
        .unwrap()
}

const TO: protocol::AgentId = protocol::AgentId(12);

fn chat() -> protocol::ChatRef {
    protocol::ChatRef {
        from: None,
        kind: None,
        past: None,
        with: TO,
    }
}

fn usage() -> protocol::UsageView {
    protocol::UsageView {
        cached: 10,
        calls: 2,
        input: 1200,
        last_input: 600,
        micros: 4_500,
        ms: 3_100,
        output: 340,
        reasoning: 0,
        summary: "2 calls, $0.0045".into(),
        unreported: 0,
    }
}

fn item(id: &str, kind: protocol::ItemKind, text: &str) -> ChatItem {
    ChatItem {
        id: id.into(),
        at: 1_763_200_000,
        kind,
        text: text.into(),
        author: None,
        images: Vec::new(),
        files: Vec::new(),
        rewindable: None,
        tool: None,
    }
}

fn the_clients_calls(fixtures: &mut Fixtures) {
    fixtures.args(
        "agent/stop",
        &Stop(protocol::Stop {
            agent: Some(TO),
            id: id(1),
        }),
    );
    fixtures.answer("agent/stop", &Stopped { turns: 1 });
    fixtures.args(
        "agent/new_session",
        &NewSession(protocol::New { id: id(2), to: TO }),
    );
    fixtures.answer("agent/new_session", &Began { number: 4 });
    fixtures.args(
        "agent/resume",
        &Resume(protocol::Resume {
            id: id(3),
            number: 2,
            to: TO,
        }),
    );
    fixtures.answer(
        "agent/resume.back",
        &ResumeAnswer::Back { number: 2 },
    );
    fixtures.answer(
        "agent/resume.refused.no_such_session",
        &ResumeAnswer::Refused {
            why: ResumeRefusal::NoSuchSession { number: 9 },
        },
    );
    fixtures.args(
        "agent/retry",
        &Retry(protocol::Retry { id: id(4), to: TO }),
    );
    fixtures.answer("agent/retry.done", &Done::<RetryRefusal>::Done);
    for (case, why) in [
        ("running", RetryRefusal::Running),
        ("answered", RetryRefusal::Answered),
        ("no_turn", RetryRefusal::NoTurn),
    ] {
        fixtures.answer(
            &format!("agent/retry.refused.{case}"),
            &Done::Refused { why },
        );
    }
    fixtures.args(
        "agent/rewind",
        &Rewind(protocol::Rewind {
            chat: chat(),
            id: id(5),
            item: "i7".into(),
        }),
    );
    fixtures.answer(
        "agent/rewind.rewound",
        &RewindAnswer::Rewound {
            text: "Write the tests".into(),
            images: Vec::new(),
            files: Vec::new(),
        },
    );
    for (case, why) in [
        ("not_a_chief_line", RewindRefusal::NotAChiefLine),
        (
            "no_such_item",
            RewindRefusal::NoSuchItem { item: "i9".into() },
        ),
        ("not_current", RewindRefusal::NotCurrent),
        ("working", RewindRefusal::Working),
    ] {
        fixtures.answer(
            &format!("agent/rewind.refused.{case}"),
            &RewindAnswer::Refused { why },
        );
    }
    fixtures.args(
        "agent/read_chat",
        &ReadChat(protocol::Open {
            before: Some("i3".into()),
            chat: protocol::ChatRef {
                from: Some(protocol::AgentId(3)),
                kind: Some(protocol::ChatRefKind::Letters),
                past: Some(1),
                with: TO,
            },
            id: id(6),
        }),
    );
    fixtures.args(
        "agent/read_chat.questions",
        &ReadChat(protocol::Open {
            before: None,
            chat: protocol::ChatRef {
                from: Some(protocol::AgentId(3)),
                kind: Some(protocol::ChatRefKind::Questions),
                past: None,
                with: TO,
            },
            id: id(8),
        }),
    );
    fixtures.answer("agent/read_chat", &page());
    fixtures.answer("agent/chat.event", &event());
    fixtures.args("agent/read_history", &ReadHistory);
    fixtures.answer(
        "agent/read_history",
        &History {
            sessions: vec![Session {
                number: 2,
                began: 1_763_100_000,
                modified: 1_763_200_000,
                current: true,
                messages: 14,
                tokens: 9_300,
                title: "The tests".into(),
            }],
        },
    );
    fixtures.args("agent/read_prompt", &ReadPrompt);
    fixtures.answer(
        "agent/read_prompt",
        &Prompt {
            text: "You are `hire#12`.".into(),
        },
    );
    fixtures.args("agent/reflect", &Reflect);
    fixtures.answer("agent/reflect.asked", &Reflecting::Asked);
    fixtures.answer("agent/reflect.already", &Reflecting::Already);
    fixtures.unprobed_args(
        "agent/login",
        &Login(protocol::Login::Start {
            id: id(7),
            start: protocol::LoginStart {
                method: protocol::LoginMethod::Subscription,
                provider: "anthropic".into(),
            },
        }),
    );
    for (arm, answer) in [
        (
            "step",
            LoginAnswer::Step {
                login: "l1".into(),
                step: protocol::LoginStep::Code {
                    code: "ABCD-1234".into(),
                },
            },
        ),
        ("done", LoginAnswer::Done { login: "l1".into() }),
        (
            "failed",
            LoginAnswer::Failed {
                login: "l1".into(),
                words: "the code expired".into(),
            },
        ),
    ] {
        fixtures.answer(&format!("agent/login.{arm}"), &answer);
    }
}

fn page() -> ChatPage {
    use protocol::ItemKind::{
        Answered, Assistant, Error, Notice, Reasoning, Tool, User,
    };
    ChatPage {
        epoch: 3,
        number: 41,
        session: Some(2),
        items: [
            ("i1", User, "Write the tests"),
            ("i2", Reasoning, "Read the crate first"),
            ("i3", Tool, "read src/lib.rs"),
            ("i4", Assistant, "Done: three tests"),
            ("i5", Notice, "The charter changed"),
            ("i6", Error, "the model failed"),
            ("i7", Answered, "Allowed"),
        ]
        .into_iter()
        .map(|(id, kind, text)| item(id, kind, text))
        .collect(),
        earlier: Some("i0".into()),
        working: false,
        pending: 0,
        usage: usage(),
    }
}

fn event() -> ChatEvent {
    ChatEvent {
        chat: chat(),
        epoch: 3,
        number: 42,
        ops: vec![
            ChatOp::Add {
                item: Box::new(ChatItem {
                    author: Some("hire#3".into()),
                    rewindable: Some(true),
                    ..item("i8", protocol::ItemKind::User, "Go on")
                }),
                replaces: Some("p1".into()),
            },
            ChatOp::Append {
                item: "i9".into(),
                at: 12,
                text: " and more".into(),
            },
            ChatOp::Status {
                working: true,
                pending: 1,
            },
            ChatOp::Usage { usage: usage() },
            ChatOp::Reset,
        ],
    }
}

fn the_copies(fixtures: &mut Fixtures) {
    use Effort::{High, Low, Max, Medium, Minimal, Off, XHigh};
    fixtures.args(
        "agent/copies",
        &Copies {
            charter: Some(Versioned {
                version: 3,
                value: "Be brief.".into(),
            }),
            character: Some(Versioned {
                version: 2,
                value: "Calm.".into(),
            }),
            model: Some(Versioned {
                version: 5,
                value: ModelChoice {
                    model: "anthropic::claude-opus".into(),
                    effort: Some(High),
                    fast: true,
                    price: Some(Price {
                        input: 15.0,
                        output: 75.0,
                        cache_read: 1.5,
                        cache_write: 18.75,
                    }),
                },
            }),
            settings: Some(Versioned {
                version: 1,
                value: AgentSettings {
                    reflect_every: 20,
                    allow_net: false,
                },
            }),
        },
    );
    for effort in [Off, Minimal, Low, Medium, XHigh, Max] {
        let name = serde_json::to_value(effort).unwrap();
        fixtures.args(
            &format!("agent/copies.{}", name.as_str().unwrap()),
            &Copies {
                model: Some(Versioned {
                    version: 6,
                    value: ModelChoice {
                        model: "faux::faux-1".into(),
                        effort: Some(effort),
                        fast: false,
                        price: None,
                    },
                }),
                ..Copies::default()
            },
        );
    }
    fixtures.answer("agent/copies", &());
}

fn the_calls_about_the_model(fixtures: &mut Fixtures) {
    let login = || OauthLogin {
        access: "access-token".into(),
        refresh: "refresh-token".into(),
        expires: 1_763_203_600,
        extra: Some("{\"account\":\"a1\"}".into()),
    };
    fixtures.args(
        "realm/credential",
        &Credential {
            provider: "anthropic".into(),
        },
    );
    for (arm, answer) in [
        (
            "key",
            CredentialAnswer::Key {
                key: "sk-test".into(),
            },
        ),
        ("oauth", CredentialAnswer::Oauth { login: login() }),
        (
            "refused.not_signed_in",
            CredentialAnswer::Refused {
                why: CredentialRefusal::NotSignedIn {
                    provider: "anthropic".into(),
                },
            },
        ),
    ] {
        fixtures.answer(&format!("realm/credential.{arm}"), &answer);
    }
    for (case, value) in [
        (
            "key",
            Kept::Key {
                key: "sk-test".into(),
            },
        ),
        ("oauth", Kept::Oauth { login: login() }),
    ] {
        fixtures.args(
            &format!("realm/keep_credential.{case}"),
            &KeepCredential {
                provider: "anthropic".into(),
                value,
            },
        );
    }
    fixtures.answer("realm/keep_credential", &());
    for (case, cause) in
        [("model", Cause::Model), ("budget", Cause::Budget)]
    {
        fixtures.args(
            &format!("realm/turn_failed.{case}"),
            &TurnFailed {
                task: RealmTask("task-2".into()),
                conversation: ConversationId("c1".into()),
                words: "the model answered 529".into(),
                cause,
            },
        );
    }
    fixtures.answer("realm/turn_failed", &());
}
