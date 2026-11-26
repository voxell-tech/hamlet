use std::time::{Duration, Instant};

use hamlet_actor::Action;
use hamlet_host::{Address, Host};
use hamlet_messages::agent::Outcome;
use hamlet_messages::workspace::files::{ChangeAnswer, ChangeEnd};
use hamlet_messages::workspace::reach::Reach;
use hamlet_messages::workspace::refusal::{ReachRefusal, WaitEnd};
use hamlet_messages::workspace::{
    AgentNumber, Answer, AnswerTaken, CallId, Cancel, CancelAgent,
    Cancelled, ConversationId, Exit, InForce, Lapse, ReachVersion,
    RealmTask, Run, RunAnswer, RunEnd, SetReach, Which,
};
use hamlet_testing::Scratch;

use super::changes::{edit, manual, read, write};
use super::files::{ask, project};
use super::waits::{chief, waits};
use super::{
    AGENT, Leave, WRONG_CALLER, agent, agent_runs, answering_within,
    cut_off, error_of, filesystem, host_with, ran, reach, realm,
    realm_sets, run, running, runs_as, status_as, told, told_ends,
    told_within,
};

fn cancel_agent(which: Which) -> CancelAgent {
    CancelAgent {
        agent: AGENT,
        which,
    }
}

async fn agent_cancels(host: &Host, which: Which) -> Cancelled {
    ask(host, &agent(AGENT), &Cancel { which }).await
}

/// Asks the workspace, as the realm, to end agent 7's commands of
/// `which`.
async fn realm_cancels(host: &Host, which: Which) -> Cancelled {
    ask(host, &realm(), &cancel_agent(which)).await
}

fn one(call: &str) -> Which {
    Which::One {
        call: CallId(call.to_owned()),
    }
}

/// A run of `script` as call `call` in the conversation
/// `conversation`.
fn run_in(conversation: &str, call: &str, script: &str) -> Run {
    Run {
        conversation: ConversationId(conversation.to_owned()),
        ..run(call, script)
    }
}

/// A cancel of a command that answered `Running` ends it within a
/// second: its process is gone, the cancel answers its id, and the
/// agent is called once with its end as cancelled, which its status
/// answers too.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_ends_a_running_command_and_the_agent_is_told_once()
{
    let dir = Scratch::new("workspace-cancel-running");
    let fs = filesystem(&dir);
    let peers = answering_within(300);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command = running(
        &agent_runs(
            &host,
            &run("c1", "echo started; sleep 2; touch late"),
        )
        .await,
    );

    let asked = Instant::now();
    let answer = agent_cancels(&host, one("c1")).await;
    let took = asked.elapsed();
    told_within(&peers, 1, Duration::from_secs(3)).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let status = status_as(&host, AGENT, command).await;
    host.shutdown().await.unwrap();

    assert_eq!(
        answer,
        Cancelled {
            commands: vec![command]
        }
    );
    assert!(took < Duration::from_secs(1), "{took:?}");
    let told = told(&peers);
    assert_eq!(told.len(), 1, "{told:?}");
    let end = ran(&told[0].1);
    assert_eq!(told[0].1.call, CallId("c1".into()));
    assert_eq!(
        (&end.exit, end.output.as_str()),
        (&Exit::Cancelled, "started\n")
    );
    assert!(!fs.join("late").exists(), "the command ran on");
    assert_eq!(
        status,
        RunAnswer::Ended {
            end: RunEnd::Ran(end.clone())
        }
    );
}

