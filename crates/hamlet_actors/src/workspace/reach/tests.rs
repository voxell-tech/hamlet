use hamlet_messages::workspace::reach::{Project, Trust};

use super::*;

const AGENT: AgentNumber = AgentNumber(7);
const STARTER: AgentNumber = AgentNumber(3);
const OTHER: AgentNumber = AgentNumber(9);
const V1: SecretsVersion = SecretsVersion(1);

fn home() -> RealmFolder {
    RealmFolder::new(Path::new("/realm")).unwrap()
}

fn project(mode: Mode) -> Option<Project> {
    Some(Project {
        folder: "/work/project".into(),
        mode,
    })
}

fn footing(project: Option<Project>) -> Footing {
    Footing {
        project,
        network: false,
        trust: Vec::new(),
        secrets: V1,
    }
}

fn conversation(id: &str) -> ConversationId {
    ConversationId(id.into())
}

fn command<'a>(
    conversation: &'a ConversationId,
    script: &'a str,
) -> Op<'a> {
    Op::Command {
        conversation,
        script,
        network: false,
    }
}

fn change<'a>(
    conversation: &'a ConversationId,
    path: &'a Path,
) -> Op<'a> {
    Op::Change { conversation, path }
}

fn passed(reach: &Reach, op: &Op<'_>) -> Gate {
    gate(&home(), AGENT, Some(reach), Some(V1), op)
}

fn command_policy(reach: &Reach) -> Placement {
    placement(
        &home(),
        AGENT,
        reach,
        Purpose::Command {
            network: Network::Off,
        },
    )
    .unwrap()
}

