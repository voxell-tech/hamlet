//! The fixtures of a client's calls to an agent about its chat, of
//! the realm's copies to an agent, and of the agent's calls to the
//! realm about its model.

use hamlet_messages::agent::chat::{
    Began, ChangeLine, ChatEvent, ChatFile, ChatImage, ChatItem,
    ChatOp, ChatPage, ChatRef, ChatTool, Check, Checked, Count,
    History, LineMark, ListedImages, ListedModel, Login, LoginAnswer,
    ModelList, NewSession, Prompt, PromptSection, ReadChat,
    ReadHistory, ReadModels, ReadPrompt, Reflect, Reflecting, Resume,
    ResumeAnswer, ResumeRefusal, Retry, RetryRefusal, Rewind,
    RewindAnswer, RewindRefusal, Session, Settle, ToolChange, Usage,
};
use hamlet_messages::agent::copies::{
    AgentSettings, Copies, CopyVersion, ModelChoice, Price, Versioned,
};
use hamlet_messages::bytes::Sha256;
use hamlet_messages::realm::Done;
use hamlet_messages::realm::documents::Endpoint;
use hamlet_messages::realm::models::{
    Cause, Credential, CredentialAnswer, CredentialRefusal,
    KeepCredential, Kept, OauthLogin, Settings, TurnFailed,
};
use hamlet_messages::workspace::{
    AgentNumber, ConversationId, RealmTask,
};
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
        tagged::<Checked>(),
        tagged::<CredentialAnswer>(),
        tagged::<CredentialRefusal>(),
        tagged::<Kept>(),
        named::<Reflecting>(),
        named::<LineMark>(),
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

fn usage() -> Usage {
    Usage {
        calls: Count(2),
        unreported: Count(0),
        input: Count(1200),
        cached: Count(10),
        output: Count(340),
        reasoning: Count(0),
        ms: Count(3_100),
        micros: Count(4_500),
        last_input: Count(600),
        summary: "2 calls, $0.0045".into(),
    }
}

fn images() -> Vec<ChatImage> {
    vec![
        ChatImage {
            at: Some(6),
            id: Sha256([0xab; 32]),
        },
        ChatImage {
            at: None,
            id: Sha256([0xcd; 32]),
        },
    ]
}

fn files() -> Vec<ChatFile> {
    vec![
        ChatFile {
            at: Some(0),
            id: Sha256([0x12; 32]),
            name: "notes/plan.md".into(),
            size: 2_048,
        },
        ChatFile {
            at: None,
            id: Sha256([0x34; 32]),
            name: "log.txt".into(),
            size: 0,
        },
    ]
}

fn tool() -> ChatTool {
    ChatTool {
        name: "edit".into(),
        summary: "src/lib.rs".into(),
        exit: Some(-1_i32 as u32),
        ms: Some(840),
        failure: Some("stopped".into()),
        change: Some(ToolChange {
            path: "/work/project/src/lib.rs".into(),
            created: false,
            added: 1,
            removed: 1,
            total: 3,
            count: "+1 -1".into(),
            lines: [
                (LineMark::Same, "fn main() {"),
                (LineMark::Removed, "    old();"),
                (LineMark::Added, "    new();"),
            ]
            .into_iter()
            .map(|(mark, text)| ChangeLine {
                mark,
                text: text.into(),
            })
            .collect(),
        }),
    }
}

fn item(id: &str, kind: protocol::ItemKind, text: &str) -> ChatItem {
    ChatItem {
        id: id.into(),
        at: Count(1_763_200_000_123),
        kind,
        text: text.into(),
        author: None,
        chief: None,
        images: Vec::new(),
        files: Vec::new(),
        rewindable: None,
        tool: None,
    }
}