/// A cancel while the call still waits for its command ends it, and
/// the call answers `Ended` as cancelled: the agent hears the end
/// once, so it is not called.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_within_the_wait_answers_the_call() {
    let dir = Scratch::new("workspace-cancel-waiting");
    let fs = filesystem(&dir);
    let peers = answering_within(5000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let call = {
        let host = host.clone();
        tokio::spawn(async move {
            agent_runs(&host, &run("c1", "sleep 2; touch late")).await
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;

    let asked = Instant::now();
    let answer = realm_cancels(&host, Which::All).await;
    let answered = call.await.unwrap();
    let took = asked.elapsed();
    tokio::time::sleep(Duration::from_millis(2000)).await;
    host.shutdown().await.unwrap();

    let RunAnswer::Ended {
        end: RunEnd::Ran(end),
    } = answered
    else {
        panic!("the call answered {answered:?}");
    };
    assert_eq!(end.exit, Exit::Cancelled);
    assert_eq!(
        answer,
        Cancelled {
            commands: vec![end.command]
        }
    );
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert_eq!(told(&peers), []);
    assert!(!fs.join("late").exists(), "the command ran on");
}

/// A cancel of a conversation ends that conversation's commands
/// only; a cancel of all ends the rest.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_ends_a_conversations_commands_or_all() {
    let dir = Scratch::new("workspace-cancel-conversation");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let mut commands = Vec::new();
    for (conversation, call) in
        [("a", "c1"), ("a", "c2"), ("b", "c3")]
    {
        commands.push(running(
            &agent_runs(
                &host,
                &run_in(conversation, call, "sleep 5"),
            )
            .await,
        ));
    }

    let of_a = realm_cancels(
        &host,
        Which::Conversation {
            conversation: ConversationId("a".into()),
        },
    )
    .await;
    let b_meanwhile = status_as(&host, AGENT, commands[2]).await;
    let all = agent_cancels(&host, Which::All).await;
    told_within(&peers, 3, Duration::from_secs(3)).await;
    host.shutdown().await.unwrap();

    let mut of_a = of_a.commands;
    of_a.sort_by_key(|command| command.0);
    assert_eq!(of_a, commands[..2]);
    assert_eq!(
        b_meanwhile,
        RunAnswer::Running {
            command: commands[2]
        }
    );
    assert_eq!(
        all,
        Cancelled {
            commands: vec![commands[2]]
        }
    );
    let told = told(&peers);
    assert_eq!(told.len(), 3, "{told:?}");
    assert!(
        told.iter().all(
            |(_, finished)| ran(finished).exit == Exit::Cancelled
        ),
        "{told:?}"
    );
}

/// A cancel that finds no running command answers none and changes
/// nothing: an unknown call, a command that ended, or no command at
/// all.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_of_nothing_does_nothing() {
    let dir = Scratch::new("workspace-cancel-nothing");
    filesystem(&dir);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let before = realm_cancels(&host, Which::All).await;
    let unknown = agent_cancels(&host, one("nope")).await;
    let RunAnswer::Ended {
        end: RunEnd::Ran(end),
    } = agent_runs(&host, &run("c1", "echo hi")).await
    else {
        panic!("echo did not answer Ended");
    };
    let ended = agent_cancels(&host, one("c1")).await;
    let status = status_as(&host, AGENT, end.command).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.shutdown().await.unwrap();

    let none = Cancelled {
        commands: Vec::new(),
    };
    assert_eq!(
        [before, unknown, ended],
        [none.clone(), none.clone(), none]
    );
    assert_eq!(end.exit, Exit::Code { code: 0 });
    assert_eq!(
        status,
        RunAnswer::Ended {
            end: RunEnd::Ran(end)
        }
    );
    assert_eq!(told(&peers), []);
}

/// The commands of a host that stopped died with it, so a cancel
/// after the restart finds nothing, and the command reads cut off.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_after_a_restart_finds_nothing() {
    let dir = Scratch::new("workspace-cancel-restart");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command =
        running(&agent_runs(&host, &run("c1", "sleep 30")).await);
    host.shutdown().await.unwrap();
    let again = host_with(&dir, peers.clone()).await;
    let answer = realm_cancels(&again, Which::All).await;
    let status = status_as(&again, AGENT, command).await;
    told_within(&peers, 1, Duration::from_secs(5)).await;
    again.shutdown().await.unwrap();

    assert_eq!(
        answer,
        Cancelled {
            commands: Vec::new()
        }
    );
    assert!(
        matches!(&status, RunAnswer::Ended { end: RunEnd::Ran(end) } if end.exit == Exit::CutOff),
        "{status:?}"
    );
    assert_eq!(
        told_ends(&peers),
        [("c1".to_owned(), cut_off(command))]
    );
}

