//! The fixtures: each message's bytes as the host carries them. A
//! call's arguments are encoded as the authoring layer encodes them,
//! and an answer as it encodes an action's output. A message whose
//! bytes changed rewrites its fixture and fails once, so the change
//! is committed with it.

mod cases;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use ciborium::Value;
use hamlet_actor::Action;
use hamlet_actor::action::{decode_positional, encode_positional};
use hamlet_actor_error::ActorError;
use hamlet_address::Caller;
use hamlet_messages::address::{Agent, Party, Realm, Workspace};
use hamlet_messages::agent::{Finished, Outcome};
use hamlet_messages::bytes::{Base64, Sha256};
use hamlet_messages::realm::{NeedsChief, SecretList, Secrets};
use hamlet_messages::sender::{id_reused, wrong_caller};
use hamlet_messages::workspace::files::{
    Change, ChangeAnswer, ChangeEnd, CutBy, EditDiff, EditFile, Find,
    Hashed, Image, Note, ReadAnswer, ReadFile, Replace, Search,
    SearchAnswer, Severity, Text, Truncation, WriteFile,
};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Need, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::refusal::{
    CallRefusal, ChangeNotRun, ChangeRefusal, FileGate,
    InvalidContent, InvalidPath, InvalidProgram, PathRefusal,
    ReachChanged, ReachRefusal, ReadRefusal, RunNotRun, RunRefusal,
    Stale, StatusRefusal, ToChange, ToRead, ToolRefusal, WaitEnd,
    WriteRefusal,
};
use hamlet_messages::workspace::{
    AddedTool, AgentNumber, Answer, AnswerTaken, CallId, Cancel,
    CancelAgent, Cancelled, ChiefAnswer, CommandEnd, CommandId,
    CommandStatus, ConversationId, Cut, Exit, Fault, InForce, Lapse,
    ListTools, Program, ReachVersion, RealmTask, Run, RunAnswer,
    RunEnd, SetReach, ToolList, Which,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use self::cases::{
    Cases, Found, InUnion, Union, family, named, tagged, union,
};

const AGENT: AgentNumber = AgentNumber(7);

fn enums() -> Vec<Cases> {
    vec![
        tagged::<Answer>(),
        tagged::<AnswerTaken>(),
        tagged::<Program>(),
        tagged::<RunAnswer>(),
        tagged::<RunEnd>(),
        tagged::<Exit>(),
        tagged::<Which>(),
        tagged::<Outcome>(),
        tagged::<Reach>(),
        tagged::<ReadAnswer>(),
        tagged::<SearchAnswer>(),
        tagged::<ChangeAnswer>(),
        tagged::<ChangeEnd>(),
        tagged::<Hashed>(),
        tagged::<CallRefusal>(),
        tagged::<InvalidProgram>(),
        tagged::<ToolRefusal>(),
        tagged::<InvalidPath>(),
        tagged::<PathRefusal<ToRead>>(),
        tagged::<ReachRefusal>(),
        tagged::<InvalidContent>(),
        tagged::<StatusRefusal>(),
        tagged::<WaitEnd>(),
        tagged::<ReachChanged<ReachRefusal>>(),
        tagged::<Stale>(),
        named::<Lapse>(),
        named::<Need>(),
        named::<Mode>(),
        named::<ToRead>(),
        named::<ToChange>(),
        named::<Severity>(),
        named::<CutBy>(),
    ]
    .into_iter()
    .chain(realm::enums())
    .collect()
}

macro_rules! union {
    ($union:ident $(<$param:ty>)? {
        $($case:ident: $family:ty),+ $(,)?
    }) => {{
        let _families_match_variants =
            |union: $union $(<$param>)?| match union {
            $($union::$case(family) => {
                let _: $family = family;
            })+
        };
        union::<$union $(<$param>)?>(vec![$(family::<$family>()),+])
    }};
}

// After `union!`, which it uses.
mod realm;

fn unions() -> Vec<Union> {
    vec![
        union!(RunRefusal {
            Call: CallRefusal,
            Program: InvalidProgram,
            Tool: ToolRefusal,
            Reach: ReachRefusal,
        }),
        union!(ReadRefusal {
            Invalid: InvalidPath,
            Path: PathRefusal<ToRead>,
            Reach: ReachRefusal,
        }),
        union!(ChangeRefusal {
            Call: CallRefusal,
            Invalid: InvalidPath,
            Path: PathRefusal<ToChange>,
            Reach: ReachRefusal,
        }),
        union!(WriteRefusal {
            Call: CallRefusal,
            Invalid: InvalidPath,
            Path: PathRefusal<ToChange>,
            Reach: ReachRefusal,
            Content: InvalidContent,
        }),
        union!(FileGate {
            Path: PathRefusal<ToChange>,
            Reach: ReachRefusal,
        }),
        union!(RunNotRun {
            Wait: WaitEnd,
            Reach: ReachChanged<ReachRefusal>,
            Tool: ToolRefusal,
        }),
        union!(ChangeNotRun {
            Wait: WaitEnd,
            Reach: ReachChanged<FileGate>,
            Stale: Stale,
        }),
    ]
    .into_iter()
    .chain(realm::unions())
    .collect()
}

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// An enum a fixture's root is: its cases and those found.
struct Root {
    cases: Cases,
    found: BTreeSet<String>,
}

/// The cases the probed values hold.
#[derive(Default)]
struct Coverage {
    found: BTreeMap<Cases, BTreeSet<String>>,
    in_unions: BTreeSet<InUnion>,
    roots: BTreeMap<String, Root>,
}

impl Coverage {
    fn probe<T: DeserializeOwned>(
        &mut self,
        name: &str,
        value: &Value,
        unions: &[Union],
        problems: &mut Vec<String>,
    ) {
        let mut found = Found::default();
        cases::found::<T>(name, value, unions, &mut found);
        problems.extend(found.problems);
        self.in_unions.extend(found.in_unions);
        if let (Some(cases), Some(tag)) =
            (cases::tagged_cases::<T>(), cases::tag(value))
        {
            self.roots
                .entry(cases::short_name::<T>())
                .or_insert(Root {
                    cases,
                    found: BTreeSet::new(),
                })
                .found
                .insert(tag.to_owned());
        }
        for (cases, case) in found.cases {
            self.found.entry(cases).or_default().insert(case);
        }
    }

    /// The cases of `enums`, of `unions` and of each root that no
    /// probed value holds.
    fn missing<'a>(
        &self,
        what: &str,
        enums: impl IntoIterator<Item = &'a Cases>,
        unions: impl IntoIterator<Item = &'a Union>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        for cases in enums {
            let found = self.found.get(cases);
            for case in *cases {
                if !found.is_some_and(|found| found.contains(*case)) {
                    problems.push(format!(
                        "no {what} holds {case} of {cases:?}"
                    ));
                }
            }
        }
        for (name, root) in &self.roots {
            for case in root.cases {
                if !root.found.contains(*case) {
                    problems.push(format!(
                        "no {what}'s root is {case} of {name}"
                    ));
                }
            }
        }
        for union in unions {
            for family in &union.families {
                for case in family.cases {
                    let held = InUnion {
                        union: union.name.clone(),
                        family: family.name.clone(),
                        case: (*case).to_owned(),
                    };
                    if !self.in_unions.contains(&held) {
                        problems.push(format!(
                            "no {what} holds {case} of {} in the \
                             union {}",
                            family.name, union.name
                        ));
                    }
                }
            }
        }
        problems
    }
}