fn paths(list: &[&str]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

/// C18, C20: an agent with a project folder writes it and
/// its own filesystem, and reads the machine but the realm
/// folder, with the chief's environment.
#[test]
fn an_agent_with_a_project_works_in_it() {
    let reach = Reach::Agent(footing(project(Mode::Auto)));
    let placed = command_policy(&reach);
    assert_eq!(
        placed.policy.rw,
        paths(&["/work/project", "/realm/actors/agent/7.fs"])
    );
    assert_eq!(
        placed.policy.ro,
        paths(&[
            "/realm/library",
            "/realm/tools",
            "/realm/actors/workspace/local.output/7",
        ])
    );
    assert!(placed.policy.open);
    assert_eq!(placed.policy.hidden, paths(&["/realm"]));
    assert_eq!(placed.policy.env, host_env());
    assert_eq!(placed.cwd, PathBuf::from("/work/project"));
}

/// C18, C16: the Hand or a hire with no folder works in its
/// own filesystem, which is its home, and reads nothing else of
/// the machine.
#[test]
fn an_agent_with_no_project_works_in_its_filesystem() {
    let reach = Reach::Agent(footing(None));
    let placed = command_policy(&reach);
    assert_eq!(
        placed.policy.rw,
        paths(&["/realm/actors/agent/7.fs"])
    );
    assert!(!placed.policy.open);
    assert_eq!(
        placed.policy.env.get("HOME").map(String::as_str),
        Some("/realm/actors/agent/7.fs")
    );
    assert_eq!(placed.cwd, PathBuf::from("/realm/actors/agent/7.fs"));
}

/// H42, C18: a worker writes its scratch folder and the
/// project, and reads its starter's filesystem, never its own
/// or another agent's.
#[test]
fn a_worker_reads_only_its_starters_filesystem() {
    let reach = Reach::Worker {
        starter: Some(STARTER),
        footing: footing(project(Mode::Auto)),
    };
    let placed = command_policy(&reach);
    assert_eq!(
        placed.policy.rw,
        paths(&["/work/project", "/realm/workers/7"])
    );
    assert!(
        placed
            .policy
            .ro
            .contains(&PathBuf::from("/realm/actors/agent/3.fs"))
    );
    let reads = |path: &str| {
        gate(
            &home(),
            AGENT,
            Some(&reach),
            Some(V1),
            &Op::Read {
                path: Path::new(path),
            },
        )
    };
    assert_eq!(
        reads("/realm/actors/agent/3.fs/memory/notes.md"),
        Gate::Run {
            network: Network::Off
        }
    );
    for path in [
        "/realm/actors/agent/9.fs/memory/notes.md",
        "/realm/actors/agent/7.fs/memory/notes.md",
    ] {
        assert_eq!(
            reads(path),
            Gate::Refuse(Refused::Read(
                PathRefusal::AgentFilesystem { path: path.into() }
            ))
        );
    }
}

/// a worker with no project works in its scratch folder,
/// which is its home.
#[test]
fn a_worker_with_no_project_works_in_its_scratch_folder() {
    let reach = Reach::Worker {
        starter: None,
        footing: footing(None),
    };
    let placed = command_policy(&reach);
    assert_eq!(placed.policy.rw, paths(&["/realm/workers/7"]));
    assert!(!placed.policy.open);
    assert_eq!(placed.cwd, PathBuf::from("/realm/workers/7"));
    assert_eq!(
        placed.policy.env.get("HOME").map(String::as_str),
        Some("/realm/workers/7")
    );
}

/// a file tool's policy does not write the agent's own
/// filesystem, so a link into it is refused when the work checks
/// the real path.
#[test]
fn a_file_tool_never_writes_an_agents_filesystem() {
    let reach = Reach::Agent(footing(project(Mode::Auto)));
    let placed =
        placement(&home(), AGENT, &reach, Purpose::FileTool).unwrap();
    assert_eq!(placed.policy.rw, paths(&["/work/project"]));
    let talk = conversation("c");
    for agent in [AGENT, OTHER] {
        let path = home().filesystem(agent).join("memory/notes.md");
        assert_eq!(
            passed(&reach, &change(&talk, &path)),
            Gate::Refuse(Refused::Change(
                PathRefusal::AgentFilesystem { path }
            ))
        );
    }
}

/// A change outside the reach is refused with its path, before
/// any chief is asked, also in manual mode.
#[test]
fn a_change_outside_the_reach_names_the_path() {
    let reach = Reach::Agent(footing(project(Mode::Manual)));
    let talk = conversation("c");
    for path in ["/etc/passwd", "/work/project/../elsewhere/a.txt"] {
        assert_eq!(
            passed(&reach, &change(&talk, Path::new(path))),
            Gate::Refuse(Refused::Change(
                PathRefusal::OutsideReach {
                    path: normalize(path).into(),
                    access: ToChange::Change,
                }
            ))
        );
    }
}

/// A dream's errand works in its main, writes its hire's side
/// too, and has its hire's network, asking for nothing.
#[test]
fn a_dream_works_in_its_main_with_its_hires_network() {
    let dream = |network| Reach::Dream {
        main: "/realm/dreams/1/main".into(),
        hire: Some("/realm/dreams/1/hire".into()),
        network,
        secrets: V1,
    };
    let talk = conversation("c");
    let pull = command(&talk, "git pull");
    let placed = placement(
        &home(),
        AGENT,
        &dream(true),
        Purpose::Command {
            network: Network::On,
        },
    )
    .unwrap();

    assert_eq!(placed.cwd, PathBuf::from("/realm/dreams/1/main"));
    assert_eq!(
        placed.policy.rw,
        paths(&["/realm/dreams/1/main", "/realm/dreams/1/hire"])
    );
    assert!(placed.policy.net);
    assert_eq!(
        [passed(&dream(true), &pull), passed(&dream(false), &pull)],
        [
            Gate::Run {
                network: Network::On
            },
            Gate::Run {
                network: Network::Off
            },
        ]
    );
}

/// H11, C19: a networked command of an agent with no network in
/// manual mode asks once for both; the network is asked first.
#[test]
fn the_network_is_asked_before_manual_mode() {
    let talk = conversation("c");
    let manual = Reach::Agent(footing(project(Mode::Manual)));
    assert_eq!(
        passed(&manual, &command(&talk, "curl https://example.com")),
        Gate::NeedsChief(vec![Need::Network, Need::Command])
    );
    let auto = Reach::Agent(footing(project(Mode::Auto)));
    assert_eq!(
        passed(
            &auto,
            &Op::Command {
                conversation: &talk,
                script: "make",
                network: true,
            }
        ),
        Gate::NeedsChief(vec![Need::Network])
    );
    assert_eq!(
        passed(&auto, &command(&talk, "echo curly")),
        Gate::Run {
            network: Network::Off
        }
    );
    let mut networked = footing(project(Mode::Auto));
    networked.network = true;
    assert_eq!(
        passed(&Reach::Agent(networked), &command(&talk, "git pull")),
        Gate::Run {
            network: Network::On
        }
    );
}

/// H11, C21: trust for a conversation lets its commands and edits
/// run in manual mode, and no other conversation's.
#[test]
fn trust_holds_for_its_conversation_only() {
    let mut trusted = footing(project(Mode::Manual));
    trusted.trust.push(Trust {
        conversation: conversation("a"),
        edits: true,
        commands: false,
    });
    let reach = Reach::Agent(trusted);
    let file = Path::new("/work/project/src/main.rs");
    let (a, b) = (conversation("a"), conversation("b"));
    assert_eq!(
        passed(&reach, &change(&a, file)),
        Gate::Run {
            network: Network::Off
        }
    );
    assert_eq!(
        passed(&reach, &change(&b, file)),
        Gate::NeedsChief(vec![Need::Edit])
    );
    assert_eq!(
        passed(&reach, &command(&a, "ls")),
        Gate::NeedsChief(vec![Need::Command])
    );
}

/// C21, H42: manual mode asks for edits of the folder only, never
/// of a worker's scratch folder.
#[test]
fn a_workers_scratch_folder_is_never_manual() {
    let reach = Reach::Worker {
        starter: Some(STARTER),
        footing: footing(project(Mode::Manual)),
    };
    let talk = conversation("c");
    assert_eq!(
        passed(
            &reach,
            &change(&talk, Path::new("/realm/workers/7/a"))
        ),
        Gate::Run {
            network: Network::Off
        }
    );
    assert_eq!(
        passed(&reach, &change(&talk, Path::new("/work/project/a"))),
        Gate::NeedsChief(vec![Need::Edit])
    );
}

#[test]
fn an_ended_or_missing_reach_refuses_every_call() {
    let talk = conversation("c");
    let op = command(&talk, "ls");
    for reach in [None, Some(&Reach::Ended)] {
        assert_eq!(
            gate(&home(), AGENT, reach, Some(V1), &op),
            Gate::Refuse(Refused::Reach(ReachRefusal::NoReach))
        );
    }
    assert_eq!(
        placement(&home(), AGENT, &Reach::Ended, Purpose::FileTool),
        None
    );
}

/// C32: no call passes while the workspace's list of secrets is
/// older than the reach's, or missing after a restart, even one
/// that would wait for a chief. A refusal needs no list.
#[test]
fn a_call_waits_for_the_secrets_in_force() {
    let mut ahead = footing(project(Mode::Manual));
    ahead.secrets = SecretsVersion(2);
    let reach = Reach::Agent(ahead);
    let talk = conversation("c");
    for held in [None, Some(V1)] {
        assert_eq!(
            gate(
                &home(),
                AGENT,
                Some(&reach),
                held,
                &command(&talk, "ls")
            ),
            Gate::NeedsSecrets(SecretsVersion(2))
        );
    }
    let outside = Path::new("/etc/passwd");
    assert!(matches!(
        gate(
            &home(),
            AGENT,
            Some(&reach),
            None,
            &change(&talk, outside)
        ),
        Gate::Refuse(Refused::Change(
            PathRefusal::OutsideReach { .. }
        ))
    ));
    assert_eq!(
        gate(
            &home(),
            AGENT,
            Some(&reach),
            Some(SecretsVersion(2)),
            &command(&talk, "ls")
        ),
        Gate::NeedsChief(vec![Need::Command])
    );
}