/// An agent's cancel ends only its own commands, and only the realm
/// cancels another agent's: an agent and another kind sending
/// `cancel_agent`, or another kind sending `cancel`, fail as wrong
/// callers, and the command runs on.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_ends_the_callers_commands_or_the_realm_names_them()
{
    let dir = Scratch::new("workspace-cancel-caller");
    filesystem(&dir);
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    let command =
        running(&agent_runs(&host, &run("c1", "sleep 5")).await);
    let other = agent(AgentNumber(8));
    let stranger = Address::new("other", "x").unwrap();
    let by_other =
        ask(&host, &other, &Cancel { which: Which::All }).await;
    let refused = [
        error_of(&host, &other, &cancel_agent(Which::All)).await,
        error_of(&host, &agent(AGENT), &cancel_agent(Which::All))
            .await,
        error_of(&host, &stranger, &cancel_agent(Which::All)).await,
        error_of(&host, &stranger, &Cancel { which: Which::All })
            .await,
    ];
    let meanwhile = status_as(&host, AGENT, command).await;
    let by_the_realm = realm_cancels(&host, Which::All).await;
    told_within(&peers, 1, Duration::from_secs(3)).await;
    host.shutdown().await.unwrap();

    assert_eq!(by_other, Cancelled { commands: vec![] });
    assert_eq!(refused, [WRONG_CALLER; 4]);
    assert_eq!(meanwhile, RunAnswer::Running { command });
    assert_eq!(
        by_the_realm,
        Cancelled {
            commands: vec![command]
        }
    );
}

/// Calls the workspace with a [`SetReach`] of agent `number`, as
/// the realm.
pub(super) async fn realm_sets_for(
    host: &Host,
    number: AgentNumber,
    version: u64,
    reach: Reach,
) -> InForce {
    ask(
        host,
        &realm(),
        &SetReach {
            agent: number,
            version: ReachVersion(version),
            reach,
        },
    )
    .await
}

/// when an agent's reach ends, its running commands end in the
/// handling of that call, so none runs on once the realm has its
/// answer, and the agent is told each once. Another agent's command
/// runs on, and the ended agent starts no command.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_reach_ends_the_agents_commands() {
    let dir = Scratch::new("workspace-ended-commands");
    let fs = filesystem(&dir);
    let other = AgentNumber(8);
    let other_fs = dir.join("actors/agent/8.fs");
    std::fs::create_dir_all(&other_fs).unwrap();
    let peers = answering_within(200);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, reach(None)).await;
    realm_sets_for(&host, other, 1, reach(None)).await;
    let command = running(
        &agent_runs(&host, &run("c1", "sleep 1.5; touch late")).await,
    );
    let others = run("c1", "sleep 1.5; touch late");
    running(&runs_as(&host, other, &others).await);

    let asked = Instant::now();
    let ended = realm_sets(&host, 2, Reach::Ended).await;
    let took = asked.elapsed();
    let after = agent_runs(&host, &run("c2", "touch after")).await;
    told_within(&peers, 2, Duration::from_secs(3)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let seven = agent(AgentNumber(7));
    host.get_or_start(&seven).await.unwrap();
    host.shutdown().await.unwrap();

    assert_eq!(
        ended,
        InForce {
            version: ReachVersion(2)
        }
    );
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert!(
        !fs.join("late").exists(),
        "the ended agent's command ran on"
    );
    assert!(
        other_fs.join("late").exists(),
        "the other agent's ended"
    );
    assert_eq!(
        after,
        RunAnswer::Refused {
            why: ReachRefusal::NoReach.into()
        }
    );
    assert!(!fs.join("after").exists());
    let told = told(&peers);
    let to_seven = told
        .iter()
        .filter(|(to, _)| *to == seven.to_string())
        .map(|(_, finished)| (finished.call.clone(), ran(finished)))
        .collect::<Vec<_>>();
    assert_eq!(to_seven.len(), 1, "{told:?}");
    assert_eq!(to_seven[0].0, CallId("c1".into()));
    assert_eq!(
        (to_seven[0].1.command, &to_seven[0].1.exit),
        (command, &Exit::Cancelled)
    );
}