struct Fixtures {
    unions: Vec<Union>,
    written: BTreeSet<String>,
    rewritten: Vec<String>,
    problems: Vec<String>,
    messages: Coverage,
    stored: Coverage,
}

impl Fixtures {
    fn new() -> Self {
        Self {
            unions: unions(),
            written: BTreeSet::new(),
            rewritten: Vec::new(),
            problems: Vec::new(),
            messages: Coverage::default(),
            stored: Coverage::default(),
        }
    }

    /// Checks the arguments of a call.
    fn args<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let (name, bytes) = self.unprobed_args(name, value);
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

    /// Checks the arguments of a call, but not the cases they hold.
    fn unprobed_args<T>(
        &mut self,
        name: &str,
        value: &T,
    ) -> (String, Vec<u8>)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let bytes = encode_positional(value).unwrap();
        let name = format!("{name}.args");
        self.check(&name, &bytes);
        self.read_back(&name, value, decode_positional::<T>(&bytes));
        (name, bytes)
    }

    fn answer<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_value(value).unwrap();
        let mut bytes = Vec::new();
        ciborium::into_writer(&json, &mut bytes).unwrap();
        self.decoded(&format!("{name}.answer"), value, &bytes);
    }

    /// Checks the input an actor is created with.
    fn input<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let mut bytes = Vec::new();
        ciborium::into_writer(value, &mut bytes).unwrap();
        self.decoded(&format!("{name}.input"), value, &bytes);
    }

    fn decoded<T>(&mut self, name: &str, value: &T, bytes: &[u8])
    where
        T: DeserializeOwned + PartialEq + Debug,
    {
        self.check(name, bytes);
        self.read_back(
            name,
            value,
            ciborium::from_reader::<T, _>(bytes),
        );
        self.probe::<T>(
            name,
            &ciborium::from_reader::<Value, _>(bytes).unwrap(),
        );
    }

    fn stored<T>(&mut self, name: &str, value: &T) -> Vec<u8>
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let name = format!("stored/{name}");
        self.written.insert(format!("{name}.cbor"));
        let mut bytes = Vec::new();
        ciborium::into_writer(value, &mut bytes).unwrap();
        let path = folder().join(format!("{name}.cbor"));
        let Ok(kept) = std::fs::read(&path) else {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            self.rewritten.push(format!("{name} (new)"));
            return bytes;
        };
        self.read_back(
            &name,
            value,
            ciborium::from_reader::<T, _>(kept.as_slice()),
        );
        self.stored.probe::<T>(
            &name,
            &ciborium::from_reader::<Value, _>(kept.as_slice())
                .unwrap(),
            &self.unions,
            &mut self.problems,
        );
        kept
    }

    fn stored_request<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let kept = self.stored(name, value);
        let mut bytes = Vec::new();
        ciborium::into_writer(value, &mut bytes).unwrap();
        if kept != bytes {
            self.problems.push(format!(
                "stored/{name}'s bytes changed, so a kept call's \
                 repeat would not match"
            ));
        }
    }

    fn read_back<T: PartialEq + Debug, E: Debug>(
        &mut self,
        name: &str,
        value: &T,
        read: Result<T, E>,
    ) {
        match read {
            Ok(read) if read == *value => {}
            Ok(read) => self.problems.push(format!(
                "{name} reads back as {read:?}, not {value:?}"
            )),
            Err(error) => self.problems.push(format!(
                "{name} does not read back: {error:?}"
            )),
        }
    }

    fn probe<T: DeserializeOwned>(
        &mut self,
        name: &str,
        value: &Value,
    ) {
        if *value != Value::Null && holds_null(value) {
            self.problems
                .push(format!("{name} writes a field as null"));
        }
        self.messages.probe::<T>(
            name,
            value,
            &self.unions,
            &mut self.problems,
        );
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

    fn remove_unwritten(&mut self) {
        for file in files(&folder()) {
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

    fn case_problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let enums = enums();
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
        }
        problems.extend(self.messages.missing(
            "fixture",
            &enums,
            &self.unions,
        ));
        let stored_unions = self.unions.iter().filter(|union| {
            self.stored
                .in_unions
                .iter()
                .any(|held| held.union == union.name)
        });
        problems.extend(self.stored.missing(
            "stored fixture",
            self.stored.found.keys(),
            stored_unions,
        ));
        for cases in
            self.messages.found.keys().chain(self.stored.found.keys())
        {
            if !enums.contains(cases) {
                problems.push(format!(
                    "the enum of {cases:?} is missing from enums()"
                ));
            }
        }
        for union in &self.unions {
            for (at, first) in union.families.iter().enumerate() {
                for second in &union.families[at + 1..] {
                    for tag in first
                        .cases
                        .iter()
                        .filter(|tag| second.cases.contains(tag))
                    {
                        problems.push(format!(
                            "{} and {} of the union {} share the \
                             type {tag}",
                            first.name, second.name, union.name
                        ));
                    }
                }
            }
        }
        problems
    }

    fn assert_unchanged_and_covered(self) {
        let mut problems = self.case_problems();
        problems.extend(self.problems);
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

fn files(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .flat_map(|path| {
            if path.is_dir() {
                files(&path)
            } else {
                vec![path]
            }
        })
        .collect()
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
    let mut fixtures = Fixtures::new();
    the_workspaces_messages(&mut fixtures);
    the_chiefs_answers(&mut fixtures);
    the_answers_about_a_command(&mut fixtures);
    the_reads(&mut fixtures);
    the_changes(&mut fixtures);
    the_realms_and_the_agents(&mut fixtures);
    the_addresses(&mut fixtures);
    the_errors(&mut fixtures);
    the_stored_rows(&mut fixtures);
    realm::the_realms_messages(&mut fixtures);
    fixtures.remove_unwritten();
    fixtures.assert_unchanged_and_covered();
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

fn call(id: &str) -> CallId {
    CallId(id.into())
}

fn the_workspaces_messages(fixtures: &mut Fixtures) {
    fixtures.args(
        "workspace/set_reach.agent",
        &set_reach(Reach::Agent(footing())),
    );
    for (name, starter) in
        [("worker", Some(AgentNumber(3))), ("chiefs_worker", None)]
    {
        fixtures.args(
            &format!("workspace/set_reach.{name}"),
            &set_reach(Reach::Worker {
                starter,
                footing: Footing {
                    project: None,
                    ..footing()
                },
            }),
        );
    }
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
        &InForce {
            version: ReachVersion(3),
        },
    );
    fixtures.args("workspace/run", &asked_run());
    fixtures.args(
        "workspace/run.unset",
        &Run {
            call: call("call-1"),
            conversation: ConversationId("c1".into()),
            realm_task: None,
            program: Program::Script {
                script: "echo hi".into(),
                network: true,
                timeout_ms: None,
            },
        },
    );
    fixtures.args(
        "workspace/run.tool",
        &Run {
            call: call("call-1"),
            conversation: ConversationId("c1".into()),
            realm_task: task(),
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
            faults: vec![Fault {
                folder: "/realm/tools/broken".into(),
                words: "manifest.json is not valid JSON".into(),
            }],
        },
    );
    fixtures.args("workspace/write_file", &asked_write());
    fixtures.args("workspace/edit_file", &asked_edit());
}

fn task() -> Option<RealmTask> {
    Some(RealmTask("task-1".into()))
}

fn asked_run() -> Run {
    Run {
        call: call("call-1"),
        conversation: ConversationId("c1".into()),
        realm_task: task(),
        program: Program::Script {
            script: "echo hi".into(),
            network: false,
            timeout_ms: Some(30_000),
        },
    }
}

fn asked_write() -> WriteFile {
    WriteFile {
        call: call("call-2"),
        conversation: ConversationId("c1".into()),
        realm_task: None,
        path: "/work/project/notes.md".into(),
        content: "hello\n".into(),
    }
}

fn asked_edit() -> EditFile {
    EditFile {
        call: call("call-3"),
        conversation: ConversationId("c1".into()),
        realm_task: task(),
        path: "/work/project/notes.md".into(),
        edits: vec![Replace {
            old: "hello".into(),
            new: "goodbye".into(),
        }],
    }
}

fn the_stored_rows(fixtures: &mut Fixtures) {
    fixtures.stored("reach.agent", &Reach::Agent(footing()));
    fixtures.stored(
        "reach.worker",
        &Reach::Worker {
            starter: None,
            footing: Footing {
                project: None,
                ..footing()
            },
        },
    );
    fixtures.stored("reach.ended", &Reach::Ended);
    fixtures.stored_request("request.run", &asked_run());
    fixtures.stored_request("request.write_file", &asked_write());
    fixtures.stored_request("request.edit_file", &asked_edit());
    fixtures.stored("needs", &vec![Need::Network, Need::Command]);
    fixtures.stored("hashed.absent", &Hashed::Absent);
    fixtures.stored(
        "hashed.sha256",
        &Hashed::Sha256 {
            hash: Sha256([1; 32]),
        },
    );
    fixtures
        .stored("end.command", &command_end(Exit::Code { code: 0 }));
    fixtures.stored(
        "end.run_not_run",
        &RunNotRun::from(WaitEnd::Lapsed {
            lapse: Lapse::Ended,
        }),
    );
    fixtures.stored(
        "end.change.changed",
        &ChangeEnd::Changed(changed(true, 7, None)),
    );
    fixtures.stored(
        "end.change.not_run",
        &ChangeEnd::NotRun {
            why: file_changed().into(),
        },
    );
    fixtures.stored(
        "end.change_not_run",
        &ChangeNotRun::from(WaitEnd::ChiefRefused { reason: None }),
    );
    the_stored_cases(fixtures);
}

/// A kept row of each case the rows above leave out, and of each
/// optional field set.
fn the_stored_cases(fixtures: &mut Fixtures) {
    fixtures.stored(
        "reach.worker.started",
        &Reach::Worker {
            starter: Some(AGENT),
            footing: footing(),
        },
    );
    fixtures.stored(
        "reach.agent.auto",
        &Reach::Agent(Footing {
            project: Some(Project {
                folder: "/work/project".into(),
                mode: Mode::Auto,
            }),
            ..footing()
        }),
    );
    fixtures.stored_request(
        "request.run.tool",
        &Run {
            program: Program::Tool {
                name: "word_count".into(),
                arguments: r#"{"path":"/project/a.md"}"#.into(),
            },
            ..asked_run()
        },
    );
    fixtures.stored(
        "needs.edit",
        &vec![Need::Network, Need::Command, Need::Edit],
    );
    fixtures.stored(
        "end.command.cut",
        &CommandEnd {
            cut: Some(Cut {
                total_bytes: 52_428_800,
                total_lines: 1_048_576,
                full_output: "/realm/actors/workspace/local.output/\
                              7/4.log"
                    .into(),
            }),
            ..command_end(Exit::Code { code: 0 })
        },
    );
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
        fixtures.stored(
            &format!("end.command.{name}"),
            &command_end(exit),
        );
    }
    fixtures.stored(
        "end.change.changed.diff",
        &ChangeEnd::Changed(changed(
            false,
            8,
            Some(EditDiff {
                diff: "- 1 hello\n+ 1 goodbye".into(),
                patch: "--- notes.md\n+++ notes.md\n@@ -1 +1 \
                        @@\n-hello\n+goodbye\n"
                    .into(),
                first_changed_line: Some(1),
            }),
        )),
    );
    fixtures.stored(
        "end.change.failed",
        &ChangeEnd::Failed {
            words: "notes.md: permission denied".into(),
        },
    );
    let mut ends = wait_ends().to_vec();
    ends.push((
        "lapsed.stopped",
        WaitEnd::Lapsed {
            lapse: Lapse::Stopped,
        },
    ));
    for (name, end) in ends {
        fixtures.stored(
            &format!("end.run_not_run.{name}"),
            &RunNotRun::from(end.clone()),
        );
        fixtures.stored(
            &format!("end.change_not_run.{name}"),
            &ChangeNotRun::from(end),
        );
    }
    fixtures.stored(
        "end.run_not_run.reach_needs",
        &RunNotRun::from(needs_more::<ReachRefusal>()),
    );
    fixtures.stored(
        "end.change_not_run.reach_needs",
        &ChangeNotRun::from(needs_more::<FileGate>()),
    );
    for (name, why) in [
        ("no_reach", ReachRefusal::NoReach),
        ("project_in_realm", project_in_realm()),
    ] {
        fixtures.stored(
            &format!("end.run_not_run.reach_refuses.{name}"),
            &RunNotRun::from(refuses(why.clone())),
        );
        fixtures.stored(
            &format!("end.change_not_run.reach_refuses.{name}"),
            &ChangeNotRun::from(refuses(FileGate::from(why))),
        );
    }
    for (name, why) in [
        ("outside_reach", outside_reach_to_change()),
        ("agent_filesystem", agent_filesystem()),
    ] {
        fixtures.stored(
            &format!("end.change_not_run.reach_refuses.{name}"),
            &ChangeNotRun::from(refuses(FileGate::from(why))),
        );
    }
    for (name, why) in [
        ("tool_not_found", tool_not_found()),
        ("tool_arguments", tool_arguments()),
    ] {
        fixtures.stored(
            &format!("end.run_not_run.{name}"),
            &RunNotRun::from(why),
        );
    }
}

fn wait_ends() -> [(&'static str, WaitEnd); 3] {
    [
        (
            "chief_refused",
            WaitEnd::ChiefRefused {
                reason: Some("not on main".into()),
            },
        ),
        (
            "chief_refused.no_reason",
            WaitEnd::ChiefRefused { reason: None },
        ),
        (
            "lapsed",
            WaitEnd::Lapsed {
                lapse: Lapse::Rewound,
            },
        ),
    ]
}

fn needs_more<G>() -> ReachChanged<G> {
    ReachChanged::ReachNeeds {
        since: ReachVersion(3),
        now: ReachVersion(4),
        needs: vec![Need::Network, Need::Edit],
    }
}

fn refuses<G>(why: G) -> ReachChanged<G> {
    ReachChanged::ReachRefuses {
        since: ReachVersion(3),
        now: ReachVersion(5),
        why,
    }
}

fn file_changed() -> Stale {
    Stale::FileChanged {
        path: "/work/project/notes.md".into(),
        was: Hashed::Sha256 {
            hash: Sha256([1; 32]),
        },
        now: Hashed::Absent,
    }
}

fn outside_reach_to_change() -> PathRefusal<ToChange> {
    PathRefusal::OutsideReach {
        path: "/etc/hosts".into(),
        access: ToChange::Change,
    }
}

fn agent_filesystem<A>() -> PathRefusal<A> {
    PathRefusal::AgentFilesystem {
        path: "/realm/actors/agent/7.fs/notes.md".into(),
    }
}

fn project_in_realm() -> ReachRefusal {
    ReachRefusal::ProjectInRealm {
        folder: "/realm/projects/site".into(),
    }
}

fn not_absolute() -> InvalidPath {
    InvalidPath::NotAbsolute {
        path: "notes.md".into(),
    }
}

fn tool_not_found() -> ToolRefusal {
    ToolRefusal::NotFound {
        name: "word_count".into(),
    }
}

fn tool_arguments() -> ToolRefusal {
    ToolRefusal::Arguments {
        name: "word_count".into(),
    }
}

fn the_chiefs_answers(fixtures: &mut Fixtures) {
    for (name, answer) in [
        ("allow", Answer::Allow),
        (
            "refuse",
            Answer::Refuse {
                reason: Some("not on main".into()),
            },
        ),
        ("refuse.no_reason", Answer::Refuse { reason: None }),
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
                call: call("call-1"),
                answer,
            },
        );
    }
    let mut answers = vec![
        ("taken".to_owned(), AnswerTaken::Taken),
        ("no_wait".to_owned(), AnswerTaken::NoWait),
    ];
    let run_not_run = wait_ends()
        .into_iter()
        .map(|(name, why)| (name, RunNotRun::from(why)))
        .chain([
            ("reach_refuses", refuses(ReachRefusal::NoReach).into()),
            ("reach_needs", needs_more().into()),
            ("tool_not_found", tool_not_found().into()),
        ]);
    answers.extend(run_not_run.map(|(name, why)| {
        (
            format!("run_not_run.{name}"),
            AnswerTaken::RunNotRun { why },
        )
    }));
    let change_not_run = [
        (
            "lapsed",
            ChangeNotRun::from(WaitEnd::Lapsed {
                lapse: Lapse::Stopped,
            }),
        ),
        (
            "reach_refuses",
            refuses(FileGate::from(outside_reach_to_change())).into(),
        ),
        ("reach_needs", needs_more().into()),
        ("file_changed", file_changed().into()),
    ];
    answers.extend(change_not_run.into_iter().map(|(name, why)| {
        (
            format!("change_not_run.{name}"),
            AnswerTaken::ChangeNotRun { why },
        )
    }));
    for (name, taken) in answers {
        fixtures.answer(
            &format!("workspace/chief_answer.{name}"),
            &taken,
        );
    }
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

fn ran(end: CommandEnd) -> RunAnswer {
    RunAnswer::Ended {
        end: RunEnd::Ran(end),
    }
}

fn the_answers_about_a_command(fixtures: &mut Fixtures) {
    fixtures.answer(
        "workspace/run.ended.ran",
        &ran(command_end(Exit::Code { code: 0 })),
    );
    fixtures.answer(
        "workspace/run.ended.ran.cut",
        &ran(CommandEnd {
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
            &format!("workspace/run.ended.ran.{name}"),
            &ran(command_end(exit)),
        );
    }
    let not_run = wait_ends()
        .into_iter()
        .map(|(name, why)| (name, RunNotRun::from(why)))
        .chain([
            ("reach_refuses", refuses(project_in_realm()).into()),
            ("reach_needs", needs_more().into()),
            ("tool_not_found", tool_not_found().into()),
            ("tool_arguments", tool_arguments().into()),
        ]);
    for (name, why) in not_run {
        fixtures.answer(
            &format!("workspace/run.ended.not_run.{name}"),
            &RunAnswer::<RunRefusal>::Ended {
                end: RunEnd::NotRun { why },
            },
        );
    }
    fixtures.answer(
        "workspace/run.running",
        &RunAnswer::<RunRefusal>::Running {
            command: CommandId(4),
        },
    );
    fixtures.answer(
        "workspace/run.waits_for_chief",
        &RunAnswer::<RunRefusal>::WaitsForChief {
            needs: vec![Need::Network, Need::Command],
        },
    );
    for (name, why) in [
        (
            "invalid_call_id",
            RunRefusal::from(CallRefusal::InvalidId {
                call: call("call 1"),
            }),
        ),
        ("holds_secret", CallRefusal::HoldsSecret.into()),
        ("script_too_long", InvalidProgram::ScriptTooLong.into()),
        ("zero_timeout", InvalidProgram::ZeroTimeout.into()),
        (
            "arguments_too_long",
            InvalidProgram::ArgumentsTooLong.into(),
        ),
        ("no_reach", ReachRefusal::NoReach.into()),
        ("project_in_realm", project_in_realm().into()),
        ("tool_not_found", tool_not_found().into()),
        ("tool_arguments", tool_arguments().into()),
    ] {
        fixtures.answer(
            &format!("workspace/run.refused.{name}"),
            &RunAnswer::Refused { why },
        );
    }
    fixtures.args(
        "workspace/command_status",
        &CommandStatus {
            command: CommandId(4),
        },
    );
    for (name, answer) in [
        (
            "refused.no_such_command",
            RunAnswer::Refused {
                why: StatusRefusal::NoSuchCommand {
                    command: CommandId(9),
                },
            },
        ),
        (
            "ended.ran",
            RunAnswer::Ended {
                end: RunEnd::Ran(command_end(Exit::Code { code: 0 })),
            },
        ),
        (
            "running",
            RunAnswer::Running {
                command: CommandId(4),
            },
        ),
        (
            "waits_for_chief",
            RunAnswer::WaitsForChief {
                needs: vec![Need::Command],
            },
        ),
    ] {
        fixtures.answer(
            &format!("workspace/command_status.{name}"),
            &answer,
        );
    }
    for (name, which) in [
        (
            "one",
            Which::One {
                call: call("call-1"),
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
            &Cancel { which },
        );
    }
    fixtures.args(
        "workspace/cancel_agent.conversation",
        &CancelAgent {
            agent: AGENT,
            which: Which::Conversation {
                conversation: ConversationId("c1".into()),
            },
        },
    );
    fixtures.answer(
        "workspace/cancel.cancelled",
        &Cancelled {
            commands: vec![CommandId(4), CommandId(5)],
        },
    );
}

fn the_reads(fixtures: &mut Fixtures) {
    fixtures.args(
        "workspace/read_file",
        &ReadFile {
            path: "/work/project/notes.md".into(),
            offset: Some(10),
            limit: Some(20),
        },
    );
    fixtures.args(
        "workspace/read_file.whole",
        &ReadFile {
            path: "/work/project/notes.md".into(),
            offset: None,
            limit: None,
        },
    );
    fixtures.args(
        "workspace/search",
        &Search {
            under: "/work/project".into(),
            pattern: "fn main".into(),
            name: Some("*.rs".into()),
            ignore_case: true,
        },
    );
    fixtures.args(
        "workspace/find",
        &Find {
            under: "/work/project".into(),
            name: None,
        },
    );
    fixtures.answer(
        "workspace/read_file.text",
        &ReadAnswer::Text(Text {
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
        "workspace/read_file.text.whole",
        &ReadAnswer::Text(Text {
            text: "hello\n".into(),
            notes: Vec::new(),
            truncation: None,
        }),
    );
    fixtures.answer(
        "workspace/read_file.text.long_line",
        &ReadAnswer::Text(Text {
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
        &ReadAnswer::Image(Image {
            mime: "image/gif".into(),
            data: Base64(b"GIF89a".to_vec()),
        }),
    );
    fixtures.answer(
        "workspace/read_file.failed",
        &ReadAnswer::Failed {
            words: "ENOENT: no such file or directory, open \
                    '/work/project/gone.md'"
                .into(),
        },
    );
    for (name, why) in [
        ("not_absolute", ReadRefusal::from(not_absolute())),
        (
            "outside_reach",
            PathRefusal::OutsideReach {
                path: "/home/chief/.ssh/id_ed25519".into(),
                access: ToRead::Read,
            }
            .into(),
        ),
        ("agent_filesystem", agent_filesystem::<ToRead>().into()),
        ("no_reach", ReachRefusal::NoReach.into()),
        ("project_in_realm", project_in_realm().into()),
    ] {
        fixtures.answer(
            &format!("workspace/read_file.refused.{name}"),
            &ReadAnswer::Refused { why },
        );
    }
    fixtures.answer(
        "workspace/search.text",
        &SearchAnswer::Text(Text {
            text: "/work/project/src/main.rs:1: fn main() {".into(),
            notes: Vec::new(),
            truncation: None,
        }),
    );
    fixtures.answer(
        "workspace/search.failed",
        &SearchAnswer::Failed {
            words: "regex parse error".into(),
        },
    );
    fixtures.answer(
        "workspace/find.refused.outside_reach",
        &SearchAnswer::Refused {
            why: PathRefusal::OutsideReach {
                path: "/home/chief".into(),
                access: ToRead::Read,
            }
            .into(),
        },
    );
}

fn changed(
    created: bool,
    hash: u8,
    diff: Option<EditDiff>,
) -> Change {
    Change {
        path: "/work/project/notes.md".into(),
        created,
        hash: Sha256([hash; 32]),
        diff,
    }
}

fn the_changes(fixtures: &mut Fixtures) {
    fixtures.answer(
        "workspace/write_file.ended.changed",
        &ChangeAnswer::<WriteRefusal>::Ended {
            end: ChangeEnd::Changed(changed(true, 7, None)),
        },
    );
    let not_run = wait_ends()
        .into_iter()
        .map(|(name, why)| (name, ChangeNotRun::from(why)))
        .chain([
            (
                "reach_refuses",
                refuses(FileGate::from(outside_reach_to_change()))
                    .into(),
            ),
            (
                "reach_refuses.no_reach",
                refuses(FileGate::from(ReachRefusal::NoReach)).into(),
            ),
            (
                "reach_refuses.agent_filesystem",
                refuses(FileGate::from(
                    agent_filesystem::<ToChange>(),
                ))
                .into(),
            ),
            (
                "reach_refuses.project_in_realm",
                refuses(FileGate::from(project_in_realm())).into(),
            ),
            ("reach_needs", needs_more().into()),
        ]);
    for (name, why) in not_run {
        fixtures.answer(
            &format!("workspace/write_file.ended.not_run.{name}"),
            &ChangeAnswer::<WriteRefusal>::Ended {
                end: ChangeEnd::NotRun { why },
            },
        );
    }
    fixtures.answer(
        "workspace/write_file.waits_for_chief",
        &ChangeAnswer::<WriteRefusal>::WaitsForChief {
            needs: vec![Need::Edit],
        },
    );
    for (name, why) in [
        (
            "too_large",
            WriteRefusal::from(InvalidContent::TooLarge {
                bytes: 16 * 1024 * 1024 + 1,
                limit: 16 * 1024 * 1024,
            }),
        ),
        ("outside_reach", outside_reach_to_change().into()),
        ("agent_filesystem", agent_filesystem::<ToChange>().into()),
        ("project_in_realm", project_in_realm().into()),
        (
            "invalid_call_id",
            CallRefusal::InvalidId { call: call("") }.into(),
        ),
        ("holds_secret", CallRefusal::HoldsSecret.into()),
        ("not_absolute", not_absolute().into()),
        ("no_reach", ReachRefusal::NoReach.into()),
    ] {
        fixtures.answer(
            &format!("workspace/write_file.refused.{name}"),
            &ChangeAnswer::Refused { why },
        );
    }
    fixtures.answer(
        "workspace/edit_file.ended.changed",
        &ChangeAnswer::<ChangeRefusal>::Ended {
            end: ChangeEnd::Changed(changed(
                false,
                8,
                Some(EditDiff {
                    diff: "- 1 hello\n+ 1 goodbye".into(),
                    patch: "--- notes.md\n+++ notes.md\n@@ -1 +1 \
                            @@\n-hello\n+goodbye\n"
                        .into(),
                    first_changed_line: Some(1),
                }),
            )),
        },
    );
    fixtures.answer(
        "workspace/edit_file.ended.failed",
        &ChangeAnswer::<ChangeRefusal>::Ended {
            end: ChangeEnd::Failed {
                words: "Could not find the exact text in \
                        /work/project/notes.md. The old text must \
                        match exactly including all whitespace and \
                        newlines."
                    .into(),
            },
        },
    );
    fixtures.answer(
        "workspace/edit_file.waits_for_chief",
        &ChangeAnswer::<ChangeRefusal>::WaitsForChief {
            needs: vec![Need::Edit],
        },
    );
    fixtures.answer(
        "workspace/edit_file.ended.not_run.file_changed",
        &ChangeAnswer::<ChangeRefusal>::Ended {
            end: ChangeEnd::NotRun {
                why: file_changed().into(),
            },
        },
    );
    for (name, why) in [
        (
            "invalid_call_id",
            ChangeRefusal::from(CallRefusal::InvalidId {
                call: call(""),
            }),
        ),
        ("holds_secret", CallRefusal::HoldsSecret.into()),
        ("not_absolute", not_absolute().into()),
        ("no_reach", ReachRefusal::NoReach.into()),
        ("outside_reach", outside_reach_to_change().into()),
        ("agent_filesystem", agent_filesystem::<ToChange>().into()),
        ("project_in_realm", project_in_realm().into()),
    ] {
        fixtures.answer(
            &format!("workspace/edit_file.refused.{name}"),
            &ChangeAnswer::Refused { why },
        );
    }
}

fn the_realms_and_the_agents(fixtures: &mut Fixtures) {
    fixtures.args(
        "realm/needs_chief",
        &NeedsChief {
            agent: AGENT,
            call: call("call-1"),
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
    fixtures.args(
        "realm/needs_chief.command",
        &NeedsChief {
            agent: AGENT,
            call: call("call-1"),
            conversation: ConversationId("conversation-1".into()),
            realm_task: None,
            needs: vec![Need::Network, Need::Command],
            subject: "curl https://example.com".into(),
            change: None,
            reach_version: ReachVersion(3),
        },
    );
    fixtures.answer("realm/needs_chief", &());
    fixtures.args("realm/secrets", &Secrets);
    fixtures.answer(
        "realm/secrets",
        &SecretList {
            version: SecretsVersion(2),
            values: vec!["made-up-secret".into()],
        },
    );
    for (name, id, outcome) in [
        (
            "run.ran",
            "call-1",
            Outcome::Run {
                end: RunEnd::Ran(command_end(Exit::Code { code: 1 })),
            },
        ),
        (
            "run.not_run",
            "call-1",
            Outcome::Run {
                end: RunEnd::NotRun {
                    why: WaitEnd::Lapsed {
                        lapse: Lapse::Ended,
                    }
                    .into(),
                },
            },
        ),
        (
            "change.changed",
            "call-2",
            Outcome::Change {
                end: ChangeEnd::Changed(changed(true, 7, None)),
            },
        ),
        (
            "change.failed",
            "call-2",
            Outcome::Change {
                end: ChangeEnd::Failed {
                    words: "EACCES: permission denied".into(),
                },
            },
        ),
        (
            "change.not_run",
            "call-3",
            Outcome::Change {
                end: ChangeEnd::NotRun {
                    why: file_changed().into(),
                },
            },
        ),
    ] {
        fixtures.args(
            &format!("agent/finished.{name}"),
            &Finished {
                call: call(id),
                outcome,
            },
        );
    }
    fixtures.answer("agent/finished", &());
}

fn the_addresses(fixtures: &mut Fixtures) {
    fixtures
        .answer("address/workspace", &Workspace.address().unwrap());
    fixtures.answer("address/realm", &Realm.address().unwrap());
    fixtures
        .answer("address/agent", &Agent(AGENT).address().unwrap());
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Seen {
    group: String,
    code: String,
    message: String,
    metadata: serde_json::Value,
}

impl Seen {
    fn of(error: &anyhow::Error) -> Self {
        let error = ActorError::extract(error);
        Self {
            group: error.group().to_owned(),
            code: error.code().to_owned(),
            message: error.message().to_owned(),
            metadata: error.metadata().unwrap(),
        }
    }
}

fn the_errors(fixtures: &mut Fixtures) {
    let realm = Caller::Actor(Realm.address().unwrap());
    for (name, action, caller) in [
        ("run.realm", Run::NAME, Some(realm)),
        ("set_reach.client", SetReach::NAME, Some(Caller::Client)),
        ("set_reach.program", SetReach::NAME, None),
    ] {
        fixtures.answer(
            &format!("error/wrong_caller.{name}"),
            &Seen::of(&wrong_caller(action, caller)),
        );
    }
    fixtures.answer(
        "error/id_reused.run",
        &Seen::of(&id_reused(Run::NAME, &call("call-1"))),
    );
}

/// A refusal of a changed file says what the file held and holds.
#[test]
fn a_changed_file_is_told_by_its_hashes() {
    let refusal = Stale::FileChanged {
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

/// Each integer TypeScript writes reads up to 2^32 - 1 and fails
/// from 2^32.
#[test]
fn an_integer_typescript_writes_is_a_u32() {
    fn reads<T: DeserializeOwned>(args: serde_json::Value) -> bool {
        let args = encode_positional(&args).unwrap();
        decode_positional::<T>(&args).is_ok()
    }
    let read = |number| serde_json::json!({"path": "/a", "offset": number, "limit": number});
    let script = |number| {
        serde_json::json!({
            "call": "c", "conversation": "c",
            "program": {"type": "script", "script": "true",
                        "network": false, "timeout_ms": number}
        })
    };
    let status = |number| serde_json::json!({"command": number});
    let cancel = |number| serde_json::json!({"agent": number, "which": {"type": "all"}});
    for (number, fits) in
        [(u64::from(u32::MAX), true), (1 << 32, false)]
    {
        assert_eq!(
            [
                reads::<ReadFile>(read(number)),
                reads::<Run>(script(number)),
                reads::<CommandStatus>(status(number)),
                reads::<CancelAgent>(cancel(number)),
            ],
            [fits; 4],
            "{number}"
        );
    }
}