fn the_clients_calls(fixtures: &mut Fixtures) {
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
            images: images(),
            files: files(),
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
    fixtures.event("agent/chat.event", &event());
    for (name, chat) in [
        (
            "letters",
            ChatRef {
                from: Some(AgentNumber(3)),
                kind: Some(protocol::ChatRefKind::Letters),
                ..chat_of()
            },
        ),
        (
            "past",
            ChatRef {
                past: Some(1),
                ..chat_of()
            },
        ),
    ] {
        fixtures.event(
            &format!("agent/chat.event.{name}"),
            &ChatEvent {
                chat,
                epoch: 3,
                number: 43,
                ops: vec![ChatOp::Reset],
            },
        );
    }
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
                tokens: Count(9_300),
                title: "The tests".into(),
            }],
        },
    );
    fixtures.args("agent/read_prompt", &ReadPrompt);
    fixtures.answer(
        "agent/read_prompt",
        &Prompt {
            sections: vec![
                PromptSection {
                    name: "charter".into(),
                    text: "Be brief.".into(),
                },
                PromptSection {
                    name: "place".into(),
                    text: "# Your place\nYou are `hire#12`.".into(),
                },
            ],
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
        ("not_the_hand", LoginAnswer::NotTheHand),
    ] {
        fixtures.answer(&format!("agent/login.{arm}"), &answer);
    }
    fixtures.args("agent/settle", &Settle);
    fixtures.args("agent/read_models", &ReadModels { refresh: true });
    fixtures.answer(
        "agent/read_models",
        &ModelList {
            models: vec![
                ListedModel {
                    name: "anthropic::claude-sonnet-4-5".into(),
                    title: "Claude Sonnet 4.5".into(),
                    context: 200_000,
                    price: Some(Price {
                        input: 3.0,
                        output: 15.0,
                        cache_read: 0.3,
                        cache_write: 3.75,
                    }),
                    images: Some(ListedImages {
                        max_bytes: Some(5_242_880),
                        max_height: Some(8000),
                        max_width: Some(8000),
                        per_line: Some(100),
                    }),
                    efforts: vec![Effort::Off],
                    fast: false,
                },
                ListedModel {
                    name: "local::small".into(),
                    title: "Small".into(),
                    context: 8192,
                    price: None,
                    images: Some(ListedImages::default()),
                    efforts: vec![Effort::Off],
                    fast: false,
                },
            ],
        },
    );
    fixtures.args(
        "agent/check",
        &Check {
            model: Some("anthropic::claude-sonnet-4-5".into()),
        },
    );
    fixtures.args("agent/check.running", &Check { model: None });
    for (arm, answer) in [
        (
            "connected",
            Checked::Answered {
                words: "Connected, 1 model(s)".into(),
                models: Some(1),
            },
        ),
        (
            "answered",
            Checked::Answered {
                words: "Hello.".into(),
                models: None,
            },
        ),
        (
            "failed",
            Checked::Failed {
                words: "the provider answered 401".into(),
            },
        ),
        (
            "refused",
            Checked::Refused {
                words: "no model is named `nope`".into(),
            },
        ),
    ] {
        fixtures.answer(&format!("agent/check.{arm}"), &answer);
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
        .map(|(id, kind, text)| match kind {
            User => ChatItem {
                images: images(),
                files: files(),
                ..item(id, kind, text)
            },
            Tool => ChatItem {
                tool: Some(tool()),
                ..item(id, kind, text)
            },
            _ => item(id, kind, text),
        })
        .chain([ChatItem {
            tool: Some(ChatTool {
                name: "read".into(),
                summary: "src/main.rs".into(),
                exit: None,
                ms: None,
                failure: None,
                change: None,
            }),
            ..item("i3b", Tool, "read src/main.rs")
        }])
        .collect(),
        earlier: Some("i0".into()),
        working: false,
        pending: 0,
        usage: usage(),
    }
}

fn chat_of() -> ChatRef {
    ChatRef {
        with: AgentNumber(TO.0 as u32),
        from: None,
        kind: None,
        past: None,
    }
}

