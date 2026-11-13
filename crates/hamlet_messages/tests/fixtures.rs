//! The fixtures: each message's bytes as the host carries them. A
//! call's arguments are encoded as the authoring layer encodes them,
//! and an answer as it encodes an action's output. A message whose
//! bytes changed rewrites its fixture and fails once, so the change
//! is committed and the Node host's tests read it.

use std::fmt::Debug;
use std::path::{Path, PathBuf};

use hamlet_actor::action::{decode_positional, encode_positional};
use hamlet_messages::workspace::reach::{
    Footing, Mode, Project, Reach, SecretsVersion, Trust,
};
use hamlet_messages::workspace::{
    AgentNumber, CallId, ConversationId, EditFile, ReachAnswer,
    ReachRefusal, ReachVersion, RealmTask, Replace, Run, SetReach,
    Shell, WriteFile,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

const AGENT: AgentNumber = AgentNumber(7);

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// The fixtures checked so far, and the ones rewritten.
#[derive(Default)]
struct Fixtures {
    rewritten: Vec<String>,
}

impl Fixtures {
    /// Checks the arguments of a call.
    fn args<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let bytes = encode_positional(value).unwrap();
        self.check(&format!("{name}.args"), &bytes);
        let read: T = decode_positional(&bytes).unwrap();
        assert_eq!(&read, value, "{name} reads back");
    }

    /// Checks an answer.
    fn answer<T>(&mut self, name: &str, value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_value(value).unwrap();
        let mut bytes = Vec::new();
        ciborium::into_writer(&json, &mut bytes).unwrap();
        self.check(&format!("{name}.answer"), &bytes);
        let read: T =
            ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(&read, value, "{name} reads back");
    }

    fn check(&mut self, name: &str, bytes: &[u8]) {
        let path = folder().join(format!("{name}.cbor"));
        if std::fs::read(&path).ok().as_deref() == Some(bytes) {
            return;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        self.rewritten.push(name.to_owned());
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        if !self.rewritten.is_empty() && !std::thread::panicking() {
            panic!(
                "These messages' bytes changed, so their fixtures \
                 under crates/hamlet_messages/fixtures are \
                 rewritten; commit them: {}",
                self.rewritten.join(", ")
            );
        }
    }
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

#[test]
fn the_workspaces_messages_match_their_fixtures() {
    let mut fixtures = Fixtures::default();
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
    fixtures.answer(
        "workspace/set_reach.in_force",
        &ReachAnswer::InForce(ReachVersion(3)),
    );
    fixtures.answer(
        "workspace/set_reach.refused",
        &ReachAnswer::Refused(ReachRefusal::NotTheRealm),
    );
    let task = Some(RealmTask("task-1".into()));
    fixtures.args(
        "workspace/run",
        &Run {
            agent: AGENT,
            call: CallId("call-1".into()),
            conversation: ConversationId("c1".into()),
            realm_task: task.clone(),
            shell: Shell::Bash,
            script: "echo hi".into(),
            network: false,
            timeout_ms: Some(30_000),
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
