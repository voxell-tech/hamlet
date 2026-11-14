//! The fixtures: each message's bytes as the host carries them. A
//! call's arguments are encoded as the authoring layer encodes them,
//! and an answer as it encodes an action's output. A message whose
//! bytes changed rewrites its fixture and fails once, so the change
//! is committed with it. Every case of every enum a message holds
//! has a fixture, and no field is written as `null`.

mod cases;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use ciborium::Value;
use hamlet_actor::action::{decode_positional, encode_positional};
use hamlet_messages::agent::{Finished, Outcome};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::{NeedsChief, SecretList, Secrets};
use hamlet_messages::workspace::files::{
    Change, ChangeAnswer, CutBy, EditDiff, EditFile, FileAnswer,
    Find, Hashed, Image, Note, ReadFile, Replace, Search, Severity,
    Text, Truncation, WriteFile,
};
use hamlet_messages::workspace::reach::{
    Access, Footing, Mode, Need, Project, Reach, Refusal,
    SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AddedTool, AgentNumber, Answer, AnswerTaken, CallId, CallRefusal,
    CallerRefusal, Cancel, CancelAnswer, Changed, ChiefAnswer,
    CommandEnd, CommandId, CommandStatus, ConversationId, Cut, Exit,
    Lapse, ListTools, Program, ReachAnswer, ReachVersion,
    RealmRefusal, RealmTask, Run, RunAnswer, SetReach, ToolList,
    Which,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use self::cases::{Cases, named, tagged};

const AGENT: AgentNumber = AgentNumber(7);

/// Every enum a message holds, by its cases.
fn enums() -> Vec<Cases> {
    vec![
        tagged::<ReachAnswer>(),
        tagged::<Answer>(),
        tagged::<AnswerTaken>(),
        tagged::<CallerRefusal>(),
        tagged::<Program>(),
        tagged::<RunAnswer>(),
        tagged::<Exit>(),
        tagged::<Which>(),
        tagged::<CancelAnswer>(),
        tagged::<CallRefusal>(),
        tagged::<Changed>(),
        tagged::<Outcome>(),
        tagged::<Reach>(),
        tagged::<Refusal>(),
        tagged::<FileAnswer>(),
        tagged::<ChangeAnswer>(),
        tagged::<Hashed>(),
        named::<RealmRefusal>(),
        named::<Lapse>(),
        named::<Need>(),
        named::<Mode>(),
        named::<Access>(),
        named::<Severity>(),
        named::<CutBy>(),
    ]
}

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The fixtures written so far, what changed on disk, and the enum
/// cases they hold.
#[derive(Default)]
struct Fixtures {
    written: BTreeSet<String>,
    rewritten: Vec<String>,
    nulls: Vec<String>,
    found: BTreeMap<Cases, BTreeSet<String>>,
}

impl Fixtures {
    /// Checks the arguments of a call.
    fn args<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let bytes = encode_positional(value).unwrap();
        let name = format!("{name}.args");
        self.check(&name, &bytes);
        let read = decode_positional::<T>(&bytes).unwrap();
        assert_eq!(&read, value, "{name} reads back");
        let Value::Array(arguments) =
            ciborium::from_reader::<Value, _>(bytes.as_slice())
                .unwrap()
        else {
            panic!("{name} is not a list of arguments");
        };
        for argument in &arguments {
            self.probe::<T>(&name, argument);
        }
    }

    /// Checks an answer.
    fn answer<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_value(value).unwrap();
        let mut bytes = Vec::new();
        ciborium::into_writer(&json, &mut bytes).unwrap();
        let name = format!("{name}.answer");
        self.check(&name, &bytes);
        let read =
            ciborium::from_reader::<T, _>(bytes.as_slice()).unwrap();
        assert_eq!(&read, value, "{name} reads back");
        self.probe::<T>(
            &name,
            &ciborium::from_reader::<Value, _>(bytes.as_slice())
                .unwrap(),
        );
    }

    fn probe<T: DeserializeOwned>(
        &mut self,
        name: &str,
        value: &Value,
    ) {
        if *value != Value::Null && holds_null(value) {
            self.nulls.push(name.to_owned());
        }
        for (cases, case) in cases::found::<T>(value) {
            self.found.entry(cases).or_default().insert(case);
        }
    }

    fn check(&mut self, name: &str, bytes: &[u8]) {
        self.written.insert(format!("{name}.cbor"));
        let path = folder().join(format!("{name}.cbor"));
        if std::fs::read(&path).ok().as_deref() == Some(bytes) {
            return;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        self.rewritten.push(name.to_owned());
    }

    /// Removes the fixtures no message wrote, then fails if a fixture
    /// changed or an enum has a case no fixture holds.
    fn finish(mut self) {
        for kind in std::fs::read_dir(folder()).unwrap() {
            let kind = kind.unwrap().path();
            for file in std::fs::read_dir(&kind).unwrap() {
                let file = file.unwrap().path();
                let name = file
                    .strip_prefix(folder())
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                if !self.written.contains(&name) {
                    std::fs::remove_file(&file).unwrap();
                    self.rewritten.push(format!("{name} (removed)"));
                }
            }
        }
        let enums = enums();
        let mut problems = self
            .nulls
            .iter()
            .map(|name| format!("{name} writes a field as null"))
            .collect::<Vec<_>>();
        for (at, cases) in enums.iter().enumerate() {
            if let Some(name) =
                cases.iter().find(|name| !is_snake_case(name))
            {
                problems.push(format!(
                    "{name} of {cases:?} is not snake_case"
                ));
            }
            if enums[..at].contains(cases) {
                problems.push(format!(
                    "two enums have the cases {cases:?}"
                ));
            }
            let found = self.found.get(cases);
            for case in *cases {
                if !found.is_some_and(|found| found.contains(*case)) {
                    problems.push(format!(
                        "no fixture holds {case} of {cases:?}"
                    ));
                }
            }
        }
        for cases in self.found.keys() {
            if !enums.contains(cases) {
                problems.push(format!(
                    "the enum of {cases:?} is missing from enums()"
                ));
            }
        }
        if !self.rewritten.is_empty() {
            problems.push(format!(
                "These messages' bytes changed, so their fixtures \
                 under crates/hamlet_messages/fixtures are \
                 rewritten; commit them: {}",
                self.rewritten.join(", ")
            ));
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}

fn holds_null(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.iter().any(holds_null),
        Value::Map(entries) => {
            entries.iter().any(|(_, item)| holds_null(item))
        }
        _ => false,
    }
}

fn is_snake_case(name: &str) -> bool {
    name.bytes()
        .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

#[test]
fn every_message_and_every_case_matches_its_fixture() {
    let mut fixtures = Fixtures::default();
    the_workspaces_messages(&mut fixtures);
    the_answers_about_a_command(&mut fixtures);
    the_reads(&mut fixtures);
    the_changes(&mut fixtures);
    fixtures.finish();
}

fn footing() -> Footing {
    Footing {
        project: Some(Project {
            folder: "/work/project".into(),
            mode: Mode::Manual,
        }),
        network: true,
        trust: vec![Trust {
            conversation: ConversationId("c1".into()),
            edits: true,
            commands: false,
        }],
        secrets: SecretsVersion(2),
    }
}

fn set_reach(reach: Reach) -> SetReach {
    SetReach {
        agent: AGENT,
        version: ReachVersion(3),
        reach,
    }
}

fn the_workspaces_messages(fixtures: &mut Fixtures) {
    fixtures.args(
        "workspace/set_reach.agent",
        &set_reach(Reach::Agent(footing())),
    );
    fixtures.args(
        "workspace/set_reach.worker",
        &set_reach(Reach::Worker {
            starter: Some(AgentNumber(3)),
            footing: Footing {
                project: None,
                ..footing()
            },
        }),
    );
    fixtures
        .args("workspace/set_reach.ended", &set_reach(Reach::Ended));
    fixtures.args(
        "workspace/set_reach.auto",
        &set_reach(Reach::Agent(Footing {
            project: Some(Project {
                folder: "/work/project".into(),
                mode: Mode::Auto,
            }),
            ..footing()
        })),
    );
    fixtures.answer(
        "workspace/set_reach.in_force",
        &ReachAnswer::InForce {
            version: ReachVersion(3),
        },
    );
    fixtures.answer(
        "workspace/set_reach.refused",
        &ReachAnswer::Refused {
            why: RealmRefusal::NotTheRealm,
        },
    );
    let task = Some(RealmTask("task-1".into()));
    fixtures.args(
        "workspace/run",
        &Run {
            agent: AGENT,
            call: CallId("call-1".into()),
            conversation: ConversationId("c1".into()),
            realm_task: task.clone(),
            program: Program::Script {
                script: "echo hi".into(),
                network: false,
                timeout_ms: Some(30_000),
            },
        },
    );
    fixtures.args(
        "workspace/run.tool",
        &Run {
            agent: AGENT,
            call: CallId("call-1".into()),
            conversation: ConversationId("c1".into()),
            realm_task: task.clone(),
            program: Program::Tool {
                name: "word_count".into(),
                arguments: r#"{"path":"/project/a.md"}"#.into(),
            },
        },
    );
    fixtures.args("workspace/list_tools", &ListTools);
    fixtures.answer(
        "workspace/list_tools",
        &ToolList {
            tools: vec![AddedTool {
                name: "word_count".into(),
                description: "Counts the words of a file".into(),
                parameters: r#"{"type":"object","properties":{"path":{"type":"string"}}}"#.into(),
            }],
            faults: vec![
                "/realm/tools/broken: manifest.json is not valid JSON"
                    .into(),
            ],
        },
    );
    fixtures.args(
        "workspace/write_file",
        &WriteFile {
            agent: AGENT,
            call: CallId("call-2".into()),
            conversation: ConversationId("c1".into()),
            realm_task: None,
            path: "/work/project/notes.md".into(),
            content: "hello\n".into(),
        },
    );
    fixtures.args(
        "workspace/edit_file",
        &EditFile {
            agent: AGENT,
            call: CallId("call-3".into()),
            conversation: ConversationId("c1".into()),
            realm_task: task,
            path: "/work/project/notes.md".into(),
            edits: vec![Replace {
                old: "hello".into(),
                new: "goodbye".into(),
            }],
        },
    );
}

fn command_end(exit: Exit) -> CommandEnd {
    CommandEnd {
        command: CommandId(4),
        exit,
        output: "hi\n".into(),
        cut: None,
        policy: "3f2a".into(),
        took_ms: 12,
    }
}

fn the_answers_about_a_command(fixtures: &mut Fixtures) {
    fixtures.answer(
        "workspace/run.done",
        &RunAnswer::Done(command_end(Exit::Code { code: 0 })),
    );
    fixtures.answer(
        "workspace/run.done.cut",
        &RunAnswer::Done(CommandEnd {
            cut: Some(Cut {
                total_bytes: 52_428_800,
                total_lines: 1_048_576,
                full_output: "/realm/actors/workspace/local.output/\
                              7/4.log"
                    .into(),
            }),
            ..command_end(Exit::Code { code: 0 })
        }),
    );
    fixtures.answer(
        "workspace/run.running",
        &RunAnswer::Running {
            command: CommandId(4),
        },
    );
    fixtures.answer(
        "workspace/run.waits_for_chief",
        &RunAnswer::WaitsForChief {
            needs: vec![Need::Network, Need::Command],
        },
    );
    for (name, refusal) in [
        (
            "caller",
            CallRefusal::Caller {
                why: CallerRefusal::NotTheCaller {
                    named: AGENT,
                    caller: AgentNumber(8),
                },
            },
        ),
        (
            "reach",
            CallRefusal::Reach {
                why: Refusal::OutsideReach {
                    path: "/etc".into(),
                    access: Access::Read,
                },
            },
        ),
        (
            "reused",
            CallRefusal::CallReused {
                call: CallId("call-1".into()),
            },
        ),
        ("holds_secret", CallRefusal::HoldsSecret),
        (
            "chief_refused",
            CallRefusal::ChiefRefused {
                reason: Some("not on main".into()),
            },
        ),
        (
            "lapsed",
            CallRefusal::Lapsed {
                lapse: Lapse::Rewound,
            },
        ),
        (
            "reach_changed",
            CallRefusal::ReachChanged {
                since: ReachVersion(3),
                now: ReachVersion(4),
                why: Changed::Needs {
                    needs: vec![Need::Network],
                },
            },
        ),
        (
            "reach_changed.refused",
            CallRefusal::ReachChanged {
                since: ReachVersion(3),
                now: ReachVersion(5),
                why: Changed::Refused {
                    why: Refusal::NoReach,
                },
            },
        ),
        (
            "invalid",
            CallRefusal::Invalid {
                words: "Invalid timeout: must be a positive number \
                        of seconds"
                    .into(),
            },
        ),
    ] {
        fixtures.answer(
            &format!("workspace/run.refused.{name}"),
            &RunAnswer::Refused { why: refusal },
        );
    }
    for (name, exit) in [
        ("timed_out", Exit::TimedOut),
        (
            "failed",
            Exit::Failed {
                words: "bash: command not found".into(),
            },
        ),
        ("cut_off", Exit::CutOff),
        ("cancelled", Exit::Cancelled),
    ] {
        fixtures.answer(
            &format!("workspace/run.done.{name}"),
            &RunAnswer::Done(command_end(exit)),
        );
    }
    fixtures.args(
        "workspace/command_status",
        &CommandStatus {
            agent: AGENT,
            command: CommandId(4),
        },
    );
    fixtures.answer(
        "workspace/command_status.refused.no_such_command",
        &RunAnswer::Refused {
            why: CallRefusal::NoSuchCommand {
                command: CommandId(9),
            },
        },
    );
    for (name, which) in [
        (
            "one",
            Which::One {
                call: CallId("call-1".into()),
            },
        ),
        (
            "conversation",
            Which::Conversation {
                conversation: ConversationId("c1".into()),
            },
        ),
        ("all", Which::All),
    ] {
        fixtures.args(
            &format!("workspace/cancel.{name}"),
            &Cancel {
                agent: AGENT,
                which,
            },
        );
    }
    fixtures.answer(
        "workspace/cancel.cancelled",
        &CancelAnswer::Cancelled {
            commands: vec![CommandId(4), CommandId(5)],
        },
    );
    fixtures.answer(
        "workspace/cancel.refused",
        &CancelAnswer::Refused {
            why: CallerRefusal::NotTheCaller {
                named: AGENT,
                caller: AgentNumber(8),
            },
        },
    );
    fixtures.answer(
        "workspace/cancel.refused.not_an_agent",
        &CancelAnswer::Refused {
            why: CallerRefusal::NotAnAgent,
        },
    );
    fixtures.args(
        "agent/finished",
        &Finished {
            call: CallId("call-1".into()),
            outcome: Outcome::Ran(command_end(Exit::Code {
                code: 1,
            })),
        },
    );
    fixtures.args(
        "agent/finished.changed",
        &Finished {
            call: CallId("call-2".into()),
            outcome: Outcome::Changed {
                answer: ChangeAnswer::Changed(Change {
                    path: "/work/project/notes.md".into(),
                    created: true,
                    hash: Sha256([7; 32]),
                    diff: None,
                }),
            },
        },
    );
    fixtures.args(
        "agent/finished.not_run",
        &Finished {
            call: CallId("call-3".into()),
            outcome: Outcome::NotRun {
                why: CallRefusal::ChiefRefused { reason: None },
            },
        },
    );
    fixtures.args(
        "realm/needs_chief",
        &NeedsChief {
            agent: AGENT,
            call: CallId("call-1".into()),
            conversation: ConversationId("conversation-1".into()),
            realm_task: Some(RealmTask("task-9".into())),
            needs: vec![Need::Edit],
            subject: "/work/project/notes.md\nReplace:\nhello\nWith:\
                      \ngoodbye"
                .into(),
            change: Some(EditDiff {
                diff: "-1 hello\n+1 goodbye".into(),
                patch: "--- notes.md\n+++ notes.md\n@@ -1 +1 \
                        @@\n-hello\n+goodbye\n"
                    .into(),
                first_changed_line: Some(1),
            }),
            reach_version: ReachVersion(3),
        },
    );
    fixtures.answer("realm/needs_chief", &());
    for (name, answer) in [
        ("allow", Answer::Allow),
        (
            "refuse",
            Answer::Refuse {
                reason: Some("not on main".into()),
            },
        ),
        (
            "stopped",
            Answer::Lapsed {
                lapse: Lapse::Stopped,
            },
        ),
        (
            "ended",
            Answer::Lapsed {
                lapse: Lapse::Ended,
            },
        ),
    ] {
        fixtures.args(
            &format!("workspace/chief_answer.{name}"),
            &ChiefAnswer {
                agent: AGENT,
                call: CallId("call-1".into()),
                answer,
            },
        );
    }
    for (name, taken) in [
        ("runs", AnswerTaken::Runs),
        (
            "not_run",
            AnswerTaken::NotRun {
                why: CallRefusal::Lapsed {
                    lapse: Lapse::Stopped,
                },
            },
        ),
        ("no_wait", AnswerTaken::NoWait),
        (
            "refused",
            AnswerTaken::Refused {
                why: RealmRefusal::NotTheRealm,
            },
        ),
    ] {
        fixtures.answer(
            &format!("workspace/chief_answer.{name}"),
            &taken,
        );
    }
    fixtures.args("realm/secrets", &Secrets);
    fixtures.answer(
        "realm/secrets",
        &SecretList {
            version: SecretsVersion(2),
            values: vec!["made-up-secret".into()],
        },
    );
}

fn the_reads(fixtures: &mut Fixtures) {
    fixtures.args(
        "workspace/read_file",
        &ReadFile {
            agent: AGENT,
            path: "/work/project/notes.md".into(),
            offset: Some(10),
            limit: Some(20),
        },
    );
    fixtures.args(
        "workspace/search",
        &Search {
            agent: AGENT,
            under: "/work/project".into(),
            pattern: "fn main".into(),
            name: Some("*.rs".into()),
            ignore_case: true,
        },
    );
    fixtures.args(
        "workspace/find",
        &Find {
            agent: AGENT,
            under: "/work/project".into(),
            name: None,
        },
    );
    fixtures.answer(
        "workspace/read_file.text",
        &FileAnswer::Text(Text {
            text: "line 10\nline 11".into(),
            notes: vec![Note {
                severity: Severity::Info,
                code: "truncated".into(),
                message: "Showing lines 10-11 of 40. Use offset=12 \
                          to continue."
                    .into(),
            }],
            truncation: Some(Truncation {
                by: Some(CutBy::Lines),
                total_lines: 40,
                total_bytes: 300,
                output_lines: 2,
                output_bytes: 15,
                first_line_exceeds_limit: false,
            }),
        }),
    );
    fixtures.answer(
        "workspace/read_file.text.long_line",
        &FileAnswer::Text(Text {
            text: r#"{"rows":[[1,2,3],[4,5,6],[7,8,9]"#.into(),
            notes: vec![Note {
                severity: Severity::Warn,
                code: "truncated".into(),
                message: "Line 1 is 120.0KB, exceeds the 50.0KB \
                          limit; showing its first 50.0KB. Use \
                          bash: sed -n '1p' /work/project/rows.json \
                          | tail -c +51201"
                    .into(),
            }],
            truncation: Some(Truncation {
                by: Some(CutBy::Bytes),
                total_lines: 1,
                total_bytes: 122_880,
                output_lines: 1,
                output_bytes: 51_200,
                first_line_exceeds_limit: true,
            }),
        }),
    );
    fixtures.answer(
        "workspace/read_file.image",
        &FileAnswer::Image(Image {
            mime: "image/gif".into(),
            data: Base64(b"GIF89a".to_vec()),
        }),
    );
    fixtures.answer(
        "workspace/read_file.refused",
        &FileAnswer::Refused {
            why: CallRefusal::Reach {
                why: Refusal::AgentFilesystem {
                    path: "/realm/actors/agent/7.fs/notes.md".into(),
                },
            },
        },
    );
    fixtures.answer(
        "workspace/read_file.failed",
        &FileAnswer::Failed {
            words: "ENOENT: no such file or directory, open \
                    '/work/project/gone.md'"
                .into(),
        },
    );
}

fn the_changes(fixtures: &mut Fixtures) {
    let path = PathBuf::from("/work/project/notes.md");
    fixtures.answer(
        "workspace/write_file.changed",
        &ChangeAnswer::Changed(Change {
            path: path.clone(),
            created: true,
            hash: Sha256([7; 32]),
            diff: None,
        }),
    );
    fixtures.answer(
        "workspace/edit_file.changed",
        &ChangeAnswer::Changed(Change {
            path: path.clone(),
            created: false,
            hash: Sha256([8; 32]),
            diff: Some(EditDiff {
                diff: "- 1 hello\n+ 1 goodbye".into(),
                patch: "--- notes.md\n+++ notes.md\n@@ -1 +1 \
                        @@\n-hello\n+goodbye\n"
                    .into(),
                first_changed_line: Some(1),
            }),
        }),
    );
    fixtures.answer(
        "workspace/edit_file.refused.file_changed",
        &ChangeAnswer::Refused {
            why: CallRefusal::FileChanged {
                path: path.clone(),
                was: Hashed::Sha256 {
                    hash: Sha256([1; 32]),
                },
                now: Hashed::Absent,
            },
        },
    );
    fixtures.answer(
        "workspace/write_file.refused.reach",
        &ChangeAnswer::Refused {
            why: CallRefusal::Reach {
                why: Refusal::AgentFilesystem {
                    path: "/realm/actors/agent/7.fs/notes.md".into(),
                },
            },
        },
    );
    fixtures.answer(
        "workspace/write_file.refused.outside_reach",
        &ChangeAnswer::Refused {
            why: CallRefusal::Reach {
                why: Refusal::OutsideReach {
                    path: "/etc/hosts".into(),
                    access: Access::Change,
                },
            },
        },
    );
    fixtures.answer(
        "workspace/write_file.refused.project_in_realm",
        &ChangeAnswer::Refused {
            why: CallRefusal::Reach {
                why: Refusal::ProjectInRealm {
                    folder: "/realm/projects/site".into(),
                },
            },
        },
    );
    fixtures.answer(
        "workspace/write_file.waits_for_chief",
        &ChangeAnswer::WaitsForChief {
            needs: vec![Need::Edit],
        },
    );
    fixtures.answer(
        "workspace/write_file.refused.too_large",
        &ChangeAnswer::Refused {
            why: CallRefusal::TooLarge {
                bytes: 16 * 1024 * 1024 + 1,
                limit: 16 * 1024 * 1024,
            },
        },
    );
    fixtures.answer(
        "workspace/edit_file.failed",
        &ChangeAnswer::Failed {
            words: "Could not find the exact text in \
                    /work/project/notes.md. The old text must match \
                    exactly including all whitespace and newlines."
                .into(),
        },
    );
}

/// A refusal of a changed file says what the file held and holds.
#[test]
fn a_changed_file_is_told_by_its_hashes() {
    let refusal = CallRefusal::FileChanged {
        path: "/p/a.md".into(),
        was: Hashed::Sha256 {
            hash: Sha256([0xab; 32]),
        },
        now: Hashed::Absent,
    };
    assert_eq!(
        refusal.to_string(),
        format!(
            "/p/a.md changed after the change was asked for: it \
             held sha256 {} and now holds no file. Read it again \
             and ask again",
            "ab".repeat(32)
        )
    );
}