fn event() -> ChatEvent {
    ChatEvent {
        chat: chat_of(),
        epoch: 3,
        number: 42,
        ops: vec![
            ChatOp::Add {
                item: Box::new(ChatItem {
                    rewindable: Some(true),
                    chief: Some("ada".into()),
                    images: images(),
                    files: files(),
                    ..item("i8", protocol::ItemKind::User, "Go on")
                }),
                replaces: None,
            },
            ChatOp::Add {
                item: Box::new(ChatItem {
                    author: Some("hire#3 (Ada)".into()),
                    ..item("i8b", protocol::ItemKind::User, "Hello.")
                }),
                replaces: None,
            },
            ChatOp::Add {
                item: Box::new(item(
                    "i8c",
                    protocol::ItemKind::Assistant,
                    "Going on.",
                )),
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

/// Every part filled, at its version.
pub(super) fn copies() -> Copies {
    Copies {
        charter: Some(Versioned {
            version: CopyVersion(3),
            value: "Be brief.".into(),
        }),
        model: Some(Versioned {
            version: CopyVersion(5),
            value: ModelChoice {
                model: "anthropic::claude-opus".into(),
                effort: Some(Effort::High),
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
            version: CopyVersion(1),
            value: AgentSettings {
                reflect_every: 20,
                allow_net: false,
                endpoints: vec![Endpoint {
                    name: "local".into(),
                    base_url: "http://127.0.0.1:8080/v1".into(),
                }],
            },
        }),
    }
}

fn the_copies(fixtures: &mut Fixtures) {
    use Effort::{Low, Max, Medium, Minimal, Off, XHigh};
    fixtures.args("agent/copies", &copies());
    for effort in [Off, Minimal, Low, Medium, XHigh, Max] {
        let name = serde_json::to_value(effort).unwrap();
        fixtures.args(
            &format!("agent/copies.{}", name.as_str().unwrap()),
            &Copies {
                model: Some(Versioned {
                    version: CopyVersion(6),
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
    let settings = || {
        [("ACCOUNT_ID", "acc-1"), ("GATEWAY_ID", "gateway-1")]
            .map(|(name, value)| (name.into(), value.into()))
            .into()
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
                env: Settings::new(),
            },
        ),
        (
            "key.settings",
            CredentialAnswer::Key {
                key: "sk-test".into(),
                env: settings(),
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
                env: Settings::new(),
            },
        ),
        (
            "key.settings",
            Kept::Key {
                key: "sk-test".into(),
                env: settings(),
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

/// The agent's encoder (cbor-x 1.6.9, through the host's
/// `encodeCborCompat`) writes a `chat` event whose usage holds
/// 2^32 + 5 and 2^53. As decimal text they read exactly; as numbers
/// it writes them as floats, which do not read.
#[test]
fn a_count_past_2_32_reads_exactly() {
    const TEXT: &str = "b900046463686174b9000164776974680c6565706f636803666e756d626572182c636f707381b900026474797065657573616765657573616765b9000a6563616c6c7361326a756e7265706f72746564613065696e7075747039303037313939323534373430393932666361636865646a34323934393637333031666f75747075746333343069726561736f6e696e676130626d736433313030666d6963726f7364343530306a6c6173745f696e707574633630306773756d6d61727967322063616c6c73";
    const NUMBERS: &str = "b900046463686174b9000164776974680c6565706f636803666e756d626572182c636f707381b900026474797065657573616765657573616765b9000a6563616c6c73026a756e7265706f727465640065696e707574fb434000000000000066636163686564fb41f0000000500000666f757470757419015469726561736f6e696e6700626d73190c1c666d6963726f731911946a6c6173745f696e7075741902586773756d6d61727967322063616c6c73";
    let bytes = |hex: &str| {
        (0..hex.len())
            .step_by(2)
            .map(|at| {
                u8::from_str_radix(&hex[at..at + 2], 16).unwrap()
            })
            .collect::<Vec<_>>()
    };
    let read =
        ciborium::from_reader::<ChatEvent, _>(bytes(TEXT).as_slice())
            .unwrap();
    let [ChatOp::Usage { usage }] = read.ops.as_slice() else {
        panic!("{read:?} holds one usage");
    };
    assert_eq!(
        (usage.cached, usage.input),
        (Count((1 << 32) + 5), Count(1 << 53))
    );
    let numbers = ciborium::from_reader::<serde_json::Value, _>(
        bytes(NUMBERS).as_slice(),
    )
    .unwrap();
    for count in ["cached", "input"] {
        let written = &numbers["ops"][0]["usage"][count];
        assert!(written.is_f64(), "{count} is {written}");
        assert!(
            serde_json::from_value::<u64>(written.clone()).is_err()
        );
    }
}