/// The end an ended reach tells of a call's closed wait carries the
/// realm's task the call was made under.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_closed_waits_end_carries_its_calls_task() {
    let dir = Scratch::new("workspace-ended-task");
    filesystem(&dir);
    let (_project, folder) = project("workspace-ended-task-p", &[]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let task = RealmTask("task-1".into());
    let call = Run {
        realm_task: Some(task.clone()),
        ..run("c1", "touch marker")
    };
    agent_runs(&host, &call).await;

    realm_sets(&host, 2, Reach::Ended).await;
    told_within(&peers, 1, Duration::from_secs(3)).await;
    host.shutdown().await.unwrap();

    let told = told(&peers)
        .into_iter()
        .map(|(_, finished)| (finished.call, finished.task))
        .collect::<Vec<_>>();
    assert_eq!(told, [(CallId("c1".into()), Some(task))]);
}

/// an ended reach closes the agent's waits for a chief in the
/// same change, a command's and a change's alike (M-E7): the agent
/// is told each call's wait lapsed, by its kind, a chief's allow a
/// moment later finds no wait and starts nothing, and a repeat of
/// the call answers that it lapsed.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_ended_reach_closes_the_agents_waits() {
    let dir = Scratch::new("workspace-ended-waits");
    filesystem(&dir);
    let (_project, folder) =
        project("workspace-ended-waits-p", &[("a.md", "a")]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    let call = run("c1", "touch marker");
    let me = agent(AGENT);
    let file = folder.join("a.md");
    let asked_write = write("w1", &file, "b");
    let waiting = agent_runs(&host, &call).await;
    let write_waits = ask(&host, &me, &asked_write).await;
    let edit_waits =
        ask(&host, &me, &edit("e1", &file, &[("a", "c")])).await;

    let ended = realm_sets(&host, 2, Reach::Ended).await;
    let allowed = chief(&host, "c1", Answer::Allow).await;
    let write_allowed = chief(&host, "w1", Answer::Allow).await;
    let repeat = agent_runs(&host, &call).await;
    let write_repeat = ask(&host, &me, &asked_write).await;
    told_within(&peers, 3, Duration::from_secs(3)).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    host.shutdown().await.unwrap();

    assert!(
        matches!(waiting, RunAnswer::WaitsForChief { .. }),
        "{waiting:?}"
    );
    assert!(
        matches!(write_waits, ChangeAnswer::WaitsForChief { .. }),
        "{write_waits:?}"
    );
    assert!(
        matches!(edit_waits, ChangeAnswer::WaitsForChief { .. }),
        "{edit_waits:?}"
    );
    assert_eq!(
        ended,
        InForce {
            version: ReachVersion(2)
        }
    );
    assert_eq!(
        (allowed, write_allowed),
        (AnswerTaken::NoWait, AnswerTaken::NoWait)
    );
    assert_eq!(waits(&dir), 0);
    let lapse = WaitEnd::Lapsed {
        lapse: Lapse::Ended,
    };
    let lapsed = RunEnd::NotRun {
        why: lapse.clone().into(),
    };
    let change_lapsed = ChangeEnd::NotRun { why: lapse.into() };
    assert_eq!(
        repeat,
        RunAnswer::Ended {
            end: lapsed.clone()
        }
    );
    assert_eq!(
        write_repeat,
        ChangeAnswer::Ended {
            end: change_lapsed.clone()
        }
    );
    let mut told = told(&peers)
        .into_iter()
        .map(|(_, finished)| (finished.call, finished.outcome))
        .collect::<Vec<_>>();
    told.sort_by(|(a, _), (b, _)| a.0.cmp(&b.0));
    assert_eq!(
        told,
        [
            (CallId("c1".into()), Outcome::Run { end: lapsed }),
            (
                CallId("e1".into()),
                Outcome::Change {
                    end: change_lapsed.clone()
                }
            ),
            (
                CallId("w1".into()),
                Outcome::Change { end: change_lapsed }
            ),
        ]
    );
    assert!(!folder.join("marker").exists());
    assert_eq!(read(&file), "a");
}

/// A chief's allow and the agent's end, called at once, never let a
/// command of the ended agent run: whichever comes first, the agent
/// is told once, and the command's file is never made. Every other
/// round calls with the end a moment later, so both orders occur.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_allow_racing_an_ended_reach_runs_nothing() {
    let dir = Scratch::new("workspace-ended-race");
    filesystem(&dir);
    let (_project, folder) = project("workspace-ended-race-p", &[]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    let rounds = 20;
    for round in 0..rounds {
        let version = 2 * round + 1;
        realm_sets(&host, version, manual(&folder)).await;
        let call = format!("c{round}");
        let script = format!("sleep 0.5; touch marker{round}");
        let waiting = agent_runs(&host, &run(&call, &script)).await;
        assert!(
            matches!(waiting, RunAnswer::WaitsForChief { .. }),
            "{waiting:?}"
        );
        let allow = {
            let (host, call) = (host.clone(), call.clone());
            tokio::spawn(async move {
                chief(&host, &call, Answer::Allow).await
            })
        };
        // A moment later in every other round, so the allow comes
        // first there.
        if round % 2 == 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let ended =
            realm_sets(&host, version + 1, Reach::Ended).await;
        let allowed = allow.await.unwrap();
        assert_eq!(
            ended,
            InForce {
                version: ReachVersion(version + 1)
            }
        );
        assert!(
            matches!(
                allowed,
                AnswerTaken::Taken | AnswerTaken::NoWait
            ),
            "round {round}: {allowed:?}"
        );
    }
    told_within(&peers, rounds as usize, Duration::from_secs(3))
        .await;
    tokio::time::sleep(Duration::from_millis(1000)).await;
    host.shutdown().await.unwrap();

    let made = (0..rounds)
        .filter(|round| {
            folder.join(format!("marker{round}")).exists()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        made,
        Vec::<u64>::new(),
        "the ended agent's commands ran"
    );
    let told = told(&peers);
    assert_eq!(told.len(), rounds as usize, "{told:?}");
    for (_, finished) in told {
        assert!(
            matches!(
                &finished.outcome,
                Outcome::Run { end: RunEnd::Ran(end) } if end.exit == Exit::Cancelled
            ) || finished.outcome
                == Outcome::Run {
                    end: RunEnd::NotRun {
                        why: WaitEnd::Lapsed {
                            lapse: Lapse::Ended
                        }
                        .into()
                    }
                },
            "{finished:?}"
        );
    }
}

/// A call does not make its target: when the ended agent is gone, the
/// notice of its closed wait fails and is dropped, and the realm
/// still has its answer.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn the_notice_to_an_agent_that_is_gone_is_dropped() {
    let dir = Scratch::new("workspace-ended-gone");
    filesystem(&dir);
    let (_project, folder) = project("workspace-ended-gone-p", &[]);
    let peers = answering_within(2000);
    let host = host_with(&dir, peers.clone()).await;
    realm_sets(&host, 1, manual(&folder)).await;
    agent_runs(&host, &run("c1", "touch marker")).await;
    host.call(
        &agent(AGENT),
        Leave::NAME,
        hamlet_actor::action::encode_positional(&Leave).unwrap(),
        Duration::from_secs(10),
    )
    .await
    .unwrap();

    let ended = realm_sets(&host, 2, Reach::Ended).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let agent_file = dir.join("actors/agent/7.sqlite").exists();
    host.shutdown().await.unwrap();

    assert_eq!(
        ended,
        InForce {
            version: ReachVersion(2)
        }
    );
    assert_eq!(waits(&dir), 0);
    assert_eq!(told(&peers), []);
    assert!(!agent_file, "the notice made the agent again");
}
